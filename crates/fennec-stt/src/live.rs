use anyhow::Context;
use fennec_core::types::TimedSegment;
use fennec_core::whisper_filter::HALLUCINATION_PATTERNS;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

const SAMPLE_RATE: usize = 16000;
const BLOCK: usize = SAMPLE_RATE / 10;
const VAD_RMS_THRESHOLD: f32 = 0.01;
const SILENCE_END_BLOCKS: usize = 7;
const MAX_UTTERANCE_SECS: usize = 25;
const MIN_UTTERANCE_SAMPLES: usize = SAMPLE_RATE / 2;
const PARTIAL_INTERVAL_SAMPLES: u64 = (SAMPLE_RATE * 2) as u64;

#[derive(Debug, Clone)]
pub enum LiveEvent {
    Partial(String),
    Final(TimedSegment),
}

pub struct LiveEngine {
    ctx: WhisperContext,
    decode_lock: Mutex<()>,
    language: String,
}

impl LiveEngine {
    pub fn new(model_path: &Path, language: &str) -> anyhow::Result<Arc<Self>> {
        let ctx = WhisperContext::new_with_params(
            model_path
                .to_str()
                .context("model path is not valid utf-8")?,
            WhisperContextParameters::default(),
        )
        .with_context(|| format!("failed to load whisper model {}", model_path.display()))?;
        Ok(Arc::new(Self {
            ctx,
            decode_lock: Mutex::new(()),
            language: language.to_string(),
        }))
    }

    fn decode(&self, samples: &[f32]) -> anyhow::Result<String> {
        let _guard = self.decode_lock.lock().unwrap();
        let mut state = self.ctx.create_state()?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(&self.language));
        params.set_suppress_blank(true);
        params.set_no_speech_thold(0.3);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_single_segment(true);
        state.full(params, samples)?;
        let n = state.full_n_segments()?;
        let mut text = String::new();
        for i in 0..n {
            if let Ok(t) = state.full_get_segment_text_lossy(i) {
                text.push_str(&t);
            }
        }
        Ok(text.trim().to_string())
    }
}

struct LaneState {
    pending: Vec<f32>,
    utterance: Vec<f32>,
    utterance_start: u64,
    total_samples: u64,
    silence_blocks: usize,
    in_speech: bool,
    finals: Vec<TimedSegment>,
    last_partial_at: u64,
}

pub struct WhisperLiveTranscriber {
    lane: Arc<Mutex<LaneState>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    engine: Arc<LiveEngine>,
    on_event: Arc<dyn Fn(LiveEvent) + Send + Sync>,
}

impl WhisperLiveTranscriber {
    pub fn start(
        engine: Arc<LiveEngine>,
        on_event: Arc<dyn Fn(LiveEvent) + Send + Sync>,
    ) -> Self {
        let lane = Arc::new(Mutex::new(LaneState {
            pending: Vec::new(),
            utterance: Vec::new(),
            utterance_start: 0,
            total_samples: 0,
            silence_blocks: 0,
            in_speech: false,
            finals: Vec::new(),
            last_partial_at: 0,
        }));
        let stop = Arc::new(AtomicBool::new(false));

        let worker_lane = lane.clone();
        let worker_stop = stop.clone();
        let worker_engine = engine.clone();
        let worker_events = on_event.clone();
        let worker = std::thread::Builder::new()
            .name("fennec-live-stt".into())
            .spawn(move || {
                while !worker_stop.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    process_lane(&worker_lane, &worker_engine, &worker_events, false);
                }
            })
            .expect("failed to spawn live stt worker");

        Self {
            lane,
            stop,
            worker: Some(worker),
            engine,
            on_event,
        }
    }

    pub fn feed(&self, samples_16k_mono: &[f32]) {
        let mut lane = self.lane.lock().unwrap();
        lane.pending.extend_from_slice(samples_16k_mono);
    }

    pub fn finish(mut self) -> Vec<TimedSegment> {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        process_lane(&self.lane, &self.engine, &self.on_event, true);
        let lane = self.lane.lock().unwrap();
        lane.finals.clone()
    }
}

impl Drop for WhisperLiveTranscriber {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn process_lane(
    lane: &Arc<Mutex<LaneState>>,
    engine: &LiveEngine,
    on_event: &Arc<dyn Fn(LiveEvent) + Send + Sync>,
    flush: bool,
) {
    loop {
        enum Action {
            None,
            Final { samples: Vec<f32>, start: f64, end: f64 },
            Partial { samples: Vec<f32> },
        }

        let action = {
            let mut lane = lane.lock().unwrap();

            let has_block = lane.pending.len() >= BLOCK;
            if has_block {
                let block: Vec<f32> = lane.pending.drain(..BLOCK).collect();
                let rms = fennec_audio::rms(&block);
                lane.total_samples += BLOCK as u64;

                if rms >= VAD_RMS_THRESHOLD {
                    if !lane.in_speech {
                        lane.in_speech = true;
                        lane.utterance.clear();
                        lane.utterance_start = lane.total_samples - BLOCK as u64;
                        lane.last_partial_at = lane.total_samples;
                    }
                    lane.silence_blocks = 0;
                    lane.utterance.extend_from_slice(&block);
                } else if lane.in_speech {
                    lane.silence_blocks += 1;
                    lane.utterance.extend_from_slice(&block);
                }

                let utterance_done = lane.in_speech
                    && (lane.silence_blocks >= SILENCE_END_BLOCKS
                        || lane.utterance.len() >= MAX_UTTERANCE_SECS * SAMPLE_RATE);
                if utterance_done {
                    let samples = std::mem::take(&mut lane.utterance);
                    let start = lane.utterance_start as f64 / SAMPLE_RATE as f64;
                    let end = lane.total_samples as f64 / SAMPLE_RATE as f64;
                    lane.in_speech = false;
                    lane.silence_blocks = 0;
                    if samples.len() >= MIN_UTTERANCE_SAMPLES {
                        Action::Final { samples, start, end }
                    } else {
                        Action::None
                    }
                } else if lane.in_speech
                    && lane.total_samples - lane.last_partial_at >= PARTIAL_INTERVAL_SAMPLES
                    && lane.utterance.len() >= SAMPLE_RATE
                {
                    lane.last_partial_at = lane.total_samples;
                    Action::Partial {
                        samples: lane.utterance.clone(),
                    }
                } else {
                    Action::None
                }
            } else if flush {
                let mut remaining = std::mem::take(&mut lane.pending);
                if lane.in_speech {
                    let mut samples = std::mem::take(&mut lane.utterance);
                    samples.append(&mut remaining);
                    let start = lane.utterance_start as f64 / SAMPLE_RATE as f64;
                    lane.total_samples += remaining.len() as u64;
                    let end =
                        (lane.utterance_start + samples.len() as u64) as f64 / SAMPLE_RATE as f64;
                    lane.in_speech = false;
                    if samples.len() >= MIN_UTTERANCE_SAMPLES {
                        Action::Final { samples, start, end }
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        };

        match action {
            Action::None => continue,
            Action::Partial { samples } => {
                if let Ok(text) = engine.decode(&samples) {
                    if !text.is_empty() {
                        on_event(LiveEvent::Partial(text));
                    }
                }
            }
            Action::Final { samples, start, end } => {
                match engine.decode(&samples) {
                    Ok(text) => {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() && !HALLUCINATION_PATTERNS.contains(&trimmed) {
                            let segment = TimedSegment::new(trimmed, start, end);
                            lane.lock().unwrap().finals.push(segment.clone());
                            on_event(LiveEvent::Final(segment));
                        } else {
                            on_event(LiveEvent::Partial(String::new()));
                        }
                    }
                    Err(e) => {
                        eprintln!("live decode failed: {e:#}");
                    }
                }
            }
        }
    }
}
