use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimedSegment {
    pub text: String,
    pub start: f64,
    pub end: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker_id: Option<i32>,
}

impl TimedSegment {
    pub fn new(text: impl Into<String>, start: f64, end: f64) -> Self {
        Self {
            text: text.into(),
            start,
            end,
            speaker_id: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeakerTimedSegment {
    pub speaker_id: i32,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptSource {
    System,
    Mic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptEntry {
    pub source: TranscriptSource,
    pub text: String,
    pub start_time: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker_id: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation: Option<String>,
}

impl TranscriptEntry {
    pub fn new(source: TranscriptSource, text: impl Into<String>, start_time: f64) -> Self {
        Self {
            source,
            text: text.into(),
            start_time,
            speaker_id: None,
            translation: None,
        }
    }
}
