use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

pub type ChunkCallback = Arc<dyn Fn(&[f32]) + Send + Sync>;

pub trait CaptureSource: Send {
    fn format(&self) -> AudioFormat;
    fn start(&mut self, on_chunk: ChunkCallback) -> anyhow::Result<()>;
    fn stop(&mut self);
}

#[derive(Debug, Clone, PartialEq)]
pub struct InputDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub sample_rate: u32,
    pub channels: u16,
}
