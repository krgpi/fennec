#[cfg(target_os = "macos")]
pub mod apple;
mod batch;
mod diarize;
mod download;
mod live;

#[cfg(target_os = "macos")]
pub use apple::{AppleLiveEvent, SidecarClient};
pub use batch::WhisperBatchTranscriber;
pub use diarize::{
    delete_diarization_models, diarization_models_at, diarize_file, download_diarization_models,
    DiarizationModelPaths,
};
pub use download::{download_file, download_model};
pub use live::{LiveEngine, LiveEvent, WhisperLiveTranscriber};

use fennec_core::types::TimedSegment;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct TranscribeOptions {
    pub language: String,
    pub model_path: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct TranscriptResult {
    pub text: String,
    pub segments: Vec<TimedSegment>,
}
