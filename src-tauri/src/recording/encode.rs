use fennec_core::session::{load_metadata, save_metadata};
use std::path::Path;

const SYS_BITRATE: i32 = 48000;
const MIC_BITRATE: i32 = 24000;

pub fn encode_session_audio(folder: &Path) -> anyhow::Result<bool> {
    let Some(mut metadata) = load_metadata(folder) else {
        return Ok(false);
    };
    let mut changed = false;

    for (field, bitrate) in [
        (&mut metadata.system_audio_file, SYS_BITRATE),
        (&mut metadata.mic_audio_file, MIC_BITRATE),
    ] {
        let Some(name) = field.clone() else { continue };
        if !name.ends_with(".wav") {
            continue;
        }
        let wav_path = folder.join(&name);
        if !wav_path.exists() {
            continue;
        }
        let ogg_name = format!("{}.ogg", name.trim_end_matches(".wav"));
        let ogg_path = folder.join(&ogg_name);
        fennec_audio::encode_wav_to_opus(&wav_path, &ogg_path, bitrate)?;
        std::fs::remove_file(&wav_path)?;
        *field = Some(ogg_name);
        changed = true;
    }

    if changed {
        save_metadata(folder, &metadata)?;
    }
    Ok(changed)
}

pub fn spawn_encode(app: tauri::AppHandle, folder: std::path::PathBuf) {
    tauri::async_runtime::spawn_blocking(move || match encode_session_audio(&folder) {
        Ok(true) => {
            crate::events::emit(&app, crate::events::SESSIONS_CHANGED, ());
        }
        Ok(false) => {}
        Err(e) => log::error!("opus encode failed for {}: {e:#}", folder.display()),
    });
}
