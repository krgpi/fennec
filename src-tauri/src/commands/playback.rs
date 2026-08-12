use crate::events;
use crate::state::{AppState, PlayerSession};
use fennec_core::session::load_metadata;
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerPosition {
    pub session_id: String,
    pub position: f64,
    pub duration: f64,
    pub playing: bool,
    pub finished: bool,
    pub has_system: bool,
    pub has_mic: bool,
}

fn emit_position(app: &AppHandle) {
    let state = app.state::<AppState>();
    let player = state.player.lock().unwrap();
    if let Some(session) = player.as_ref() {
        let status = session.player.status();
        events::emit(
            app,
            events::PLAYER_POSITION,
            PlayerPosition {
                session_id: session.session_id.clone(),
                position: status.position,
                duration: status.duration,
                playing: status.playing,
                finished: status.finished,
                has_system: session.has_system,
                has_mic: session.has_mic,
            },
        );
    }
}

#[tauri::command]
pub fn player_load(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<PlayerPosition, String> {
    let folder = state.save_directory(&app).join(&session_id);
    let (folder, metadata) = if folder.join("session.json").exists() {
        let metadata = load_metadata(&folder).ok_or("failed to read session.json")?;
        (folder, metadata)
    } else {
        return Err(format!("session not found: {session_id}"));
    };

    let resolve = |name: &Option<String>| -> Option<std::path::PathBuf> {
        name.as_ref()
            .map(|f| folder.join(f))
            .filter(|p: &std::path::PathBuf| p.exists())
    };
    let sys_path = resolve(&metadata.system_audio_file);
    let mic_path = resolve(&metadata.mic_audio_file);
    if sys_path.is_none() && mic_path.is_none() {
        return Err("再生できる音声ファイルがありません".into());
    }

    {
        let mut player = state.player.lock().unwrap();
        if let Some(old) = player.take() {
            old.alive.store(false, Ordering::Relaxed);
        }
    }

    let paths: Vec<Option<&Path>> = vec![sys_path.as_deref(), mic_path.as_deref()];
    let new_player = fennec_audio::Player::load(&paths).map_err(|e| format!("{e:#}"))?;
    let status = new_player.status();
    let alive = Arc::new(AtomicBool::new(true));

    let result = PlayerPosition {
        session_id: session_id.clone(),
        position: status.position,
        duration: status.duration,
        playing: status.playing,
        finished: status.finished,
        has_system: sys_path.is_some(),
        has_mic: mic_path.is_some(),
    };

    *state.player.lock().unwrap() = Some(PlayerSession {
        session_id,
        player: new_player,
        alive: alive.clone(),
        has_system: sys_path.is_some(),
        has_mic: mic_path.is_some(),
    });

    let tick_app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
        while alive.load(Ordering::Relaxed) {
            interval.tick().await;
            emit_position(&tick_app);
        }
    });

    Ok(result)
}

#[tauri::command]
pub fn player_unload(state: State<'_, AppState>) {
    if let Some(session) = state.player.lock().unwrap().take() {
        session.alive.store(false, Ordering::Relaxed);
    }
}

#[tauri::command]
pub fn player_play(state: State<'_, AppState>) {
    if let Some(s) = state.player.lock().unwrap().as_ref() {
        s.player.play();
    }
}

#[tauri::command]
pub fn player_pause(state: State<'_, AppState>) {
    if let Some(s) = state.player.lock().unwrap().as_ref() {
        s.player.pause();
    }
}

#[tauri::command]
pub fn player_seek(state: State<'_, AppState>, seconds: f64) {
    if let Some(s) = state.player.lock().unwrap().as_ref() {
        s.player.seek(seconds);
    }
}

#[tauri::command]
pub fn player_set_rate(state: State<'_, AppState>, rate: f32) {
    if let Some(s) = state.player.lock().unwrap().as_ref() {
        s.player.set_rate(rate);
    }
}

#[tauri::command]
pub fn player_set_track_muted(state: State<'_, AppState>, track: usize, muted: bool) {
    if let Some(s) = state.player.lock().unwrap().as_ref() {
        s.player.set_muted(track, muted);
    }
}
