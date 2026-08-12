use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptionEngine {
    Apple,
    Whisper,
}

impl TranscriptionEngine {
    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Apple => "apple",
            Self::Whisper => "whisper",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Self> {
        match raw {
            "apple" => Some(Self::Apple),
            "whisper" => Some(Self::Whisper),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperModelInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub size: &'static str,
    pub detail: &'static str,
    pub url: &'static str,
    pub file_name: &'static str,
}

pub const WHISPER_MODELS: [WhisperModelInfo; 4] = [
    WhisperModelInfo {
        id: "ggml-small",
        name: "Small",
        size: "約466MB",
        detail: "高速・省メモリ",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
        file_name: "ggml-small.bin",
    },
    WhisperModelInfo {
        id: "ggml-large-v3-turbo-q5_0",
        name: "Large V3 Turbo（推奨）",
        size: "約574MB",
        detail: "高精度・量子化済み",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
        file_name: "ggml-large-v3-turbo-q5_0.bin",
    },
    WhisperModelInfo {
        id: "ggml-large-v3-turbo",
        name: "Large V3 Turbo（フル）",
        size: "約1.6GB",
        detail: "高精度",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin",
        file_name: "ggml-large-v3-turbo.bin",
    },
    WhisperModelInfo {
        id: "ggml-large-v3",
        name: "Large V3（フル）",
        size: "約3.1GB",
        detail: "最高精度",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin",
        file_name: "ggml-large-v3.bin",
    },
];

pub const DEFAULT_WHISPER_MODEL_ID: &str = "ggml-large-v3-turbo-q5_0";

pub fn whisper_model(id: &str) -> Option<&'static WhisperModelInfo> {
    WHISPER_MODELS.iter().find(|m| m.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_model_exists() {
        assert!(whisper_model(DEFAULT_WHISPER_MODEL_ID).is_some());
        assert!(whisper_model("nonexistent").is_none());
    }

    #[test]
    fn engine_raw_values() {
        assert_eq!(TranscriptionEngine::Apple.raw_value(), "apple");
        assert_eq!(TranscriptionEngine::from_raw("whisper"), Some(TranscriptionEngine::Whisper));
    }
}
