use crate::recording::controller::RecordingController;
use crate::services::hooks::HookFire;
use crate::transcription::job::TranscribeJob;
use fennec_core::settings::Settings;
use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};
use tauri::{AppHandle, Manager};

pub struct MinutesJob {
    pub session_id: String,
    pub child: Arc<Mutex<Option<Child>>>,
    pub cancelled: Arc<AtomicBool>,
}

pub struct PlayerSession {
    pub session_id: String,
    pub player: fennec_audio::Player,
    pub alive: Arc<AtomicBool>,
    pub has_system: bool,
    pub has_mic: bool,
}

pub struct AppState {
    pub settings: RwLock<Settings>,
    pub settings_path: PathBuf,
    pub recorder: Mutex<Option<RecordingController>>,
    pub transcribe_job: Mutex<Option<TranscribeJob>>,
    pub model_download: Mutex<Option<Arc<AtomicBool>>>,
    pub minutes_job: Mutex<Option<MinutesJob>>,
    pub player: Mutex<Option<PlayerSession>>,
    #[cfg(target_os = "macos")]
    pub sidecar: Mutex<Option<Arc<fennec_stt::SidecarClient>>>,
}

impl AppState {
    pub fn initialize(app: &AppHandle) -> anyhow::Result<Self> {
        let config_dir = app.path().app_config_dir()?;
        let settings_path = config_dir.join("settings.json");
        let settings = Settings::load(&settings_path);
        Ok(Self {
            settings: RwLock::new(settings),
            settings_path,
            recorder: Mutex::new(None),
            transcribe_job: Mutex::new(None),
            model_download: Mutex::new(None),
            minutes_job: Mutex::new(None),
            player: Mutex::new(None),
            #[cfg(target_os = "macos")]
            sidecar: Mutex::new(None),
        })
    }

    pub fn save_directory(&self, app: &AppHandle) -> PathBuf {
        let custom = self.settings.read().unwrap().save_directory.clone();
        custom.unwrap_or_else(|| default_save_directory(app))
    }

    pub fn persist_settings(&self) {
        let settings = self.settings.read().unwrap();
        if let Err(e) = settings.save(&self.settings_path) {
            log::error!("failed to save settings: {e}");
        }
    }

    #[cfg(target_os = "macos")]
    pub fn sidecar_client(&self) -> anyhow::Result<Arc<fennec_stt::SidecarClient>> {
        let mut guard = self.sidecar.lock().unwrap();
        if let Some(client) = guard.as_ref() {
            return Ok(client.clone());
        }
        let path = helper_binary_path()?;
        let client = Arc::new(fennec_stt::SidecarClient::spawn(&path)?);
        *guard = Some(client.clone());
        Ok(client)
    }

    pub fn hook_config(&self, app: &AppHandle) -> HookFire {
        let settings = self.settings.read().unwrap();
        HookFire {
            hooks: settings.automation_hooks.clone(),
            timeout_seconds: settings.hook_timeout_seconds,
            log_path: app
                .path()
                .app_log_dir()
                .map(|d| d.join("hooks.log"))
                .unwrap_or_else(|_| PathBuf::from("hooks.log")),
        }
    }
}

pub fn default_save_directory(app: &AppHandle) -> PathBuf {
    app.path()
        .document_dir()
        .map(|d| d.join("Fennec"))
        .unwrap_or_else(|_| PathBuf::from("Fennec"))
}

#[cfg(target_os = "macos")]
fn helper_binary_path() -> anyhow::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let dir = exe
        .parent()
        .ok_or_else(|| anyhow::anyhow!("no executable directory"))?;
    let bundled = dir.join("fennec-helper");
    if bundled.exists() {
        return Ok(bundled);
    }
    #[cfg(debug_assertions)]
    {
        let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("binaries/fennec-helper-aarch64-apple-darwin");
        if dev.exists() {
            return Ok(dev);
        }
    }
    anyhow::bail!("fennec-helper binary not found")
}
