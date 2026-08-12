use crate::events::{self, ModelProgress};
use crate::state::AppState;
use fennec_core::models::{whisper_model, WHISPER_MODELS};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperModelStatus {
    pub id: String,
    pub name: String,
    pub size: String,
    pub detail: String,
    pub downloaded: bool,
    pub selected: bool,
}

pub fn models_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .map(|d| d.join("WhisperModels"))
        .unwrap_or_else(|_| PathBuf::from("WhisperModels"))
}

pub fn diarization_models_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .map(|d| d.join("DiarizationModels"))
        .unwrap_or_else(|_| PathBuf::from("DiarizationModels"))
}

#[tauri::command]
pub fn diarization_status(app: AppHandle) -> bool {
    fennec_stt::diarization_models_at(&diarization_models_dir(&app)).is_some()
}

#[tauri::command]
pub fn download_diarization_models(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    {
        let download = state.model_download.lock().unwrap();
        if download.is_some() {
            return Err("download already running".into());
        }
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    *state.model_download.lock().unwrap() = Some(cancelled.clone());

    let dir = diarization_models_dir(&app);
    tauri::async_runtime::spawn_blocking(move || {
        let emit_progress = |fraction: f64, error: Option<String>, done: bool| {
            events::emit(
                &app,
                events::MODEL_PROGRESS,
                ModelProgress {
                    model_id: "diarization".to_string(),
                    fraction,
                    error,
                    done,
                },
            );
        };
        let mut last = -1.0f64;
        let mut on_progress = |fraction: f64| {
            if fraction - last >= 0.01 || fraction >= 1.0 {
                last = fraction;
                emit_progress(fraction, None, false);
            }
        };
        let result = fennec_stt::download_diarization_models(&dir, &mut on_progress, cancelled);
        let state = app.state::<AppState>();
        *state.model_download.lock().unwrap() = None;
        match result {
            Ok(_) => emit_progress(1.0, None, true),
            Err(e) => emit_progress(0.0, Some(format!("{e:#}")), true),
        }
    });
    Ok(())
}

#[tauri::command]
pub fn delete_diarization_models(app: AppHandle) -> Result<(), String> {
    fennec_stt::delete_diarization_models(&diarization_models_dir(&app))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_whisper_models(state: State<'_, AppState>) -> Vec<WhisperModelStatus> {
    let settings = state.settings.read().unwrap();
    WHISPER_MODELS
        .iter()
        .map(|m| WhisperModelStatus {
            id: m.id.to_string(),
            name: m.name.to_string(),
            size: m.size.to_string(),
            detail: m.detail.to_string(),
            downloaded: settings
                .whisper_downloaded_models
                .get(m.id)
                .map(|p| PathBuf::from(p).exists())
                .unwrap_or(false),
            selected: settings.whisper_model_id == m.id,
        })
        .collect()
}

#[tauri::command]
pub fn download_whisper_model(
    app: AppHandle,
    state: State<'_, AppState>,
    model_id: String,
) -> Result<(), String> {
    let model = whisper_model(&model_id).ok_or_else(|| format!("unknown model: {model_id}"))?;
    {
        let download = state.model_download.lock().unwrap();
        if download.is_some() {
            return Err("download already running".into());
        }
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    *state.model_download.lock().unwrap() = Some(cancelled.clone());

    let dest_dir = models_dir(&app);
    tauri::async_runtime::spawn_blocking(move || {
        let emit_progress = |fraction: f64, error: Option<String>, done: bool| {
            events::emit(
                &app,
                events::MODEL_PROGRESS,
                ModelProgress {
                    model_id: model.id.to_string(),
                    fraction,
                    error,
                    done,
                },
            );
        };

        let mut last_emitted = -1.0f64;
        let mut on_progress = |fraction: f64| {
            if fraction - last_emitted >= 0.01 || fraction >= 1.0 {
                last_emitted = fraction;
                emit_progress(fraction, None, false);
            }
        };

        let result = fennec_stt::download_model(model, &dest_dir, &mut on_progress, cancelled);
        let state = app.state::<AppState>();
        *state.model_download.lock().unwrap() = None;

        match result {
            Ok(path) => {
                {
                    let mut settings = state.settings.write().unwrap();
                    settings
                        .whisper_downloaded_models
                        .insert(model.id.to_string(), path.to_string_lossy().into_owned());
                }
                state.persist_settings();
                emit_progress(1.0, None, true);
            }
            Err(e) => {
                emit_progress(0.0, Some(format!("{e:#}")), true);
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub fn cancel_model_download(state: State<'_, AppState>) {
    if let Some(cancelled) = state.model_download.lock().unwrap().as_ref() {
        cancelled.store(true, Ordering::Relaxed);
    }
}

#[tauri::command]
pub fn delete_whisper_model(state: State<'_, AppState>, model_id: String) -> Result<(), String> {
    let path = {
        let mut settings = state.settings.write().unwrap();
        settings.whisper_downloaded_models.remove(&model_id)
    };
    state.persist_settings();
    if let Some(path) = path {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}
