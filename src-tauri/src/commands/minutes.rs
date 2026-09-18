use crate::events;
use crate::services::hooks;
use crate::services::minutes::{self, MinutesRequest};
use crate::state::{AppState, MinutesJob};
use chrono::Utc;
use fennec_core::agents::{config_template, ModelChoice};
use fennec_core::hooks::HookEvent;
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
    pub backend: String,
    pub display_name: String,
    pub models: Vec<ModelChoice>,
    pub default_model: String,
    pub binary: String,
    pub binary_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentsConfigInfo {
    pub path: String,
    pub exists: bool,
    pub error: Option<String>,
    pub agents: Vec<BackendInfo>,
}

fn agents_config_info(state: &AppState) -> AgentsConfigInfo {
    let path = &state.agents_config_path;
    let (defs, error) = minutes::load_agent_definitions(path);
    AgentsConfigInfo {
        path: path.to_string_lossy().into_owned(),
        exists: path.is_file(),
        error,
        agents: defs
            .iter()
            .map(|a| BackendInfo {
                backend: a.id.clone(),
                display_name: a.display_name().to_string(),
                models: a.models.clone(),
                default_model: a.default_model.clone(),
                binary: a.binary_name().to_string(),
                binary_path: minutes::find_cli(a).map(|p| p.to_string_lossy().into_owned()),
            })
            .collect(),
    }
}

/// 組み込み + agents.json のうち CLI が見つかったものだけを返す（議事録ダイアログ用）。
#[tauri::command]
pub fn list_minutes_backends(state: State<'_, AppState>, refresh: Option<bool>) -> Vec<BackendInfo> {
    if refresh.unwrap_or(false) {
        minutes::refresh_cli_cache();
    }
    agents_config_info(&state)
        .agents
        .into_iter()
        .filter(|a| a.binary_path.is_some())
        .collect()
}

/// 設定画面用。CLI の有無に関わらず全定義と設定ファイルの状態を返す。
#[tauri::command]
pub fn get_agents_config(state: State<'_, AppState>, refresh: Option<bool>) -> AgentsConfigInfo {
    if refresh.unwrap_or(false) {
        minutes::refresh_cli_cache();
    }
    agents_config_info(&state)
}

#[tauri::command]
pub fn create_agents_config(state: State<'_, AppState>) -> Result<AgentsConfigInfo, String> {
    let path = &state.agents_config_path;
    if !path.exists() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, config_template()).map_err(|e| e.to_string())?;
    }
    Ok(agents_config_info(&state))
}

#[tauri::command]
pub fn open_agents_config(state: State<'_, AppState>) -> Result<(), String> {
    tauri_plugin_opener::open_path(&state.agents_config_path, None::<&str>)
        .map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateMinutesArgs {
    pub session_id: String,
    pub backend: String,
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

    let agent = minutes::find_agent(&state.agents_config_path, &args.backend)
        .ok_or_else(|| format!("unknown backend: {}", args.backend))?;

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
        agent,
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
