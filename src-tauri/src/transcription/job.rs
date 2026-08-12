use crate::events::{self, TranscribeDone, TranscribeProgress};
use crate::state::AppState;
use fennec_core::format::{build_transcript_text, LabelLanguage};
use fennec_core::merge::{deduplicate_mic_echo, merge_segments};
use fennec_core::session::{
    load_metadata, save_metadata, TranscriptDocument,
};
use fennec_core::types::TimedSegment;
use fennec_stt::{TranscribeOptions, WhisperBatchTranscriber};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

pub struct JobStatus {
    pub phase: std::sync::Mutex<(String, Option<f64>)>,
    pub done: AtomicBool,
    pub error: std::sync::Mutex<Option<String>>,
}

impl JobStatus {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            phase: std::sync::Mutex::new((String::new(), None)),
            done: AtomicBool::new(false),
            error: std::sync::Mutex::new(None),
        })
    }
}

pub struct TranscribeJob {
    pub session_id: String,
    pub cancelled: Arc<AtomicBool>,
    pub status: Arc<JobStatus>,
}

pub fn whisper_language(locale: Option<&str>) -> String {
    let raw = locale.unwrap_or("ja");
    raw.split(['-', '_'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("ja")
        .to_lowercase()
}

pub fn apple_locale(locale: Option<&str>) -> String {
    if let Some(locale) = locale.filter(|s| !s.is_empty()) {
        return locale.replace('_', "-");
    }
    std::env::var("LANG")
        .ok()
        .and_then(|lang| lang.split('.').next().map(|s| s.replace('_', "-")))
        .filter(|s| !s.is_empty() && s != "C")
        .unwrap_or_else(|| "ja-JP".to_string())
}

#[derive(Clone)]
pub enum EngineChoice {
    Whisper { model_path: PathBuf },
    Apple,
}

impl EngineChoice {
    fn raw(&self) -> &'static str {
        match self {
            Self::Whisper { .. } => "whisper",
            Self::Apple => "apple",
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn save_merged_transcript(
    folder: &Path,
    session_id: &str,
    sys_segments: &[TimedSegment],
    mic_segments: &[TimedSegment],
    label_lang: LabelLanguage,
    engine: &str,
    model_id: Option<String>,
    locale: Option<String>,
) -> anyhow::Result<()> {
    save_merged_transcript_with_speakers(
        folder,
        session_id,
        sys_segments,
        mic_segments,
        label_lang,
        engine,
        model_id,
        locale,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn save_merged_transcript_with_speakers(
    folder: &Path,
    session_id: &str,
    sys_segments: &[TimedSegment],
    mic_segments: &[TimedSegment],
    label_lang: LabelLanguage,
    engine: &str,
    model_id: Option<String>,
    locale: Option<String>,
    diarization: Option<&[fennec_core::types::SpeakerTimedSegment]>,
) -> anyhow::Result<()> {
    let mic_deduped = deduplicate_mic_echo(sys_segments, mic_segments);
    let entries = match diarization.filter(|d| !d.is_empty()) {
        Some(diarization) => {
            let mut all: Vec<TimedSegment> = sys_segments.to_vec();
            all.extend(mic_deduped.iter().cloned());
            let assigned = fennec_core::merge::assign_speakers(&all, diarization);
            fennec_core::merge::merge_segments_with_speakers(&assigned)
        }
        None => merge_segments(sys_segments, &mic_deduped),
    };

    let transcript_file = format!("transcript_{}.txt", session_id);
    let text = build_transcript_text(&entries, label_lang);
    std::fs::write(folder.join(&transcript_file), &text)?;
    TranscriptDocument::new(entries)
        .save(&folder.join(format!("transcript_{}.json", session_id)))?;

    let mut metadata =
        load_metadata(folder).ok_or_else(|| anyhow::anyhow!("session.json not found"))?;
    metadata.transcript_file = Some(transcript_file);
    metadata.engine_type = Some(engine.to_string());
    metadata.model_id = model_id;
    metadata.locale_identifier = locale;
    save_metadata(folder, &metadata)?;
    Ok(())
}

pub fn label_language(app_language: &str) -> LabelLanguage {
    match app_language {
        "en" => LabelLanguage::En,
        "ja" => LabelLanguage::Ja,
        _ => {
            let sys = std::env::var("LANG").unwrap_or_default();
            if sys.starts_with("en") {
                LabelLanguage::En
            } else {
                LabelLanguage::Ja
            }
        }
    }
}

pub fn start(app: AppHandle, session_id: String, folder: PathBuf) -> Result<(), String> {
    start_with_engine(app, session_id, folder, None)
}

pub fn start_with_engine(
    app: AppHandle,
    session_id: String,
    folder: PathBuf,
    engine_override: Option<String>,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    {
        let job = state.transcribe_job.lock().unwrap();
        if job.is_some() {
            return Err("transcription already running".into());
        }
    }

    let (engine, language, label_lang) = {
        let settings = state.settings.read().unwrap();
        let engine_name =
            engine_override.unwrap_or_else(|| settings.auto_transcribe_engine.clone());
        let engine = match engine_name.as_str() {
            "apple" => {
                if !cfg!(target_os = "macos") {
                    return Err("Apple音声認識はmacOSでのみ利用できます".into());
                }
                EngineChoice::Apple
            }
            _ => {
                let model_path = settings
                    .whisper_downloaded_models
                    .get(&settings.whisper_model_id)
                    .map(PathBuf::from)
                    .filter(|p| p.exists())
                    .ok_or_else(|| "Whisperモデルがダウンロードされていません".to_string())?;
                EngineChoice::Whisper { model_path }
            }
        };
        (
            engine,
            settings.transcription_locale.clone(),
            label_language(&settings.app_language),
        )
    };

    let cancelled = Arc::new(AtomicBool::new(false));
    let status = JobStatus::new();
    *state.transcribe_job.lock().unwrap() = Some(TranscribeJob {
        session_id: session_id.clone(),
        cancelled: cancelled.clone(),
        status: status.clone(),
    });

    tauri::async_runtime::spawn_blocking(move || {
        let result = run_job(
            &app,
            &session_id,
            &folder,
            &engine,
            language.as_deref(),
            label_lang,
            cancelled,
            status.clone(),
        );
        *status.error.lock().unwrap() = result.as_ref().err().map(|e| format!("{e:#}"));
        status.done.store(true, Ordering::Relaxed);
        let state = app.state::<AppState>();
        *state.transcribe_job.lock().unwrap() = None;

        crate::recording::encode::spawn_encode(app.clone(), folder.clone());

        let error = result.err().map(|e| format!("{e:#}"));
        if error.is_none() {
            crate::services::hooks::fire(
                state.hook_config(&app),
                fennec_core::hooks::HookEvent::TranscriptionCompleted,
                Some(&folder),
                Default::default(),
            );
        }
        events::emit(
            &app,
            events::TRANSCRIBE_DONE,
            TranscribeDone {
                session_id: session_id.clone(),
                error: error.clone(),
            },
        );
        events::emit(&app, events::SESSIONS_CHANGED, ());
        if let Some(error) = error {
            log::error!("transcription failed for {session_id}: {error}");
        }
    });

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_job(
    app: &AppHandle,
    session_id: &str,
    folder: &Path,
    engine: &EngineChoice,
    locale: Option<&str>,
    label_lang: LabelLanguage,
    cancelled: Arc<AtomicBool>,
    status: Arc<JobStatus>,
) -> anyhow::Result<()> {
    let metadata =
        load_metadata(folder).ok_or_else(|| anyhow::anyhow!("session.json not found"))?;

    let sys_path = metadata.system_audio_file.as_ref().map(|f| folder.join(f));
    let mic_path = metadata.mic_audio_file.as_ref().map(|f| folder.join(f));

    let mut whisper = WhisperBatchTranscriber::new();
    let whisper_opts = match engine {
        EngineChoice::Whisper { model_path } => Some(TranscribeOptions {
            language: whisper_language(locale),
            model_path: model_path.clone(),
        }),
        EngineChoice::Apple => None,
    };

    let mut transcribe_lane = |path: &Option<PathBuf>,
                               phase: &str,
                               weight_offset: f64|
     -> anyhow::Result<Vec<TimedSegment>> {
        let Some(path) = path else {
            return Ok(Vec::new());
        };
        let ext = path.extension().and_then(|e| e.to_str());
        if !path.exists() || !matches!(ext, Some("wav") | Some("ogg") | Some("opus") | Some("m4a")) {
            return Ok(Vec::new());
        }
        let progress_app = app.clone();
        let progress_session = session_id.to_string();
        let phase_name = phase.to_string();
        let progress_status = status.clone();
        let on_progress: Arc<dyn Fn(f64) + Send + Sync> = Arc::new(move |fraction: f64| {
            let overall = weight_offset + fraction * 0.5;
            *progress_status.phase.lock().unwrap() = (phase_name.clone(), Some(overall));
            events::emit(
                &progress_app,
                events::TRANSCRIBE_PROGRESS,
                TranscribeProgress {
                    session_id: progress_session.clone(),
                    phase: phase_name.clone(),
                    fraction: Some(overall),
                },
            );
        });

        match &whisper_opts {
            Some(opts) => {
                let result = whisper.transcribe(path, opts, on_progress, cancelled.clone())?;
                Ok(result.segments)
            }
            None => transcribe_lane_apple(app, path, locale, on_progress),
        }
    };

    let sys_segments = transcribe_lane(&sys_path, "system", 0.0)?;
    if cancelled.load(Ordering::Relaxed) {
        anyhow::bail!("cancelled");
    }
    let mic_segments = transcribe_lane(&mic_path, "mic", 0.5)?;
    if cancelled.load(Ordering::Relaxed) {
        anyhow::bail!("cancelled");
    }

    let (model_id, saved_locale, diarization_enabled) = {
        let state = app.state::<AppState>();
        let settings = state.settings.read().unwrap();
        let model_id = match engine {
            EngineChoice::Whisper { .. } => Some(settings.whisper_model_id.clone()),
            EngineChoice::Apple => None,
        };
        (
            model_id,
            settings.transcription_locale.clone(),
            settings.diarization_enabled,
        )
    };

    let diarization = if diarization_enabled {
        let dir = crate::commands::models::diarization_models_dir(app);
        match fennec_stt::diarization_models_at(&dir) {
            Some(models) => {
                *status.phase.lock().unwrap() = ("diarize".to_string(), None);
                events::emit(
                    app,
                    events::TRANSCRIBE_PROGRESS,
                    TranscribeProgress {
                        session_id: session_id.to_string(),
                        phase: "diarize".to_string(),
                        fraction: None,
                    },
                );
                let audio = sys_path
                    .as_ref()
                    .filter(|p| p.exists())
                    .or(mic_path.as_ref().filter(|p| p.exists()));
                audio.and_then(|path| match fennec_stt::diarize_file(path, &models, None) {
                    Ok(segments) => Some(segments),
                    Err(e) => {
                        log::warn!("diarization failed: {e:#}");
                        None
                    }
                })
            }
            None => {
                log::warn!("diarization enabled but models are not downloaded");
                None
            }
        }
    } else {
        None
    };

    save_merged_transcript_with_speakers(
        folder,
        session_id,
        &sys_segments,
        &mic_segments,
        label_lang,
        engine.raw(),
        model_id,
        saved_locale,
        diarization.as_deref(),
    )?;

    generate_summary_if_missing(app, folder);
    crate::services::translation::translate_transcript(app, folder, session_id);

    Ok(())
}

fn transcribe_lane_apple(
    app: &AppHandle,
    path: &Path,
    locale: Option<&str>,
    on_progress: Arc<dyn Fn(f64) + Send + Sync>,
) -> anyhow::Result<Vec<TimedSegment>> {
    #[cfg(target_os = "macos")]
    {
        let state = app.state::<AppState>();
        let client = state.sidecar_client()?;
        let locale = apple_locale(locale);

        // ヘルパーはAVAudioFileで読むためogg/opusは一時wavに変換して渡す
        let ext = path.extension().and_then(|e| e.to_str());
        let temp_wav = if matches!(ext, Some("ogg") | Some("opus")) {
            let samples = fennec_audio::decode_to_mono_16k(path)?;
            let temp = std::env::temp_dir().join(format!("fennec_apple_{}.wav", uuid::Uuid::new_v4()));
            let mut writer = fennec_audio::WavFileWriter::create(
                &temp,
                fennec_audio::AudioFormat {
                    sample_rate: fennec_audio::WHISPER_SAMPLE_RATE,
                    channels: 1,
                },
            )?;
            writer.write(&samples)?;
            writer.finalize()?;
            Some(temp)
        } else {
            None
        };
        let input = temp_wav.as_deref().unwrap_or(path);

        let progress = on_progress.clone();
        let result = client.batch_transcribe(
            input,
            &locale,
            Box::new(move |fraction| progress(fraction)),
        );
        if let Some(temp) = temp_wav {
            let _ = std::fs::remove_file(temp);
        }
        let (_text, segments) = result?;
        on_progress(1.0);
        Ok(segments)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, path, locale, on_progress);
        anyhow::bail!("Apple音声認識はmacOSでのみ利用できます")
    }
}

pub fn generate_summary_if_missing(app: &AppHandle, folder: &Path) {
    let Some(metadata) = load_metadata(folder) else {
        return;
    };
    if metadata.summary.is_some() {
        return;
    }
    let Some(transcript_file) = &metadata.transcript_file else {
        return;
    };
    let Ok(text) = std::fs::read_to_string(folder.join(transcript_file)) else {
        return;
    };
    if text.trim().is_empty() {
        return;
    }

    #[cfg(target_os = "macos")]
    {
        let state = app.state::<AppState>();
        let lang = {
            let settings = state.settings.read().unwrap();
            match settings.app_language.as_str() {
                "en" => "en",
                _ => "ja",
            }
            .to_string()
        };
        let title = state
            .sidecar_client()
            .ok()
            .and_then(|client| client.generate_title(&text, &lang).ok())
            .flatten()
            .map(|t| fennec_core::summary::sanitize_title(&t))
            .filter(|t| !t.is_empty());
        if let Some(title) = title {
            if let Some(mut metadata) = load_metadata(folder) {
                metadata.summary = Some(title);
                let _ = save_metadata(folder, &metadata);
                events::emit(app, events::SESSIONS_CHANGED, ());
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
    }
}
