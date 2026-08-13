use crate::{TranscribeOptions, TranscriptResult};
use anyhow::Context;
use fennec_core::whisper_filter::{filter_segments, joined_text, to_timed_segments, WhisperSegment};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub type ProgressCallback = Arc<dyn Fn(f64) + Send + Sync>;

/// whisper-rs の `use_gpu` 既定値は `cfg!(feature = "_gpu")` なので、cargo feature ではなく
/// build.rs 側で GPU バックエンドを有効にした場合は false のままになる。明示的に立てる。
/// バックエンドが無いビルドでは ggml が CPU にフォールバックするので常に true でよい。
pub(crate) fn context_params() -> WhisperContextParameters<'static> {
    let mut params = WhisperContextParameters::default();
    params.use_gpu(true);
    params
}

pub struct WhisperBatchTranscriber {
    loaded: Option<(PathBuf, WhisperContext)>,
}

impl Default for WhisperBatchTranscriber {
    fn default() -> Self {
        Self::new()
    }
}

impl WhisperBatchTranscriber {
    pub fn new() -> Self {
        Self { loaded: None }
    }

    pub fn transcribe(
        &mut self,
        audio: &Path,
        opts: &TranscribeOptions,
        on_progress: ProgressCallback,
        cancelled: Arc<AtomicBool>,
    ) -> anyhow::Result<TranscriptResult> {
        if fennec_audio::is_silent_audio(audio, 0.001).unwrap_or(true) {
            return Ok(TranscriptResult::default());
        }

        let samples = fennec_audio::decode_to_mono_16k(audio)
            .with_context(|| format!("failed to decode {}", audio.display()))?;
        if samples.is_empty() {
            return Ok(TranscriptResult::default());
        }

        let ctx = self.context_for(&opts.model_path)?;
        let mut state = ctx.create_state().context("failed to create whisper state")?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(&opts.language));
        params.set_suppress_blank(true);
        params.set_no_speech_thold(0.3);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        let progress = on_progress.clone();
        params.set_progress_callback_safe(move |p: i32| {
            progress((p as f64 / 100.0).min(0.99));
        });
        let abort_flag = cancelled.clone();
        // whisper-rs 0.14のset_abort_callback_safeはBox<dyn FnMut>以外のF型で
        // trampolineとuser_dataの型が食い違いUBになる（error -6の原因）ため明示Boxで渡す
        let abort: Box<dyn FnMut() -> bool> =
            Box::new(move || abort_flag.load(Ordering::Relaxed));
        params.set_abort_callback_safe(abort);

        if let Err(e) = state.full(params, &samples) {
            if cancelled.load(Ordering::Relaxed) {
                anyhow::bail!("cancelled");
            }
            return Err(anyhow::Error::new(e).context("whisper transcription failed"));
        }
        if cancelled.load(Ordering::Relaxed) {
            anyhow::bail!("cancelled");
        }

        let n = state.full_n_segments()?;
        let mut segments = Vec::with_capacity(n.max(0) as usize);
        for i in 0..n {
            let Ok(text) = state.full_get_segment_text_lossy(i) else {
                continue;
            };
            let t0 = state.full_get_segment_t0(i).unwrap_or(0);
            let t1 = state.full_get_segment_t1(i).unwrap_or(t0);

            let token_count = state.full_n_tokens(i).unwrap_or(0);
            let avg_logprob = if token_count > 0 {
                let mut sum = 0.0f32;
                let mut counted = 0u32;
                for j in 0..token_count {
                    if let Ok(p) = state.full_get_token_prob(i, j) {
                        sum += p.max(1e-10).ln();
                        counted += 1;
                    }
                }
                if counted > 0 {
                    sum / counted as f32
                } else {
                    0.0
                }
            } else {
                0.0
            };

            segments.push(WhisperSegment {
                text,
                start: t0 as f64 * 0.01,
                end: t1 as f64 * 0.01,
                // whisper-rs 0.14はセグメント単位のno_speech_probを公開しないため、
                // no_speech_tholdをデコーダに渡すことで代替し、ここでは0固定にする
                no_speech_prob: 0.0,
                avg_logprob,
            });
        }

        let filtered = filter_segments(segments);
        on_progress(1.0);
        Ok(TranscriptResult {
            text: joined_text(&filtered),
            segments: to_timed_segments(&filtered),
        })
    }

    fn context_for(&mut self, model_path: &Path) -> anyhow::Result<&WhisperContext> {
        let reload = match &self.loaded {
            Some((path, _)) => path != model_path,
            None => true,
        };
        if reload {
            let ctx = WhisperContext::new_with_params(
                model_path
                    .to_str()
                    .context("model path is not valid utf-8")?,
                context_params(),
            )
            .with_context(|| format!("failed to load whisper model {}", model_path.display()))?;
            self.loaded = Some((model_path.to_path_buf(), ctx));
        }
        Ok(&self.loaded.as_ref().unwrap().1)
    }
}
