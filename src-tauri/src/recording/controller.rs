use crate::events::{self, LiveTranscriptPayload, RecordingLevels, RecordingStatePayload};
use anyhow::Context;
use chrono::{Local, Utc};
use fennec_audio::{
    new_mic_capture, new_system_capture, rms, rms_to_level, CaptureSource,
    StreamDownmixResampler, WavFileWriter, DEFAULT_MIC_GAIN,
};
use fennec_core::merge::join_segment_texts;
use fennec_core::session::{save_metadata, session_id_for, SessionMetadata};
use fennec_core::types::TimedSegment;
use fennec_stt::{LiveEngine, LiveEvent, WhisperLiveTranscriber};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;
use tauri::AppHandle;

pub enum LiveConfig {
    Whisper {
        model_path: PathBuf,
        language: String,
    },
    #[cfg(target_os = "macos")]
    Apple {
        client: Arc<fennec_stt::SidecarClient>,
        locale: String,
    },
}

enum ResolvedLive {
    Whisper(Arc<LiveEngine>),
    #[cfg(target_os = "macos")]
    Apple {
        client: Arc<fennec_stt::SidecarClient>,
        locale: String,
    },
}

impl ResolvedLive {
    fn engine_name(&self) -> &'static str {
        match self {
            Self::Whisper(_) => "whisper",
            #[cfg(target_os = "macos")]
            Self::Apple { .. } => "apple",
        }
    }
}

pub struct SilenceAutoStop {
    pub minutes: u32,
}

struct PeakLevel(AtomicU32);

impl PeakLevel {
    fn new() -> Self {
        Self(AtomicU32::new(0.0f32.to_bits()))
    }

    fn update_max(&self, value: f32) {
        let mut current = self.0.load(Ordering::Relaxed);
        loop {
            if value <= f32::from_bits(current) {
                return;
            }
            match self.0.compare_exchange_weak(
                current,
                value.to_bits(),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(actual) => current = actual,
            }
        }
    }

    fn take(&self) -> f32 {
        f32::from_bits(self.0.swap(0.0f32.to_bits(), Ordering::Relaxed))
    }
}

enum LiveBackend {
    Whisper(WhisperLiveTranscriber),
    #[cfg(target_os = "macos")]
    Apple {
        client: Arc<fennec_stt::SidecarClient>,
        stream_id: String,
        feeder_tx: mpsc::Sender<Vec<f32>>,
    },
}

struct LiveLane {
    backend: LiveBackend,
    resampler: Mutex<StreamDownmixResampler>,
}

impl LiveLane {
    fn feed(&self, samples: Vec<f32>) {
        match &self.backend {
            LiveBackend::Whisper(transcriber) => transcriber.feed(&samples),
            #[cfg(target_os = "macos")]
            LiveBackend::Apple { feeder_tx, .. } => {
                let _ = feeder_tx.send(samples);
            }
        }
    }

    fn finish(self) -> Vec<TimedSegment> {
        match self.backend {
            LiveBackend::Whisper(transcriber) => transcriber.finish(),
            #[cfg(target_os = "macos")]
            LiveBackend::Apple {
                client,
                stream_id,
                feeder_tx,
            } => {
                drop(feeder_tx);
                client.live_stop(&stream_id).unwrap_or_else(|e| {
                    log::warn!("apple live stop failed: {e:#}");
                    Vec::new()
                })
            }
        }
    }
}

struct LiveTexts {
    finals: Vec<String>,
    volatile: String,
}

fn make_live_emitter(
    app: &AppHandle,
    source_name: &'static str,
) -> Arc<dyn Fn(LiveEvent) + Send + Sync> {
    let texts = Arc::new(Mutex::new(LiveTexts {
        finals: Vec::new(),
        volatile: String::new(),
    }));
    let event_app = app.clone();
    let translator = spawn_live_translator(app, source_name);
    Arc::new(move |event| {
        let (finalized, volatile) = {
            let mut texts = texts.lock().unwrap();
            match event {
                LiveEvent::Final(segment) => {
                    texts.finals.push(segment.text);
                    texts.volatile.clear();
                }
                LiveEvent::Partial(text) => {
                    texts.volatile = text;
                }
            }
            (join_segment_texts(&texts.finals), texts.volatile.clone())
        };
        if let Some(translator) = &translator {
            let mut latest = translator.lock().unwrap();
            latest.0 = finalized.clone();
            latest.1 = Instant::now();
        }
        events::emit(
            &event_app,
            events::LIVE_TRANSCRIPT,
            LiveTranscriptPayload {
                source: source_name.to_string(),
                finalized,
                volatile,
            },
        );
    })
}

type LiveTranslateState = Arc<Mutex<(String, Instant)>>;

#[cfg(target_os = "macos")]
fn spawn_live_translator(app: &AppHandle, source_name: &'static str) -> Option<LiveTranslateState> {
    use tauri::Manager;

    let state = app.state::<crate::state::AppState>();
    let (enabled, target) = {
        let settings = state.settings.read().unwrap();
        let target = match settings.app_language.as_str() {
            "en" => "en",
            _ => "ja",
        };
        (settings.translation_enabled, target.to_string())
    };
    if !enabled {
        return None;
    }

    let latest: LiveTranslateState = Arc::new(Mutex::new((String::new(), Instant::now())));
    let task_latest = latest.clone();
    let task_app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut translated_for = String::new();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let (text, updated_at) = {
                let latest = task_latest.lock().unwrap();
                latest.clone()
            };
            if Arc::strong_count(&task_latest) == 1 {
                break;
            }
            if text.is_empty()
                || text == translated_for
                || updated_at.elapsed() < std::time::Duration::from_millis(1500)
            {
                continue;
            }
            translated_for = text.clone();

            let translate_app = task_app.clone();
            let target = target.clone();
            let result = tauri::async_runtime::spawn_blocking(move || {
                let state = translate_app.state::<crate::state::AppState>();
                let client = state.sidecar_client().ok()?;
                client
                    .translate(&[text], &target)
                    .ok()
                    .and_then(|mut t| t.pop().flatten())
            })
            .await
            .ok()
            .flatten();

            if let Some(translation) = result {
                events::emit(
                    &task_app,
                    events::LIVE_TRANSLATION,
                    events::LiveTranslationPayload {
                        source: source_name.to_string(),
                        text: translation,
                    },
                );
            }
        }
    });
    Some(latest)
}

#[cfg(not(target_os = "macos"))]
fn spawn_live_translator(
    _app: &AppHandle,
    _source_name: &'static str,
) -> Option<LiveTranslateState> {
    None
}

fn make_live_lane(
    app: &AppHandle,
    live: &ResolvedLive,
    source_name: &'static str,
    format: fennec_audio::AudioFormat,
) -> anyhow::Result<Arc<LiveLane>> {
    let emitter = make_live_emitter(app, source_name);
    let backend = match live {
        ResolvedLive::Whisper(engine) => {
            LiveBackend::Whisper(WhisperLiveTranscriber::start(engine.clone(), emitter))
        }
        #[cfg(target_os = "macos")]
        ResolvedLive::Apple { client, locale } => {
            let stream_id = source_name.to_string();
            client.live_start(
                &stream_id,
                locale,
                fennec_audio::WHISPER_SAMPLE_RATE,
                Box::new(move |event| match event {
                    fennec_stt::AppleLiveEvent::Partial { text, .. } => {
                        emitter(LiveEvent::Partial(text))
                    }
                    fennec_stt::AppleLiveEvent::Final { segment, .. } => {
                        emitter(LiveEvent::Final(segment))
                    }
                }),
            )?;
            let (feeder_tx, feeder_rx) = mpsc::channel::<Vec<f32>>();
            let feeder_client = client.clone();
            let feeder_stream = stream_id.clone();
            std::thread::Builder::new()
                .name("fennec-apple-live-feed".into())
                .spawn(move || {
                    while let Ok(chunk) = feeder_rx.recv() {
                        if feeder_client.live_audio(&feeder_stream, &chunk).is_err() {
                            break;
                        }
                    }
                })?;
            LiveBackend::Apple {
                client: client.clone(),
                stream_id,
                feeder_tx,
            }
        }
    };
    Ok(Arc::new(LiveLane {
        backend,
        resampler: Mutex::new(StreamDownmixResampler::new(
            format.sample_rate,
            format.channels,
            fennec_audio::WHISPER_SAMPLE_RATE,
        )),
    }))
}

struct CaptureLane {
    source: Box<dyn CaptureSource>,
    writer_tx: mpsc::Sender<Vec<f32>>,
    writer_handle: JoinHandle<anyhow::Result<()>>,
    live: Option<Arc<LiveLane>>,
}

impl CaptureLane {
    fn start(
        mut source: Box<dyn CaptureSource>,
        path: PathBuf,
        peak: Arc<PeakLevel>,
        live: Option<Arc<LiveLane>>,
    ) -> anyhow::Result<Self> {
        let format = source.format();
        let mut writer = WavFileWriter::create(&path, format)
            .with_context(|| format!("failed to create {}", path.display()))?;
        let (writer_tx, writer_rx) = mpsc::channel::<Vec<f32>>();
        let writer_handle = std::thread::Builder::new()
            .name("fennec-wav-writer".into())
            .spawn(move || -> anyhow::Result<()> {
                while let Ok(chunk) = writer_rx.recv() {
                    writer.write(&chunk)?;
                }
                writer.finalize()?;
                Ok(())
            })?;

        let tx = writer_tx.clone();
        let chunk_live = live.clone();
        source.start(Arc::new(move |chunk: &[f32]| {
            peak.update_max(rms(chunk));
            let _ = tx.send(chunk.to_vec());
            if let Some(live) = &chunk_live {
                let resampled = live.resampler.lock().unwrap().push(chunk);
                if !resampled.is_empty() {
                    live.feed(resampled);
                }
            }
        }))?;

        Ok(Self {
            source,
            writer_tx,
            writer_handle,
            live,
        })
    }

    fn stop(mut self) -> anyhow::Result<Vec<TimedSegment>> {
        self.source.stop();
        drop(self.writer_tx);
        self.writer_handle
            .join()
            .map_err(|_| anyhow::anyhow!("wav writer thread panicked"))??;
        let segments = match self.live {
            Some(live) => match Arc::try_unwrap(live) {
                Ok(lane) => lane.finish(),
                Err(_) => Vec::new(),
            },
            None => Vec::new(),
        };
        Ok(segments)
    }
}

pub struct RecordingController {
    pub session_id: String,
    pub folder: PathBuf,
    pub live_engine: Option<&'static str>,
    started_at: Instant,
    sys_lane: Option<CaptureLane>,
    mic_lane: Option<CaptureLane>,
    metadata: SessionMetadata,
    tick_stop: Arc<AtomicBool>,
}

pub struct RecordingOutcome {
    pub session_id: String,
    pub folder: PathBuf,
    pub system_audio: Option<PathBuf>,
    pub mic_audio: Option<PathBuf>,
    pub duration: f64,
    pub live_engine: Option<&'static str>,
    pub live_sys_segments: Vec<TimedSegment>,
    pub live_mic_segments: Vec<TimedSegment>,
}

impl RecordingController {
    pub fn start(
        app: &AppHandle,
        base_dir: &PathBuf,
        mic_device_id: Option<&str>,
        live: Option<LiveConfig>,
        silence_auto_stop: Option<SilenceAutoStop>,
    ) -> anyhow::Result<Self> {
        let session_id = session_id_for(Local::now());
        let folder = base_dir.join(&session_id);
        std::fs::create_dir_all(&folder)
            .with_context(|| format!("failed to create session folder {}", folder.display()))?;

        let sys_file = format!("system_{}.wav", session_id);
        let mic_file = format!("mic_{}.wav", session_id);

        let sys_peak = Arc::new(PeakLevel::new());
        let mic_peak = Arc::new(PeakLevel::new());

        let resolved_live: Option<ResolvedLive> = match live {
            Some(LiveConfig::Whisper {
                model_path,
                language,
            }) => match LiveEngine::new(&model_path, &language) {
                Ok(engine) => Some(ResolvedLive::Whisper(engine)),
                Err(e) => {
                    log::warn!("live transcription unavailable: {e:#}");
                    None
                }
            },
            #[cfg(target_os = "macos")]
            Some(LiveConfig::Apple { client, locale }) => {
                Some(ResolvedLive::Apple { client, locale })
            }
            None => None,
        };

        let build_live_lane = |source: &Box<dyn CaptureSource>, name: &'static str| {
            resolved_live.as_ref().and_then(|live| {
                match make_live_lane(app, live, name, source.format()) {
                    Ok(lane) => Some(lane),
                    Err(e) => {
                        log::warn!("live lane unavailable for {name}: {e:#}");
                        None
                    }
                }
            })
        };

        let sys_lane = match new_system_capture(true) {
            Ok(source) => {
                let live_lane = build_live_lane(&source, "system");
                Some(CaptureLane::start(
                    source,
                    folder.join(&sys_file),
                    sys_peak.clone(),
                    live_lane,
                )?)
            }
            Err(e) => {
                log::warn!("system audio capture unavailable: {e:#}");
                None
            }
        };

        let mic_lane = match new_mic_capture(mic_device_id, DEFAULT_MIC_GAIN) {
            Ok(source) => {
                let live_lane = build_live_lane(&source, "mic");
                Some(CaptureLane::start(
                    source,
                    folder.join(&mic_file),
                    mic_peak.clone(),
                    live_lane,
                )?)
            }
            Err(e) => {
                log::warn!("mic capture unavailable: {e:#}");
                None
            }
        };

        if sys_lane.is_none() && mic_lane.is_none() {
            let _ = std::fs::remove_dir_all(&folder);
            anyhow::bail!("no capture source available");
        }

        let mut metadata = SessionMetadata::new(Utc::now());
        metadata.system_audio_file = sys_lane.is_some().then(|| sys_file.clone());
        metadata.mic_audio_file = mic_lane.is_some().then(|| mic_file.clone());
        save_metadata(&folder, &metadata)?;

        let tick_stop = Arc::new(AtomicBool::new(false));
        spawn_tick(
            app.clone(),
            session_id.clone(),
            tick_stop.clone(),
            sys_peak,
            mic_peak,
            silence_auto_stop,
        );

        Ok(Self {
            session_id,
            folder,
            live_engine: resolved_live.as_ref().map(|l| l.engine_name()),
            started_at: Instant::now(),
            sys_lane,
            mic_lane,
            metadata,
            tick_stop,
        })
    }

    pub fn duration(&self) -> f64 {
        self.started_at.elapsed().as_secs_f64()
    }

    pub fn stop(mut self, app: &AppHandle) -> anyhow::Result<RecordingOutcome> {
        self.tick_stop.store(true, Ordering::Relaxed);
        let duration = self.duration();

        let mut live_sys_segments = Vec::new();
        let mut live_mic_segments = Vec::new();

        if let Some(lane) = self.sys_lane.take() {
            match lane.stop() {
                Ok(segments) => live_sys_segments = segments,
                Err(e) => log::error!("failed to finalize system wav: {e:#}"),
            }
        }
        if let Some(lane) = self.mic_lane.take() {
            match lane.stop() {
                Ok(segments) => live_mic_segments = segments,
                Err(e) => log::error!("failed to finalize mic wav: {e:#}"),
            }
        }

        save_metadata(&self.folder, &self.metadata)?;
        events::emit(
            app,
            events::RECORDING_STATE,
            RecordingStatePayload {
                recording: false,
                duration,
                session_id: Some(self.session_id.clone()),
            },
        );

        Ok(RecordingOutcome {
            system_audio: self
                .metadata
                .system_audio_file
                .as_ref()
                .map(|f| self.folder.join(f)),
            mic_audio: self
                .metadata
                .mic_audio_file
                .as_ref()
                .map(|f| self.folder.join(f)),
            session_id: self.session_id,
            folder: self.folder,
            duration,
            live_engine: self.live_engine,
            live_sys_segments,
            live_mic_segments,
        })
    }
}

fn spawn_tick(
    app: AppHandle,
    session_id: String,
    stop: Arc<AtomicBool>,
    sys_peak: Arc<PeakLevel>,
    mic_peak: Arc<PeakLevel>,
    silence_auto_stop: Option<SilenceAutoStop>,
) {
    const SILENCE_LEVEL_THRESHOLD: f32 = 0.02;
    const SILENCE_RMS_THRESHOLD: f32 = 0.002;
    const NO_INPUT_GRACE_TICKS: u32 = 40;

    let started = Instant::now();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
        let mut ticks = 0u32;
        let mut max_sys = 0.0f32;
        let mut max_mic = 0.0f32;
        let mut warned = false;
        let mut silent_ticks = 0u64;
        let mut shown_seconds = 0u64;
        loop {
            interval.tick().await;
            if stop.load(Ordering::Relaxed) {
                break;
            }
            ticks += 1;
            let sys_rms = sys_peak.take();
            let mic_rms = mic_peak.take();
            max_sys = max_sys.max(sys_rms);
            max_mic = max_mic.max(mic_rms);
            if !warned && ticks == NO_INPUT_GRACE_TICKS {
                let sys_silent = max_sys < SILENCE_RMS_THRESHOLD;
                let mic_silent = max_mic < SILENCE_RMS_THRESHOLD;
                if sys_silent || mic_silent {
                    warned = true;
                    events::emit(
                        &app,
                        events::RECORDING_NO_INPUT,
                        events::NoInputWarning {
                            sys: sys_silent,
                            mic: mic_silent,
                        },
                    );
                }
            }

            let sys_level = rms_to_level(sys_rms);
            let mic_level = rms_to_level(mic_rms);

            if let Some(auto_stop) = &silence_auto_stop {
                if sys_level < SILENCE_LEVEL_THRESHOLD && mic_level < SILENCE_LEVEL_THRESHOLD {
                    silent_ticks += 1;
                } else {
                    silent_ticks = 0;
                }
                if silent_ticks >= auto_stop.minutes as u64 * 60 * 4 {
                    events::emit(
                        &app,
                        events::RECORDING_AUTO_STOPPED,
                        serde_json::json!({ "reason": "silence", "minutes": auto_stop.minutes }),
                    );
                    {
                        use tauri_plugin_notification::NotificationExt;
                        let _ = app
                            .notification()
                            .builder()
                            .title("録音を自動停止しました")
                            .body(format!(
                                "{}分間音声が検出されなかったため、録音を停止しました。",
                                auto_stop.minutes
                            ))
                            .show();
                    }
                    let stop_app = app.clone();
                    tauri::async_runtime::spawn_blocking(move || {
                        if let Err(e) = crate::commands::recording::perform_stop(&stop_app) {
                            log::error!("silence auto-stop failed: {e}");
                        }
                    });
                    break;
                }
            }

            events::emit(
                &app,
                events::RECORDING_LEVELS,
                RecordingLevels {
                    sys: sys_level,
                    mic: mic_level,
                },
            );
            let elapsed = started.elapsed().as_secs_f64();
            if elapsed as u64 != shown_seconds {
                shown_seconds = elapsed as u64;
                crate::tray::set_recording(&app, true, elapsed);
            }
            events::emit(
                &app,
                events::RECORDING_STATE,
                RecordingStatePayload {
                    recording: true,
                    duration: elapsed,
                    session_id: Some(session_id.clone()),
                },
            );
        }
    });
}
