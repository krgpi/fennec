use crate::events::{self, RecordingStatePayload, TranscribeDone};
use crate::recording::controller::{
    LiveConfig, RecordingController, SilenceAutoStop,
};
use crate::state::AppState;
use crate::transcription::job::{
    apple_locale, label_language, save_merged_transcript, whisper_language,
};
use fennec_audio::InputDevice;
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputDeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

impl From<InputDevice> for InputDeviceInfo {
    fn from(d: InputDevice) -> Self {
        Self {
            id: d.id,
            name: d.name,
            is_default: d.is_default,
        }
    }
}

#[tauri::command]
pub fn list_input_devices() -> Vec<InputDeviceInfo> {
    fennec_audio::list_input_devices()
        .into_iter()
        .map(Into::into)
        .collect()
}

#[tauri::command]
pub fn start_recording(app: AppHandle) -> Result<String, String> {
    perform_start(&app)
}

pub fn perform_start(app: &AppHandle) -> Result<String, String> {
    let state = app.state::<AppState>();
    let mut recorder = state.recorder.lock().unwrap();
    if recorder.is_some() {
        return Err("already recording".into());
    }
    let base_dir = state.save_directory(app);
    let (mic_device, live_pref, silence_auto_stop) = {
        let settings = state.settings.read().unwrap();
        let mic_device = if settings.use_system_default_mic {
            None
        } else {
            settings.mic_device_id.clone()
        };
        let live_pref = settings.live_transcription_enabled.then(|| {
            (
                settings.live_transcription_engine.clone(),
                settings
                    .whisper_downloaded_models
                    .get(&settings.whisper_model_id)
                    .map(PathBuf::from)
                    .filter(|p| p.exists()),
                whisper_language(settings.transcription_locale.as_deref()),
                apple_locale(settings.transcription_locale.as_deref()),
            )
        });
        let silence_auto_stop = settings.silence_auto_stop_enabled.then(|| SilenceAutoStop {
            minutes: settings.silence_auto_stop_minutes.max(1),
        });
        (mic_device, live_pref, silence_auto_stop)
    };

    let live = live_pref.and_then(|(engine, model, language, locale)| {
        #[cfg(target_os = "macos")]
        {
            if engine == "apple" {
                match state.sidecar_client() {
                    Ok(client) => return Some(LiveConfig::Apple { client, locale }),
                    Err(e) => {
                        log::warn!("apple live unavailable, falling back to whisper: {e:#}")
                    }
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (&engine, &locale);
        model.map(|model_path| LiveConfig::Whisper {
            model_path,
            language,
        })
    });

    let controller = RecordingController::start(
        app,
        &base_dir,
        mic_device.as_deref(),
        live,
        silence_auto_stop,
    )
    .map_err(|e| format!("{e:#}"))?;
    let session_id = controller.session_id.clone();
    let folder = controller.folder.clone();
    *recorder = Some(controller);
    drop(recorder);
    crate::tray::refresh(app);
    events::emit(app, events::SESSIONS_CHANGED, ());
    crate::services::hooks::fire(
        state.hook_config(app),
        fennec_core::hooks::HookEvent::RecordingStarted,
        Some(&folder),
        Default::default(),
    );
    Ok(session_id)
}

pub fn perform_stop(app: &AppHandle) -> Result<String, String> {
    let state = app.state::<AppState>();
    let controller = state
        .recorder
        .lock()
        .unwrap()
        .take()
        .ok_or_else(|| "not recording".to_string())?;

    let session_id = controller.session_id.clone();
    let duration = controller.duration();

    events::emit(
        app,
        events::RECORDING_STATE,
        RecordingStatePayload {
            recording: false,
            duration,
            session_id: Some(session_id.clone()),
        },
    );
    crate::tray::refresh(app);

    let (auto_transcribe, label_lang, model_id, locale) = {
        let settings = state.settings.read().unwrap();
        (
            settings.auto_transcribe_enabled,
            label_language(&settings.app_language),
            settings.whisper_model_id.clone(),
            settings.transcription_locale.clone(),
        )
    };
    let hook_config = state.hook_config(app);

    let bg_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        finish_stop(bg_app, controller, hook_config, auto_transcribe, label_lang, model_id, locale);
    });

    Ok(session_id)
}

fn finish_stop(
    app: AppHandle,
    controller: RecordingController,
    hook_config: crate::services::hooks::HookFire,
    auto_transcribe: bool,
    label_lang: fennec_core::format::LabelLanguage,
    model_id: String,
    locale: Option<String>,
) {
    let outcome = match controller.stop(&app) {
        Ok(o) => o,
        Err(e) => {
            log::error!("failed to stop recording: {e:#}");
            events::emit(&app, events::SESSIONS_CHANGED, ());
            return;
        }
    };
    events::emit(&app, events::SESSIONS_CHANGED, ());
    crate::services::hooks::fire(
        hook_config.clone(),
        fennec_core::hooks::HookEvent::RecordingStopped,
        Some(&outcome.folder),
        Default::default(),
    );

    let live_chars: usize = outcome
        .live_sys_segments
        .iter()
        .chain(outcome.live_mic_segments.iter())
        .map(|s| s.text.chars().count())
        .sum();

    let mut transcription_started = false;
    if outcome.live_engine.is_some() && live_chars >= 10 {
        let engine = outcome.live_engine.unwrap_or("whisper");
        match save_merged_transcript(
            &outcome.folder,
            &outcome.session_id,
            &outcome.live_sys_segments,
            &outcome.live_mic_segments,
            label_lang,
            engine,
            (engine == "whisper").then_some(model_id),
            locale,
        ) {
            Ok(()) => {
                crate::services::hooks::fire(
                    hook_config,
                    fennec_core::hooks::HookEvent::TranscriptionCompleted,
                    Some(&outcome.folder),
                    Default::default(),
                );
                let bg_folder = outcome.folder.clone();
                let bg_session = outcome.session_id.clone();
                crate::transcription::job::generate_summary_if_missing(&app, &bg_folder);
                emit_transcribe_done(&app, &bg_session);
                crate::services::translation::translate_transcript(
                    &app, &bg_folder, &bg_session,
                );
                emit_transcribe_done(&app, &bg_session);
                events::emit(&app, events::SESSIONS_CHANGED, ());
            }
            Err(e) => log::error!("failed to save live transcript: {e:#}"),
        }
    } else {
        let should_batch = if outcome.live_engine.is_some() {
            true
        } else {
            auto_transcribe
        };
        if should_batch {
            match crate::transcription::job::start(
                app.clone(),
                outcome.session_id.clone(),
                outcome.folder.clone(),
            ) {
                Ok(()) => transcription_started = true,
                Err(e) => log::warn!("auto transcription not started: {e}"),
            }
        }
    }
    if !transcription_started {
        crate::recording::encode::spawn_encode(app.clone(), outcome.folder.clone());
    }
}

fn emit_transcribe_done(app: &AppHandle, session_id: &str) {
    events::emit(
        app,
        events::TRANSCRIBE_DONE,
        TranscribeDone {
            session_id: session_id.to_string(),
            error: None,
        },
    );
}

#[tauri::command]
pub fn stop_recording(app: AppHandle) -> Result<String, String> {
    perform_stop(&app)
}

#[tauri::command]
pub fn get_recording_state(state: State<'_, AppState>) -> RecordingStatePayload {
    let recorder = state.recorder.lock().unwrap();
    match recorder.as_ref() {
        Some(c) => RecordingStatePayload {
            recording: true,
            duration: c.duration(),
            session_id: Some(c.session_id.clone()),
        },
        None => RecordingStatePayload {
            recording: false,
            duration: 0.0,
            session_id: None,
        },
    }
}
