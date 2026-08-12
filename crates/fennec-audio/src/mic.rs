use crate::traits::{AudioFormat, CaptureSource, ChunkCallback, InputDevice};
use anyhow::{anyhow, Context};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::mpsc;
use std::thread::JoinHandle;

pub fn list_input_devices() -> Vec<InputDevice> {
    let host = cpal::default_host();
    let default_name = host.default_input_device().and_then(|d| d.name().ok());
    let Ok(devices) = host.input_devices() else {
        return Vec::new();
    };
    devices
        .filter_map(|d| {
            let name = d.name().ok()?;
            let config = d.default_input_config().ok()?;
            Some(InputDevice {
                id: name.clone(),
                is_default: default_name.as_deref() == Some(name.as_str()),
                name,
                sample_rate: config.sample_rate().0,
                channels: config.channels(),
            })
        })
        .collect()
}

fn find_device(device_id: Option<&str>) -> anyhow::Result<cpal::Device> {
    let host = cpal::default_host();
    match device_id {
        Some(id) => host
            .input_devices()?
            .find(|d| d.name().map(|n| n == id).unwrap_or(false))
            .ok_or_else(|| anyhow!("input device not found: {id}")),
        None => host
            .default_input_device()
            .ok_or_else(|| anyhow!("no default input device")),
    }
}

pub struct MicCapture {
    device_id: Option<String>,
    gain: f32,
    format: AudioFormat,
    control: Option<(mpsc::Sender<()>, JoinHandle<()>)>,
}

impl MicCapture {
    pub fn new(device_id: Option<&str>, gain: f32) -> anyhow::Result<Self> {
        let device = find_device(device_id)?;
        let config = device
            .default_input_config()
            .context("no default input config")?;
        Ok(Self {
            device_id: device_id.map(String::from),
            gain,
            format: AudioFormat {
                sample_rate: config.sample_rate().0,
                channels: config.channels(),
            },
            control: None,
        })
    }
}

impl CaptureSource for MicCapture {
    fn format(&self) -> AudioFormat {
        self.format
    }

    fn start(&mut self, on_chunk: ChunkCallback) -> anyhow::Result<()> {
        // cpal::Stream は !Send のため、専用スレッドに所有させて停止までパークする
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<anyhow::Result<()>>();
        let device_id = self.device_id.clone();
        let gain = self.gain;

        let handle = std::thread::Builder::new()
            .name("fennec-mic-capture".into())
            .spawn(move || {
                let built = build_and_play(device_id.as_deref(), gain, on_chunk);
                match built {
                    Ok(stream) => {
                        let _ = ready_tx.send(Ok(()));
                        let _ = stop_rx.recv();
                        drop(stream);
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                }
            })?;

        ready_rx
            .recv()
            .context("mic capture thread terminated unexpectedly")??;
        self.control = Some((stop_tx, handle));
        Ok(())
    }

    fn stop(&mut self) {
        if let Some((stop_tx, handle)) = self.control.take() {
            let _ = stop_tx.send(());
            let _ = handle.join();
        }
    }
}

impl Drop for MicCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

fn build_and_play(
    device_id: Option<&str>,
    gain: f32,
    on_chunk: ChunkCallback,
) -> anyhow::Result<cpal::Stream> {
    let device = find_device(device_id)?;
    let supported = device.default_input_config()?;
    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let err_fn = |e| eprintln!("mic capture stream error: {e}");

    let stream = match sample_format {
        cpal::SampleFormat::F32 => {
            let mut buf: Vec<f32> = Vec::new();
            device.build_input_stream(
                &config,
                move |data: &[f32], _| {
                    buf.clear();
                    buf.extend(data.iter().map(|s| (s * gain).clamp(-1.0, 1.0)));
                    on_chunk(&buf);
                },
                err_fn,
                None,
            )?
        }
        cpal::SampleFormat::I16 => {
            let mut buf: Vec<f32> = Vec::new();
            device.build_input_stream(
                &config,
                move |data: &[i16], _| {
                    buf.clear();
                    buf.extend(
                        data.iter()
                            .map(|&s| (s as f32 / i16::MAX as f32 * gain).clamp(-1.0, 1.0)),
                    );
                    on_chunk(&buf);
                },
                err_fn,
                None,
            )?
        }
        other => return Err(anyhow!("unsupported sample format: {other:?}")),
    };
    stream.play()?;
    Ok(stream)
}
