use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HookEvent {
    RecordingStarted,
    RecordingStopped,
    TranscriptionCompleted,
    MinutesGenerated,
}

impl HookEvent {
    pub const ALL: [HookEvent; 4] = [
        Self::RecordingStarted,
        Self::RecordingStopped,
        Self::TranscriptionCompleted,
        Self::MinutesGenerated,
    ];

    pub fn raw_value(self) -> &'static str {
        match self {
            Self::RecordingStarted => "recordingStarted",
            Self::RecordingStopped => "recordingStopped",
            Self::TranscriptionCompleted => "transcriptionCompleted",
            Self::MinutesGenerated => "minutesGenerated",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.raw_value() == raw)
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationHook {
    pub id: Uuid,
    pub event: HookEvent,
    pub command: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl AutomationHook {
    pub fn new(event: HookEvent, command: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            event,
            command: command.into(),
            enabled: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_raw_values_match_swift() {
        assert_eq!(HookEvent::RecordingStarted.raw_value(), "recordingStarted");
        assert_eq!(HookEvent::MinutesGenerated.raw_value(), "minutesGenerated");
        assert_eq!(
            HookEvent::from_raw("transcriptionCompleted"),
            Some(HookEvent::TranscriptionCompleted)
        );
        assert_eq!(HookEvent::from_raw("unknown"), None);
    }

    #[test]
    fn serde_uses_camel_case() {
        let hook = AutomationHook::new(HookEvent::RecordingStopped, "echo hi");
        let json = serde_json::to_string(&hook).unwrap();
        assert!(json.contains("\"recordingStopped\""));
        let parsed: AutomationHook = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, hook);
    }

    #[test]
    fn enabled_defaults_true() {
        let json = r#"{"id":"6E4A1F1C-93B1-43A1-9E5A-111111111111","event":"recordingStarted","command":"echo"}"#;
        let hook: AutomationHook = serde_json::from_str(json).unwrap();
        assert!(hook.enabled);
    }
}
