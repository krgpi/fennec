use fennec_audio::{new_mic_capture, new_system_capture, AudioFormat, CaptureSource, DEFAULT_MIC_GAIN};
use std::collections::VecDeque;
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use tauri::{AppHandle, Manager};

pub const PREROLL_SECONDS: usize = 600;

pub type Ring = VecDeque<Vec<i16>>;

pub enum LaneChunk {
    Samples(Vec<f32>),
    Buffered(Ring),
}

pub struct TapState {
    ring: Ring,
    ring_len: usize,
    capacity: usize,
    sink: Option<mpsc::Sender<LaneChunk>>,
}

impl TapState {
    fn push(&mut self, chunk: &[f32]) {
        let samples: Vec<i16> = chunk
            .iter()
            .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
            .collect();
        self.ring_len += samples.len();
        self.ring.push_back(samples);
        while self.ring_len > self.capacity {
            match self.ring.pop_front() {
                Some(front) => self.ring_len -= front.len(),
                None => break,
            }
        }
    }

    fn seconds(&self, format: AudioFormat) -> f64 {
        self.ring_len as f64 / (format.sample_rate as f64 * format.channels as f64)
    }

    pub fn detach(&mut self) {
        self.sink = None;
    }
}

pub type Tap = Arc<Mutex<TapState>>;

struct PrerollLane {
    source: Box<dyn CaptureSource>,
    tap: Tap,
}

impl PrerollLane {
    fn start(mut source: Box<dyn CaptureSource>) -> anyhow::Result<Self> {
        let format = source.format();
        let tap = Arc::new(Mutex::new(TapState {
            ring: VecDeque::new(),
            ring_len: 0,
            capacity: format.sample_rate as usize * format.channels as usize * PREROLL_SECONDS,
            sink: None,
        }));
        let cb_tap = tap.clone();
        source.start(Arc::new(move |chunk: &[f32]| {
            let mut state = cb_tap.lock().unwrap();
            match &state.sink {
                Some(tx) => {
                    let _ = tx.send(LaneChunk::Samples(chunk.to_vec()));
                }
                None => state.push(chunk),
            }
        }))?;
        Ok(Self { source, tap })
    }
}

pub struct Preroll {
    pub mic_device_id: Option<String>,
    sys: Option<PrerollLane>,
    mic: Option<PrerollLane>,
}

impl Preroll {
    pub fn start(mic_device_id: Option<&str>) -> anyhow::Result<Self> {
        let sys = new_system_capture(true)
            .and_then(PrerollLane::start)
            .map_err(|e| log::warn!("preroll system capture unavailable: {e:#}"))
            .ok();
        let mic = new_mic_capture(mic_device_id, DEFAULT_MIC_GAIN)
            .and_then(PrerollLane::start)
            .map_err(|e| log::warn!("preroll mic capture unavailable: {e:#}"))
            .ok();
        if sys.is_none() && mic.is_none() {
            anyhow::bail!("no capture source available for preroll");
        }
        Ok(Self {
            mic_device_id: mic_device_id.map(String::from),
            sys,
            mic,
        })
    }

    pub fn sys_tap(&self) -> Option<(Tap, AudioFormat)> {
        self.sys.as_ref().map(|l| (l.tap.clone(), l.source.format()))
    }

    pub fn mic_tap(&self) -> Option<(Tap, AudioFormat)> {
        self.mic.as_ref().map(|l| (l.tap.clone(), l.source.format()))
    }

    pub fn buffered_seconds(&self) -> f64 {
        [self.sys_tap(), self.mic_tap()]
            .into_iter()
            .flatten()
            .map(|(tap, format)| tap.lock().unwrap().seconds(format))
            .fold(0.0, f64::max)
    }
}

impl Drop for Preroll {
    fn drop(&mut self) {
        for lane in [self.sys.as_mut(), self.mic.as_mut()].into_iter().flatten() {
            lane.source.stop();
        }
    }
}

pub fn silence(format: AudioFormat, seconds: f64) -> Ring {
    let frames = (seconds * format.sample_rate as f64).round() as usize;
    let mut ring = Ring::new();
    if frames > 0 {
        ring.push_back(vec![0; frames * format.channels as usize]);
    }
    ring
}

// 全レーンのロックを同時に握ってリングを取り出すことで、sys/mic の末尾（=録音開始時刻）を揃える
pub fn attach(lanes: &[(Tap, AudioFormat, mpsc::Sender<LaneChunk>)]) -> f64 {
    let mut guards: Vec<MutexGuard<TapState>> =
        lanes.iter().map(|(tap, _, _)| tap.lock().unwrap()).collect();
    let buffered: Vec<f64> = guards
        .iter()
        .zip(lanes)
        .map(|(state, (_, format, _))| state.seconds(*format))
        .collect();
    let total = buffered.iter().cloned().fold(0.0, f64::max);
    for ((state, (_, format, tx)), own) in guards.iter_mut().zip(lanes).zip(&buffered) {
        let mut ring = std::mem::take(&mut state.ring);
        state.ring_len = 0;
        for pad in silence(*format, total - own) {
            ring.push_front(pad);
        }
        let _ = tx.send(LaneChunk::Buffered(ring));
        state.sink = Some(tx.clone());
    }
    total
}

pub fn sync(app: &AppHandle) {
    let state = app.state::<crate::state::AppState>();
    let recorder = state.recorder.lock().unwrap();
    if recorder.is_some() {
        return;
    }
    let (enabled, mic_device) = {
        let settings = state.settings.read().unwrap();
        (settings.preroll_enabled, settings.effective_mic_device_id())
    };
    let mut preroll = state.preroll.lock().unwrap();
    if !enabled {
        *preroll = None;
        return;
    }
    if preroll.as_ref().is_some_and(|p| p.mic_device_id == mic_device) {
        return;
    }
    *preroll = None;
    match Preroll::start(mic_device.as_deref()) {
        Ok(p) => *preroll = Some(p),
        Err(e) => log::warn!("preroll not started: {e:#}"),
    }
}
