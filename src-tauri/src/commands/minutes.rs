use crate::events;
use crate::services::hooks;
use crate::services::minutes::{self, MinutesRequest};
use crate::state::{AppState, MinutesJob};
use chrono::Utc;
use fennec_core::hooks::HookEvent;
use fennec_core::minutes::MinutesBackend;
use fennec_core::session::load_metadata;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MinutesDone {
    pub session_id: String,
    pub file: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendInfo {
    pub backend: MinutesBackend,
    pub display_name: String,
    pub models: Vec<ModelChoice>,
    pub default_model: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelChoice {
    pub id: String,
    pub name: String,
}

#[tauri::command]
pub fn list_minutes_backends(refresh: Option<bool>) -> Vec<BackendInfo> {
    if refresh.unwrap_or(false) {
        minutes::refresh_cli_cache();
    }
    minutes::available_backends()
        .into_iter()
        .map(|b| BackendInfo {
            backend: b,
            display_name: b.display_name().to_string(),
            models: b
                .available_models()
                .into_iter()
                .map(|(id, name)| ModelChoice {
                    id: id.to_string(),
                    name: name.to_string(),
                })
                .collect(),
            default_model: b.default_model().to_string(),
        })
        .collect()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateMinutesArgs {
    pub session_id: String,
    pub backend: MinutesBackend,
    pub model: String,
    pub context_folder: Option<PathBuf>,
    pub output_folder: Option<PathBuf>,
    pub custom_prompt: Option<String>,
    pub preset_id: Option<Uuid>,
}

#[tauri::command]
pub fn generate_minutes(
    app: AppHandle,
    state: State<'_, AppState>,
    args: GenerateMinutesArgs,
) -> Result<(), String> {
    {
        let job = state.minutes_job.lock().unwrap();
        if job.is_some() {
            return Err("議事録を生成中です".into());
        }
    }

    let folder = state.save_directory(&app).join(&args.session_id);
    let metadata = load_metadata(&folder).ok_or("session not found")?;
    let transcript_file = metadata
        .transcript_file
        .clone()
        .ok_or("文字起こしがありません")?;
    let transcript =
        std::fs::read_to_string(folder.join(&transcript_file)).map_err(|e| e.to_string())?;

    let child_slot = Arc::new(Mutex::new(None));
    let cancelled = Arc::new(AtomicBool::new(false));
    *state.minutes_job.lock().unwrap() = Some(MinutesJob {
        session_id: args.session_id.clone(),
        child: child_slot.clone(),
        cancelled: cancelled.clone(),
    });

    if let Some(preset_id) = args.preset_id {
        let mut settings = state.settings.write().unwrap();
        if let Some(preset) = settings.minutes_presets.iter_mut().find(|p| p.id == preset_id) {
            preset.last_used_at = Utc::now();
        }
        drop(settings);
        state.persist_settings();
    }

    let request = MinutesRequest {
        session_id: args.session_id.clone(),
        session_folder: folder.clone(),
        transcript,
        backend: args.backend,
        model: args.model,
        context_folder: args.context_folder,
        output_folder: args.output_folder,
        custom_prompt: args.custom_prompt,
    };

    tauri::async_runtime::spawn_blocking(move || {
        let result = minutes::generate(&request, child_slot, cancelled.clone());
        let state = app.state::<AppState>();
        *state.minutes_job.lock().unwrap() = None;

        match result {
            Ok(outcome) => {
                minutes::record_outcome(&folder, &outcome, args.preset_id);

                hooks::fire(
                    state.hook_config(&app),
                    HookEvent::MinutesGenerated,
                    None,
                    HashMap::from([(
                        "FENNEC_MINUTES_FILE".to_string(),
                        outcome.output_file.to_string_lossy().into_owned(),
                    )]),
                );

                events::emit(
                    &app,
                    events::MINUTES_DONE,
                    MinutesDone {
                        session_id: request.session_id.clone(),
                        file: Some(outcome.output_file.to_string_lossy().into_owned()),
                        error: None,
                    },
                );
                events::emit(&app, events::SESSIONS_CHANGED, ());
            }
            Err(e) => {
                let cancelled_now = cancelled.load(Ordering::Relaxed);
                events::emit(
                    &app,
                    events::MINUTES_DONE,
                    MinutesDone {
                        session_id: request.session_id.clone(),
                        file: None,
                        error: (!cancelled_now).then(|| format!("{e:#}")),
                    },
                );
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub fn cancel_minutes(state: State<'_, AppState>) {
    if let Some(job) = state.minutes_job.lock().unwrap().as_ref() {
        job.cancelled.store(true, Ordering::Relaxed);
        if let Some(child) = job.child.lock().unwrap().as_mut() {
            let _ = child.kill();
        }
    }
}
