use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub command: String,
    #[serde(default)]
    pub args: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Progress {
    pub progress: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fraction: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn ok(data: serde_json::Value) -> Self {
        Self {
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn ok_empty() -> Self {
        Self {
            ok: true,
            data: None,
            error: None,
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(message.into()),
        }
    }
}

pub const CONFIG_KEYS: [&str; 8] = [
    "liveTranscriptionEnabled",
    "autoTranscribeEnabled",
    "autoTranscribeEngine",
    "silenceAutoStopEnabled",
    "silenceAutoStopMinutes",
    "transcriptionLocale",
    "whisperModelId",
    "hookTimeoutSeconds",
];

pub fn parse_bool_arg(value: &str) -> Option<bool> {
    match value.to_lowercase().as_str() {
        "true" | "1" | "on" | "yes" => Some(true),
        "false" | "0" | "off" | "no" => Some(false),
        _ => None,
    }
}

pub const IS_DEBUG: bool = cfg!(debug_assertions);

#[cfg(target_os = "macos")]
pub fn socket_name(debug: bool) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let file = if debug { "fennec.debug.sock" } else { "fennec.sock" };
    format!("{home}/Library/Application Support/Fennec/{file}")
}

#[cfg(target_os = "linux")]
pub fn socket_name(debug: bool) -> String {
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    let file = if debug { "fennec.debug.sock" } else { "fennec.sock" };
    format!("{dir}/{file}")
}

#[cfg(target_os = "windows")]
pub fn socket_name(debug: bool) -> String {
    if debug {
        r"\\.\pipe\fennec.debug".to_string()
    } else {
        r"\\.\pipe\fennec".to_string()
    }
}

pub fn encode_line<T: Serialize>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
    let mut data = serde_json::to_vec(value)?;
    data.push(b'\n');
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_from_swift_cli_json() {
        let json = r#"{"command":"transcribe","args":{"sessionId":"latest","engine":"whisper"}}"#;
        let req: Request = serde_json::from_str(json).unwrap();
        assert_eq!(req.command, "transcribe");
        assert_eq!(req.args.get("sessionId").map(String::as_str), Some("latest"));
    }

    #[test]
    fn request_without_args() {
        let req: Request = serde_json::from_str(r#"{"command":"status"}"#).unwrap();
        assert!(req.args.is_empty());
    }

    #[test]
    fn response_shape_matches_protocol() {
        let ok = serde_json::to_string(&Response::ok(serde_json::json!({"sessionId": "x"}))).unwrap();
        assert_eq!(ok, r#"{"ok":true,"data":{"sessionId":"x"}}"#);

        let err = serde_json::to_string(&Response::err("boom")).unwrap();
        assert_eq!(err, r#"{"ok":false,"error":"boom"}"#);

        let empty = serde_json::to_string(&Response::ok_empty()).unwrap();
        assert_eq!(empty, r#"{"ok":true}"#);
    }

    #[test]
    fn progress_omits_missing_fraction() {
        let p = serde_json::to_string(&Progress { progress: "downloading".into(), fraction: None }).unwrap();
        assert_eq!(p, r#"{"progress":"downloading"}"#);
        let p = serde_json::to_string(&Progress { progress: "x".into(), fraction: Some(0.42) }).unwrap();
        assert_eq!(p, r#"{"progress":"x","fraction":0.42}"#);
    }

    #[test]
    fn bool_parsing_matches_command_server() {
        for v in ["true", "1", "on", "yes", "TRUE", "Yes"] {
            assert_eq!(parse_bool_arg(v), Some(true));
        }
        for v in ["false", "0", "off", "no"] {
            assert_eq!(parse_bool_arg(v), Some(false));
        }
        assert_eq!(parse_bool_arg("maybe"), None);
    }

    #[test]
    fn encode_line_appends_newline() {
        let line = encode_line(&Response::ok_empty()).unwrap();
        assert_eq!(line.last(), Some(&b'\n'));
    }
}
