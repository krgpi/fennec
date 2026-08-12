mod decode;
mod level;
mod mic;
mod opus;
mod player;
mod traits;
mod wav;

#[cfg(target_os = "macos")]
pub mod system_macos;

#[cfg(target_os = "linux")]
pub mod system_linux;

#[cfg(target_os = "windows")]
pub mod system_windows;

pub use decode::{
    decode_to_mono_16k, is_silent_audio, mixdown_mono, read_audio, read_wav, resample_mono,
    StreamDownmixResampler, WHISPER_SAMPLE_RATE,
};
pub use level::{rms, rms_to_level};
pub use opus::{decode_ogg_opus, encode_wav_to_opus};
pub use player::{Player, PlayerStatus, MAX_TRACKS};
pub use mic::{list_input_devices, MicCapture};
pub use traits::{AudioFormat, CaptureSource, ChunkCallback, InputDevice};
pub use wav::WavFileWriter;

pub const DEFAULT_MIC_GAIN: f32 = 2.0;

pub fn new_mic_capture(
    device_id: Option<&str>,
    gain: f32,
) -> anyhow::Result<Box<dyn CaptureSource>> {
    Ok(Box::new(MicCapture::new(device_id, gain)?))
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub fn new_system_capture(exclude_self: bool) -> anyhow::Result<Box<dyn CaptureSource>> {
    Ok(Box::new(system_adapter::SystemCaptureAdapter::new(
        exclude_self,
    )?))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub fn new_system_capture(_exclude_self: bool) -> anyhow::Result<Box<dyn CaptureSource>> {
    anyhow::bail!("system audio capture is not implemented for this platform yet")
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
mod system_adapter {
    #[cfg(target_os = "linux")]
    use crate::system_linux::SystemAudioCapture;
    #[cfg(target_os = "macos")]
    use crate::system_macos::SystemAudioCapture;
    #[cfg(target_os = "windows")]
    use crate::system_windows::SystemAudioCapture;
    use crate::traits::{AudioFormat, CaptureSource, ChunkCallback};

    pub struct SystemCaptureAdapter {
        inner: SystemAudioCapture,
    }

    impl SystemCaptureAdapter {
        pub fn new(exclude_self: bool) -> anyhow::Result<Self> {
            Ok(Self {
                inner: SystemAudioCapture::new(exclude_self)?,
            })
        }
    }

    impl CaptureSource for SystemCaptureAdapter {
        fn format(&self) -> AudioFormat {
            let f = self.inner.format();
            AudioFormat {
                sample_rate: f.sample_rate,
                channels: f.channels,
            }
        }

        fn start(&mut self, on_chunk: ChunkCallback) -> anyhow::Result<()> {
            self.inner.start(on_chunk)
        }

        fn stop(&mut self) {
            self.inner.stop()
        }
    }
}
