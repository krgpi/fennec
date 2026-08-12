use crate::hooks::AutomationHook;
use crate::ipc::{parse_bool_arg, CONFIG_KEYS};
use crate::minutes::MinutesPreset;
use crate::models::DEFAULT_WHISPER_MODEL_ID;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

fn default_true() -> bool {
    true
}

fn default_engine() -> String {
    if cfg!(target_os = "macos") {
        "apple".to_string()
    } else {
        "whisper".to_string()
    }
}

fn default_whisper_model_id() -> String {
    DEFAULT_WHISPER_MODEL_ID.to_string()
}

fn default_silence_minutes() -> u32 {
    5
}

fn default_hook_timeout() -> u32 {
    30
}

fn default_reminder_minutes() -> u32 {
    5
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub save_directory: Option<PathBuf>,
    pub app_language: String,
    pub show_in_menu_bar: bool,
    pub hide_from_dock: bool,
    pub launch_at_login: bool,
    pub use_system_default_mic: bool,
    pub mic_device_id: Option<String>,
    pub transcription_locale: Option<String>,
    pub live_transcription_enabled: bool,
    pub live_transcription_engine: String,
    pub auto_transcribe_enabled: bool,
    pub auto_transcribe_engine: String,
    pub translation_enabled: bool,
    pub diarization_enabled: bool,
    pub silence_auto_stop_enabled: bool,
    pub silence_auto_stop_minutes: u32,
    pub meeting_reminder_enabled: bool,
    pub meeting_reminder_minutes_before: u32,
    pub meeting_reminder_require_meeting_url: bool,
    pub meeting_reminder_require_mic: bool,
    pub meeting_reminder_calendar_identifiers: Option<Vec<String>>,
    pub calendar_ics_urls: Vec<String>,
    pub whisper_enabled: bool,
    pub whisper_model_id: String,
    pub whisper_downloaded_models: HashMap<String, String>,
    pub hook_timeout_seconds: u32,
    pub automation_hooks: Vec<AutomationHook>,
    pub minutes_presets: Vec<MinutesPreset>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            save_directory: None,
            app_language: "system".to_string(),
            show_in_menu_bar: false,
            hide_from_dock: false,
            launch_at_login: false,
            use_system_default_mic: true,
            mic_device_id: None,
            transcription_locale: None,
            live_transcription_enabled: true,
            live_transcription_engine: default_engine(),
            auto_transcribe_enabled: true,
            auto_transcribe_engine: default_engine(),
            translation_enabled: true,
            diarization_enabled: false,
            silence_auto_stop_enabled: false,
            silence_auto_stop_minutes: default_silence_minutes(),
            meeting_reminder_enabled: false,
            meeting_reminder_minutes_before: default_reminder_minutes(),
            meeting_reminder_require_meeting_url: true,
            meeting_reminder_require_mic: true,
            meeting_reminder_calendar_identifiers: None,
            calendar_ics_urls: Vec::new(),
            whisper_enabled: !cfg!(target_os = "macos"),
            whisper_model_id: default_whisper_model_id(),
            whisper_downloaded_models: HashMap::new(),
            hook_timeout_seconds: default_hook_timeout(),
            automation_hooks: Vec::new(),
            minutes_presets: Vec::new(),
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, json)?;
        fs::rename(&tmp, path)
    }

    pub fn config_list(&self) -> Vec<(String, String)> {
        CONFIG_KEYS
            .iter()
            .map(|&key| (key.to_string(), self.config_get(key).unwrap_or_default()))
            .collect()
    }

    pub fn config_get(&self, key: &str) -> Option<String> {
        match key {
            "liveTranscriptionEnabled" => Some(self.live_transcription_enabled.to_string()),
            "autoTranscribeEnabled" => Some(self.auto_transcribe_enabled.to_string()),
            "autoTranscribeEngine" => Some(self.auto_transcribe_engine.clone()),
            "silenceAutoStopEnabled" => Some(self.silence_auto_stop_enabled.to_string()),
            "silenceAutoStopMinutes" => Some(self.silence_auto_stop_minutes.to_string()),
            "transcriptionLocale" => Some(self.transcription_locale.clone().unwrap_or_default()),
            "whisperModelId" => Some(self.whisper_model_id.clone()),
            "hookTimeoutSeconds" => Some(self.hook_timeout_seconds.to_string()),
            _ => None,
        }
    }

    pub fn config_set(&mut self, key: &str, value: &str) -> Result<(), String> {
        let bool_value = || {
            parse_bool_arg(value).ok_or_else(|| format!("invalid boolean value: {value}"))
        };
        match key {
            "liveTranscriptionEnabled" => self.live_transcription_enabled = bool_value()?,
            "autoTranscribeEnabled" => self.auto_transcribe_enabled = bool_value()?,
            "autoTranscribeEngine" => {
                if value != "apple" && value != "whisper" {
                    return Err(format!("invalid engine: {value}"));
                }
                self.auto_transcribe_engine = value.to_string();
            }
            "silenceAutoStopEnabled" => self.silence_auto_stop_enabled = bool_value()?,
            "silenceAutoStopMinutes" => {
                self.silence_auto_stop_minutes = value
                    .parse()
                    .map_err(|_| format!("invalid number: {value}"))?;
            }
            "transcriptionLocale" => {
                self.transcription_locale =
                    (!value.is_empty()).then(|| value.to_string());
            }
            "whisperModelId" => self.whisper_model_id = value.to_string(),
            "hookTimeoutSeconds" => {
                self.hook_timeout_seconds = value
                    .parse()
                    .map_err(|_| format!("invalid number: {value}"))?;
            }
            _ => return Err(format!("unknown config key: {key}")),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_existing_app() {
        let s = Settings::default();
        assert!(s.live_transcription_enabled);
        assert!(s.auto_transcribe_enabled);
        assert!(s.translation_enabled);
        assert!(s.use_system_default_mic);
        assert!(s.meeting_reminder_require_meeting_url);
        assert!(s.meeting_reminder_require_mic);
        assert!(!s.silence_auto_stop_enabled);
        assert_eq!(s.silence_auto_stop_minutes, 5);
        assert_eq!(s.hook_timeout_seconds, 30);
        assert_eq!(s.meeting_reminder_minutes_before, 5);
    }

    #[test]
    fn config_whitelist_roundtrip() {
        let mut s = Settings::default();
        for (key, _) in s.config_list() {
            assert!(s.config_get(&key).is_some());
        }
        s.config_set("liveTranscriptionEnabled", "off").unwrap();
        assert!(!s.live_transcription_enabled);
        s.config_set("silenceAutoStopMinutes", "10").unwrap();
        assert_eq!(s.silence_auto_stop_minutes, 10);
        s.config_set("autoTranscribeEngine", "whisper").unwrap();
        assert_eq!(s.auto_transcribe_engine, "whisper");

        assert!(s.config_set("autoTranscribeEngine", "google").is_err());
        assert!(s.config_set("unknownKey", "1").is_err());
        assert!(s.config_set("liveTranscriptionEnabled", "maybe").is_err());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut s = Settings::default();
        s.whisper_model_id = "ggml-small".to_string();
        s.calendar_ics_urls.push("https://example.com/cal.ics".to_string());
        s.save(&path).unwrap();

        let loaded = Settings::load(&path);
        assert_eq!(loaded, s);
    }

    #[test]
    fn load_missing_file_gives_defaults() {
        let loaded = Settings::load(Path::new("/nonexistent/settings.json"));
        assert_eq!(loaded, Settings::default());
    }

    #[test]
    fn unknown_fields_tolerated() {
        let json = r#"{"liveTranscriptionEnabled": false, "futureField": 42}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert!(!s.live_transcription_enabled);
        assert!(s.auto_transcribe_enabled);
    }
}
