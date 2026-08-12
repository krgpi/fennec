use crate::decode::{read_audio, resample_mono};
use anyhow::{anyhow, Context};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;

pub const MAX_TRACKS: usize = 2;

struct TrackData {
    frames: Vec<[f32; 2]>,
}

struct Shared {
    playing: AtomicBool,
    pos_frame: AtomicU64,
    rate_bits: AtomicU32,
    muted: [AtomicBool; MAX_TRACKS],
    finished: AtomicBool,
}

impl Shared {
    fn rate(&self) -> f32 {
        f32::from_bits(self.rate_bits.load(Ordering::Relaxed)).clamp(0.5, 3.0)
    }
}

pub struct Player {
    shared: Arc<Shared>,
    out_rate: u32,
    total_frames: u64,
    stop_tx: mpsc::Sender<()>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerStatus {
    pub position: f64,
    pub duration: f64,
    pub playing: bool,
    pub finished: bool,
}

impl Player {
    pub fn load(paths: &[Option<&Path>]) -> anyhow::Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow!("no output device"))?;
        let config = device
            .default_output_config()
            .context("no default output config")?;
        let out_rate = config.sample_rate().0;

        let mut tracks: Vec<Option<TrackData>> = Vec::with_capacity(MAX_TRACKS);
        for path in paths.iter().take(MAX_TRACKS) {
            match path {
                Some(path) => {
                    let (samples, rate, channels) = read_audio(path)?;
                    tracks.push(Some(TrackData {
                        frames: to_stereo_frames(&samples, channels, rate, out_rate),
                    }));
                }
                None => tracks.push(None),
            }
        }
        while tracks.len() < MAX_TRACKS {
            tracks.push(None);
        }

        let total_frames = tracks
            .iter()
            .flatten()
            .map(|t| t.frames.len() as u64)
            .max()
            .unwrap_or(0);
        if total_frames == 0 {
            anyhow::bail!("no audio tracks to play");
        }

        let shared = Arc::new(Shared {
            playing: AtomicBool::new(false),
            pos_frame: AtomicU64::new(0),
            rate_bits: AtomicU32::new(1.0f32.to_bits()),
            muted: [AtomicBool::new(false), AtomicBool::new(false)],
            finished: AtomicBool::new(false),
        });

        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<anyhow::Result<()>>();
        let thread_shared = shared.clone();
        let tracks: Arc<Vec<Option<TrackData>>> = Arc::new(tracks);
        let device_name = device.name().ok();

        let thread = std::thread::Builder::new()
            .name("fennec-player".into())
            .spawn(move || {
                let result = run_output_stream(
                    device_name.as_deref(),
                    thread_shared,
                    tracks,
                    total_frames,
                    stop_rx,
                );
                if let Err(e) = result {
                    let _ = ready_tx.send(Err(e));
                } else {
                    let _ = ready_tx.send(Ok(()));
                }
            })?;

        // run_output_stream sends Ok only after building the stream; ready_rx
        // therefore reports build errors before we return the Player
        match ready_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(anyhow!("player thread terminated unexpectedly")),
        }

        Ok(Self {
            shared,
            out_rate,
            total_frames,
            stop_tx,
            thread: Some(thread),
        })
    }

    pub fn play(&self) {
        self.shared.finished.store(false, Ordering::Relaxed);
        if self.shared.pos_frame.load(Ordering::Relaxed) >= self.total_frames {
            self.shared.pos_frame.store(0, Ordering::Relaxed);
        }
        self.shared.playing.store(true, Ordering::Relaxed);
    }

    pub fn pause(&self) {
        self.shared.playing.store(false, Ordering::Relaxed);
    }

    pub fn seek(&self, seconds: f64) {
        let frame = ((seconds.max(0.0) * self.out_rate as f64) as u64).min(self.total_frames);
        self.shared.pos_frame.store(frame, Ordering::Relaxed);
        self.shared.finished.store(false, Ordering::Relaxed);
    }

    pub fn set_rate(&self, rate: f32) {
        self.shared
            .rate_bits
            .store(rate.clamp(0.5, 3.0).to_bits(), Ordering::Relaxed);
    }

    pub fn set_muted(&self, track: usize, muted: bool) {
        if track < MAX_TRACKS {
            self.shared.muted[track].store(muted, Ordering::Relaxed);
        }
    }

    pub fn status(&self) -> PlayerStatus {
        let pos = self.shared.pos_frame.load(Ordering::Relaxed);
        PlayerStatus {
            position: pos as f64 / self.out_rate as f64,
            duration: self.total_frames as f64 / self.out_rate as f64,
            playing: self.shared.playing.load(Ordering::Relaxed),
            finished: self.shared.finished.load(Ordering::Relaxed),
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run_output_stream(
    device_name: Option<&str>,
    shared: Arc<Shared>,
    tracks: Arc<Vec<Option<TrackData>>>,
    total_frames: u64,
    stop_rx: mpsc::Receiver<()>,
) -> anyhow::Result<()> {
    let host = cpal::default_host();
    let device = match device_name {
        Some(name) => host
            .output_devices()?
            .find(|d| d.name().map(|n| n == name).unwrap_or(false))
            .or_else(|| host.default_output_device()),
        None => host.default_output_device(),
    }
    .ok_or_else(|| anyhow!("no output device"))?;

    let supported = device.default_output_config()?;
    let channels = supported.channels().max(1) as usize;
    let config: cpal::StreamConfig = supported.into();

    let cb_shared = shared.clone();
    let stream = device.build_output_stream(
        &config,
        move |data: &mut [f32], _| {
            let frames = data.len() / channels;
            if !cb_shared.playing.load(Ordering::Relaxed) {
                data.fill(0.0);
                return;
            }
            let rate = cb_shared.rate() as f64;
            let mut pos = cb_shared.pos_frame.load(Ordering::Relaxed) as f64;

            for frame_out in data.chunks_mut(channels) {
                let idx = pos as u64;
                let mut left = 0.0f32;
                let mut right = 0.0f32;
                if idx < total_frames {
                    for (t, track) in tracks.iter().enumerate() {
                        let Some(track) = track else { continue };
                        if cb_shared.muted[t].load(Ordering::Relaxed) {
                            continue;
                        }
                        if let Some(frame) = track.frames.get(idx as usize) {
                            left += frame[0];
                            right += frame[1];
                        }
                    }
                }
                left = left.clamp(-1.0, 1.0);
                right = right.clamp(-1.0, 1.0);
                for (c, sample) in frame_out.iter_mut().enumerate() {
                    *sample = if c == 0 { left } else { right };
                }
                pos += rate;
            }

            let new_pos = pos as u64;
            if new_pos >= total_frames {
                cb_shared.pos_frame.store(total_frames, Ordering::Relaxed);
                cb_shared.playing.store(false, Ordering::Relaxed);
                cb_shared.finished.store(true, Ordering::Relaxed);
            } else {
                cb_shared.pos_frame.store(new_pos, Ordering::Relaxed);
            }
        },
        |e| eprintln!("player stream error: {e}"),
        None,
    )?;
    stream.play()?;
    let _ = stop_rx.recv();
    drop(stream);
    Ok(())
}

fn to_stereo_frames(samples: &[f32], channels: u16, rate: u32, out_rate: u32) -> Vec<[f32; 2]> {
    let ch = channels.max(1) as usize;
    let (left, right): (Vec<f32>, Vec<f32>) = if ch == 1 {
        (samples.to_vec(), samples.to_vec())
    } else {
        let left: Vec<f32> = samples.iter().step_by(ch).copied().collect();
        let right: Vec<f32> = samples.iter().skip(1).step_by(ch).copied().collect();
        (left, right)
    };
    let left = resample_mono(&left, rate, out_rate);
    let right = resample_mono(&right, rate, out_rate);
    let len = left.len().min(right.len());
    (0..len).map(|i| [left[i], right[i]]).collect()
}
