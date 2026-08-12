use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;

use anyhow::{bail, Context, Result};
use wasapi::{
    initialize_mta, AudioCaptureClient, AudioClient, DeviceEnumerator, Direction, Handle,
    SampleType, StreamMode, WaveFormat,
};

const BYTES_PER_SAMPLE: usize = 4;
const WAIT_TIMEOUT_MS: u32 = 250;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFormatInfo {
    pub sample_rate: u32,
    pub channels: u16,
}

pub struct SystemAudioCapture {
    format: AudioFormatInfo,
    stop_flag: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl SystemAudioCapture {
    pub fn new(_exclude_current_process: bool) -> Result<Self> {
        // ignore the result: S_FALSE / RPC_E_CHANGED_MODE mean COM is already usable on this thread
        let _ = initialize_mta();
        let mix = default_render_mixformat()?;
        Ok(Self {
            format: AudioFormatInfo {
                sample_rate: mix.get_samplespersec(),
                channels: mix.get_nchannels(),
            },
            stop_flag: Arc::new(AtomicBool::new(false)),
            thread: None,
        })
    }

    pub fn format(&self) -> AudioFormatInfo {
        self.format
    }

    pub fn start(&mut self, on_chunk: Arc<dyn Fn(&[f32]) + Send + Sync>) -> Result<()> {
        if self.thread.is_some() {
            bail!("system audio capture is already started");
        }
        self.stop_flag.store(false, Ordering::Release);
        let stop_flag = Arc::clone(&self.stop_flag);
        let format = self.format;
        let (ready_tx, ready_rx) = mpsc::channel::<Result<()>>();
        let handle = std::thread::Builder::new()
            .name("fennec-system-capture".to_string())
            .spawn(move || capture_thread(format, on_chunk, stop_flag, ready_tx))
            .context("failed to spawn system audio capture thread")?;
        match ready_rx.recv() {
            Ok(Ok(())) => {
                self.thread = Some(handle);
                Ok(())
            }
            Ok(Err(e)) => {
                let _ = handle.join();
                Err(e.context("failed to start WASAPI loopback capture"))
            }
            Err(_) => {
                let _ = handle.join();
                bail!("system audio capture thread exited unexpectedly");
            }
        }
    }

    pub fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::Release);
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

fn default_render_mixformat() -> Result<WaveFormat> {
    let enumerator = DeviceEnumerator::new().context("failed to create device enumerator")?;
    let device = enumerator
        .get_default_device(&Direction::Render)
        .context("failed to get default render device")?;
    let audio_client = device
        .get_iaudioclient()
        .context("failed to get audio client from render device")?;
    audio_client
        .get_mixformat()
        .context("failed to get render device mix format")
}

struct LoopbackSession {
    audio_client: AudioClient,
    capture_client: AudioCaptureClient,
    event: Handle,
    blockalign: usize,
}

fn setup_loopback(format: AudioFormatInfo) -> Result<LoopbackSession> {
    let enumerator = DeviceEnumerator::new().context("failed to create device enumerator")?;
    let device = enumerator
        .get_default_device(&Direction::Render)
        .context("failed to get default render device")?;
    let mut audio_client = device
        .get_iaudioclient()
        .context("failed to get audio client from render device")?;

    let desired_format = WaveFormat::new(
        32,
        32,
        &SampleType::Float,
        format.sample_rate as usize,
        format.channels as usize,
        None,
    );
    let (_def_time, min_time) = audio_client
        .get_device_period()
        .context("failed to get device period")?;
    let mode = StreamMode::EventsShared {
        autoconvert: true,
        buffer_duration_hns: min_time,
    };
    // render device + Direction::Capture initializes the client in loopback mode
    audio_client
        .initialize_client(&desired_format, &Direction::Capture, &mode)
        .context("failed to initialize loopback capture client")?;
    let event = audio_client
        .set_get_eventhandle()
        .context("failed to create capture event handle")?;
    let capture_client = audio_client
        .get_audiocaptureclient()
        .context("failed to get capture client")?;
    audio_client
        .start_stream()
        .context("failed to start capture stream")?;

    Ok(LoopbackSession {
        audio_client,
        capture_client,
        event,
        blockalign: desired_format.get_blockalign() as usize,
    })
}

fn capture_thread(
    format: AudioFormatInfo,
    on_chunk: Arc<dyn Fn(&[f32]) + Send + Sync>,
    stop_flag: Arc<AtomicBool>,
    ready_tx: mpsc::Sender<Result<()>>,
) {
    let _ = initialize_mta();
    let session = match setup_loopback(format) {
        Ok(session) => session,
        Err(e) => {
            let _ = ready_tx.send(Err(e));
            return;
        }
    };
    let _ = ready_tx.send(Ok(()));
    run_capture(&session, on_chunk, &stop_flag);
    let _ = session.audio_client.stop_stream();
}

fn run_capture(
    session: &LoopbackSession,
    on_chunk: Arc<dyn Fn(&[f32]) + Send + Sync>,
    stop_flag: &AtomicBool,
) {
    let mut queue: VecDeque<u8> = VecDeque::new();
    let mut scratch: Vec<f32> = Vec::new();
    loop {
        if stop_flag.load(Ordering::Acquire) {
            break;
        }
        let mut failed = false;
        loop {
            match session.capture_client.get_next_packet_size() {
                Ok(Some(frames)) if frames > 0 => {
                    if session
                        .capture_client
                        .read_from_device_to_deque(&mut queue)
                        .is_err()
                    {
                        failed = true;
                        break;
                    }
                }
                Ok(_) => break,
                Err(_) => {
                    failed = true;
                    break;
                }
            }
        }
        emit_f32(&mut queue, &mut scratch, session.blockalign, &on_chunk);
        if failed {
            break;
        }
        let _ = session.event.wait_for_event(WAIT_TIMEOUT_MS);
    }
}

fn emit_f32(
    queue: &mut VecDeque<u8>,
    scratch: &mut Vec<f32>,
    blockalign: usize,
    on_chunk: &Arc<dyn Fn(&[f32]) + Send + Sync>,
) {
    let frames = queue.len() / blockalign.max(1);
    let samples = frames * (blockalign / BYTES_PER_SAMPLE);
    if samples == 0 {
        return;
    }
    scratch.clear();
    for _ in 0..samples {
        let mut bytes = [0u8; BYTES_PER_SAMPLE];
        for b in bytes.iter_mut() {
            *b = queue.pop_front().unwrap_or(0);
        }
        scratch.push(f32::from_le_bytes(bytes));
    }
    (on_chunk)(scratch);
}
