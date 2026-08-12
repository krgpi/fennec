use crate::state::AppState;
use crate::transcription::job;
use serde::Serialize;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, State};

#[tauri::command]
pub fn start_transcription(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    engine: Option<String>,
) -> Result<(), String> {
    let base_dir = state.save_directory(&app);
    let folder = base_dir.join(&session_id);
    if !folder.join("session.json").exists() {
        return Err(format!("session not found: {session_id}"));
    }
    job::start_with_engine(app.clone(), session_id, folder, engine)
}

#[tauri::command]
pub fn cancel_transcription(state: State<'_, AppState>) {
    if let Some(job) = state.transcribe_job.lock().unwrap().as_ref() {
        job.cancelled.store(true, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionState {
    pub running: bool,
    pub session_id: Option<String>,
}

#[tauri::command]
pub fn get_transcription_state(state: State<'_, AppState>) -> TranscriptionState {
    let job = state.transcribe_job.lock().unwrap();
    TranscriptionState {
        running: job.is_some(),
        session_id: job.as_ref().map(|j| j.session_id.clone()),
    }
}
