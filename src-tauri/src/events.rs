use serde::Serialize;
use tauri::{AppHandle, Emitter};

pub const RECORDING_LEVELS: &str = "recording://levels";
pub const RECORDING_STATE: &str = "recording://state";
pub const RECORDING_NO_INPUT: &str = "recording://no-input";
pub const RECORDING_AUTO_STOPPED: &str = "recording://auto-stopped";
pub const LIVE_TRANSCRIPT: &str = "live://transcript";
pub const LIVE_TRANSLATION: &str = "live://translation";
pub const SESSIONS_CHANGED: &str = "sessions://changed";
pub const TRANSCRIBE_PROGRESS: &str = "transcribe://progress";
pub const TRANSCRIBE_DONE: &str = "transcribe://done";
pub const TRANSCRIBE_ERROR: &str = "transcribe://error";
pub const MODEL_PROGRESS: &str = "model://progress";
pub const MINUTES_DONE: &str = "minutes://done";
pub const PLAYER_POSITION: &str = "player://position";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingLevels {
    pub sys: f32,
    pub mic: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStatePayload {
    pub recording: bool,
    pub duration: f64,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoInputWarning {
    pub sys: bool,
    pub mic: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveTranscriptPayload {
    pub source: String,
    pub finalized: String,
    pub volatile: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveTranslationPayload {
    pub source: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeProgress {
    pub session_id: String,
    pub phase: String,
    pub fraction: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeDone {
    pub session_id: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProgress {
    pub model_id: String,
    pub fraction: f64,
    pub error: Option<String>,
    pub done: bool,
}

pub fn emit<P: Serialize + Clone>(app: &AppHandle, event: &str, payload: P) {
    if let Err(e) = app.emit(event, payload) {
        log::warn!("failed to emit {event}: {e}");
    }
}
