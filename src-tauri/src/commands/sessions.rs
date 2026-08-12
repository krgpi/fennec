use crate::events;
use crate::state::AppState;
use chrono::{DateTime, Utc};
use fennec_core::format::parse_transcript_text;
use fennec_core::minutes::content_hash;
use fennec_core::session::{
    load_metadata, save_metadata, scan_sessions, RecordingSession, TranscriptDocument,
};
use fennec_core::types::TranscriptEntry;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, State};

fn session_folder(
    app: &AppHandle,
    state: &State<'_, AppState>,
    session_id: &str,
) -> Result<PathBuf, String> {
    let folder = state.save_directory(app).join(session_id);
    if folder.join("session.json").exists() {
        Ok(folder)
    } else {
        Err(format!("session not found: {session_id}"))
    }
}

#[tauri::command]
pub fn list_sessions(app: AppHandle, state: State<'_, AppState>) -> Vec<RecordingSession> {
    scan_sessions(&state.save_directory(&app))
}

#[tauri::command]
pub fn read_transcript(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<TranscriptEntry>, String> {
    let folder = session_folder(&app, &state, &session_id)?;
    let json_path = folder.join(format!("transcript_{}.json", session_id));
    if let Some(doc) = TranscriptDocument::load(&json_path) {
        return Ok(doc.entries);
    }
    let metadata = load_metadata(&folder).ok_or("failed to read session.json")?;
    let Some(transcript_file) = metadata.transcript_file else {
        return Ok(Vec::new());
    };
    let text = std::fs::read_to_string(folder.join(transcript_file)).map_err(|e| e.to_string())?;
    Ok(parse_transcript_text(&text))
}

#[tauri::command]
pub fn read_note(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<String, String> {
    let folder = session_folder(&app, &state, &session_id)?;
    let path = folder.join(format!("note_{}.md", session_id));
    Ok(std::fs::read_to_string(path).unwrap_or_default())
}

#[tauri::command]
pub fn save_note(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    content: String,
) -> Result<(), String> {
    let folder = session_folder(&app, &state, &session_id)?;
    let path = folder.join(format!("note_{}.md", session_id));
    if content.trim().is_empty() {
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MinutesDocument {
    pub content: Option<String>,
    pub exported_at: Option<DateTime<Utc>>,
    pub exported_path: Option<PathBuf>,
    pub dirty: bool,
}

fn minutes_document(folder: &Path) -> Result<MinutesDocument, String> {
    let metadata = load_metadata(folder).ok_or("failed to read session.json")?;
    let content = metadata
        .minutes_file
        .as_ref()
        .and_then(|f| std::fs::read_to_string(folder.join(f)).ok());
    let dirty = match (&content, &metadata.minutes_exported_hash) {
        (Some(content), Some(hash)) => &content_hash(content) != hash,
        _ => false,
    };
    Ok(MinutesDocument {
        content,
        exported_at: metadata.minutes_exported_at,
        exported_path: metadata.minutes_exported_path,
        dirty,
    })
}

#[tauri::command]
pub fn read_minutes(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<MinutesDocument, String> {
    let folder = session_folder(&app, &state, &session_id)?;
    minutes_document(&folder)
}

#[tauri::command]
pub fn save_minutes(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    content: String,
) -> Result<MinutesDocument, String> {
    let folder = session_folder(&app, &state, &session_id)?;
    let mut metadata = load_metadata(&folder).ok_or("failed to read session.json")?;
    let file_name = metadata
        .minutes_file
        .clone()
        .unwrap_or_else(|| "minutes.md".to_string());
    std::fs::write(folder.join(&file_name), &content).map_err(|e| e.to_string())?;
    if metadata.minutes_file.is_none() {
        metadata.minutes_file = Some(file_name);
        save_metadata(&folder, &metadata).map_err(|e| e.to_string())?;
        events::emit(&app, events::SESSIONS_CHANGED, ());
    }
    minutes_document(&folder)
}

#[tauri::command]
pub fn export_minutes(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    content: String,
    target: PathBuf,
) -> Result<MinutesDocument, String> {
    let folder = session_folder(&app, &state, &session_id)?;
    let mut metadata = load_metadata(&folder).ok_or("failed to read session.json")?;
    let file_name = metadata
        .minutes_file
        .clone()
        .unwrap_or_else(|| "minutes.md".to_string());
    std::fs::write(folder.join(&file_name), &content).map_err(|e| e.to_string())?;
    std::fs::write(&target, &content)
        .map_err(|e| format!("{}: {e}", target.to_string_lossy()))?;
    metadata.minutes_file = Some(file_name);
    metadata.minutes_exported_at = Some(Utc::now());
    metadata.minutes_exported_hash = Some(content_hash(&content));
    metadata.minutes_exported_path = Some(target);
    save_metadata(&folder, &metadata).map_err(|e| e.to_string())?;
    events::emit(&app, events::SESSIONS_CHANGED, ());
    minutes_document(&folder)
}

#[tauri::command]
pub fn update_summary(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    summary: String,
) -> Result<(), String> {
    let folder = session_folder(&app, &state, &session_id)?;
    let mut metadata = load_metadata(&folder).ok_or("failed to read session.json")?;
    let trimmed = summary.trim();
    metadata.summary = (!trimmed.is_empty()).then(|| trimmed.to_string());
    save_metadata(&folder, &metadata).map_err(|e| e.to_string())
}

// macOS既定のFinder経由(osascript)はオートメーション権限プロンプトでハングするためNSFileManagerを使う
fn trash_delete(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        let mut ctx = trash::TrashContext::default();
        ctx.set_delete_method(DeleteMethod::NsFileManager);
        ctx.delete(path).map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        trash::delete(path).map_err(|e| e.to_string())
    }
}

pub fn delete_session_at(base: &std::path::Path, session_id: &str) -> Result<(), String> {
    let folder = base.join(session_id);
    if folder.join("session.json").exists() {
        trash_delete(&folder)
    } else {
        let mut deleted_any = false;
        for prefix in ["system_", "mic_", "transcript_", "note_"] {
            for ext in ["m4a", "wav", "ogg", "txt", "md", "json"] {
                let path = base.join(format!("{prefix}{session_id}.{ext}"));
                if path.exists() {
                    trash_delete(&path)?;
                    deleted_any = true;
                }
            }
        }
        if deleted_any {
            Ok(())
        } else {
            Err(format!("session not found: {session_id}"))
        }
    }
}

#[tauri::command]
pub fn delete_session(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    delete_session_at(&state.save_directory(&app), &session_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use fennec_core::session::SessionMetadata;

    fn session_with_minutes(body: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().to_path_buf();
        let mut metadata = SessionMetadata::new(Utc::now());
        metadata.minutes_file = Some("minutes.md".to_string());
        save_metadata(&folder, &metadata).unwrap();
        std::fs::write(folder.join("minutes.md"), body).unwrap();
        (dir, folder)
    }

    #[test]
    fn never_exported_is_not_dirty() {
        let (_dir, folder) = session_with_minutes("# 議事録");
        let doc = minutes_document(&folder).unwrap();
        assert_eq!(doc.content.as_deref(), Some("# 議事録"));
        assert!(doc.exported_at.is_none());
        assert!(!doc.dirty);
    }

    #[test]
    fn dirty_only_when_content_differs_from_export() {
        let (_dir, folder) = session_with_minutes("# 議事録");
        let mut metadata = load_metadata(&folder).unwrap();
        metadata.minutes_exported_at = Some(Utc::now());
        metadata.minutes_exported_hash = Some(content_hash("# 議事録"));
        save_metadata(&folder, &metadata).unwrap();
        assert!(!minutes_document(&folder).unwrap().dirty);

        std::fs::write(folder.join("minutes.md"), "# 議事録\n追記").unwrap();
        assert!(minutes_document(&folder).unwrap().dirty);
    }
}

#[tauri::command]
pub fn reveal_session_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let folder = session_folder(&app, &state, &session_id)?;
    tauri_plugin_opener::reveal_item_in_dir(&folder).map_err(|e| e.to_string())
}
