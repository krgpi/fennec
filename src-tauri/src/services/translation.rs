use crate::state::AppState;
use fennec_core::session::TranscriptDocument;
use std::path::Path;
use tauri::{AppHandle, Manager};

pub fn translate_transcript(app: &AppHandle, folder: &Path, session_id: &str) {
    #[cfg(target_os = "macos")]
    {
        let state = app.state::<AppState>();
        let (enabled, target) = {
            let settings = state.settings.read().unwrap();
            let target = match settings.app_language.as_str() {
                "en" => "en",
                "ja" => "ja",
                _ => {
                    if std::env::var("LANG").unwrap_or_default().starts_with("en") {
                        "en"
                    } else {
                        "ja"
                    }
                }
            };
            (settings.translation_enabled, target.to_string())
        };
        if !enabled {
            return;
        }

        let json_path = folder.join(format!("transcript_{}.json", session_id));
        let Some(mut doc) = TranscriptDocument::load(&json_path) else {
            return;
        };
        if doc.entries.is_empty() {
            return;
        }

        let Ok(client) = state.sidecar_client() else {
            return;
        };
        let texts: Vec<String> = doc.entries.iter().map(|e| e.text.clone()).collect();
        match client.translate(&texts, &target) {
            Ok(translations) => {
                let mut changed = false;
                for (entry, translation) in doc.entries.iter_mut().zip(translations) {
                    if let Some(translation) = translation {
                        if !translation.is_empty() {
                            entry.translation = Some(translation);
                            changed = true;
                        }
                    }
                }
                if changed {
                    if let Err(e) = doc.save(&json_path) {
                        log::warn!("failed to save translated transcript: {e}");
                    } else {
                        crate::events::emit(app, crate::events::SESSIONS_CHANGED, ());
                    }
                }
            }
            Err(e) => log::warn!("translation failed: {e:#}"),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, folder, session_id);
    }
}
