use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;

use anyhow::{bail, Context, Result};
use pipewire as pw;
use pw::{properties::properties, spa};
use spa::pod::Pod;

const SAMPLE_RATE: u32 = 48000;
const CHANNELS: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFormatInfo {
    pub sample_rate: u32,
    pub channels: u16,
}

struct Terminate;

pub struct SystemAudioCapture {
    quit_tx: Option<pw::channel::Sender<Terminate>>,
    thread: Option<JoinHandle<()>>,
}

impl SystemAudioCapture {
    pub fn new(_exclude_current_process: bool) -> Result<Self> {
        pw::init();
        let mainloop = pw::main_loop::MainLoop::new(None)
            .context("failed to create PipeWire main loop")?;
        let context = pw::context::Context::new(&mainloop)
            .context("failed to create PipeWire context")?;
        let _core = context
            .connect(None)
            .context("failed to connect to PipeWire daemon (is PipeWire running?)")?;
        Ok(Self {
            quit_tx: None,
            thread: None,
        })
    }

    pub fn format(&self) -> AudioFormatInfo {
        AudioFormatInfo {
            sample_rate: SAMPLE_RATE,
            channels: CHANNELS,
        }
    }

    pub fn start(&mut self, on_chunk: Arc<dyn Fn(&[f32]) + Send + Sync>) -> Result<()> {
        if self.thread.is_some() {
            bail!("system audio capture is already started");
        }
        let (quit_tx, quit_rx) = pw::channel::channel::<Terminate>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<()>>();
        let handle = std::thread::Builder::new()
            .name("fennec-system-capture".to_string())
            .spawn(move || capture_thread(on_chunk, quit_rx, ready_tx))
            .context("failed to spawn system audio capture thread")?;
        match ready_rx.recv() {
            Ok(Ok(())) => {
                self.quit_tx = Some(quit_tx);
                self.thread = Some(handle);
                Ok(())
            }
            Ok(Err(e)) => {
                let _ = handle.join();
                Err(e.context("failed to start PipeWire monitor capture"))
            }
            Err(_) => {
                let _ = handle.join();
                bail!("system audio capture thread exited unexpectedly");
            }
        }
    }

    pub fn stop(&mut self) {
        if let Some(quit_tx) = self.quit_tx.take() {
            let _ = quit_tx.send(Terminate);
        }
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for SystemAudioCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

fn capture_thread(
    on_chunk: Arc<dyn Fn(&[f32]) + Send + Sync>,
    quit_rx: pw::channel::Receiver<Terminate>,
    ready_tx: mpsc::Sender<Result<()>>,
) {
    match build_stream(on_chunk) {
        Ok((mainloop, stream, listener)) => {
            let loop_clone = mainloop.clone();
            let _quit_guard = quit_rx.attach(mainloop.loop_(), move |_| loop_clone.quit());
            let _ = ready_tx.send(Ok(()));
            mainloop.run();
            let _ = stream.disconnect();
            drop(listener);
        }
        Err(e) => {
            let _ = ready_tx.send(Err(e));
        }
    }
}

fn build_stream(
    on_chunk: Arc<dyn Fn(&[f32]) + Send + Sync>,
) -> Result<(
    pw::main_loop::MainLoop,
    pw::stream::Stream,
    pw::stream::StreamListener<()>,
)> {
    let mainloop =
        pw::main_loop::MainLoop::new(None).context("failed to create PipeWire main loop")?;
    let context =
        pw::context::Context::new(&mainloop).context("failed to create PipeWire context")?;
    let core = context
        .connect(None)
        .context("failed to connect to PipeWire daemon (is PipeWire running?)")?;

    let props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Music",
        *pw::keys::NODE_NAME => "fennec-system-capture",
        *pw::keys::STREAM_CAPTURE_SINK => "true",
    };
    let stream = pw::stream::Stream::new(&core, "fennec-system-capture", props)
        .context("failed to create PipeWire stream")?;

    let mut scratch: Vec<f32> = Vec::new();
    let listener = stream
        .add_local_listener::<()>()
        .process(move |stream, _| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let datas = buffer.datas_mut();
            if datas.is_empty() {
                return;
            }
            let data = &mut datas[0];
            let size = data.chunk().size() as usize;
            let Some(bytes) = data.data() else {
                return;
            };
            let n_samples = size.min(bytes.len()) / std::mem::size_of::<f32>();
            if n_samples == 0 {
                return;
            }
            scratch.clear();
            scratch.extend(
                bytes[..n_samples * std::mem::size_of::<f32>()]
                    .chunks_exact(std::mem::size_of::<f32>())
                    .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            );
            (on_chunk)(&scratch);
        })
        .register()
        .context("failed to register PipeWire stream listener")?;

    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(spa::param::audio::AudioFormat::F32LE);
    audio_info.set_rate(SAMPLE_RATE);
    audio_info.set_channels(CHANNELS as u32);
    let obj = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };
    let values: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    )
    .context("failed to serialize stream format param")?
    .0
    .into_inner();
    let mut params = [Pod::from_bytes(&values).context("failed to build stream format pod")?];

    stream
        .connect(
            spa::utils::Direction::Input,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut params,
        )
        .context("failed to connect PipeWire stream")?;

    Ok((mainloop, stream, listener))
}
