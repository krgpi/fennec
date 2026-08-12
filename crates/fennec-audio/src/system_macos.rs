use std::sync::Arc;

use anyhow::{bail, Context, Result};
use cidre::core_audio::hardware::sub_tap_keys as tap_keys;
use cidre::core_audio::{aggregate_device_keys as agg_keys, sub_device_keys as sub_keys};
use cidre::{cat, cf, core_audio as ca, ns, os};

const SCRATCH_CAPACITY: usize = 32 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFormatInfo {
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SampleKind {
    F32,
    I16,
    I32,
}

struct IoCtx {
    on_chunk: Arc<dyn Fn(&[f32]) + Send + Sync>,
    channels: usize,
    interleaved: bool,
    kind: SampleKind,
    scratch: Vec<f32>,
}

enum DeviceState {
    Idle(ca::AggregateDevice),
    Running(ca::hardware::StartedDevice<ca::AggregateDevice>),
    Lost,
}

// Field order matters: `state` must drop before `_tap` so the aggregate device
// is destroyed before the process tap it references.
pub struct SystemAudioCapture {
    state: DeviceState,
    _tap: ca::TapGuard,
    output_uid: String,
    tap_uid: String,
    asbd: cat::AudioStreamBasicDesc,
    kind: SampleKind,
    proc_id: Option<ca::DeviceIoProcId>,
    ctx: Option<Box<IoCtx>>,
}

impl SystemAudioCapture {
    pub fn new(exclude_current_process: bool) -> Result<Self> {
        let output_device = ca::System::default_output_device()
            .context("failed to get default output device")?;
        let output_uid = output_device
            .uid()
            .context("failed to get default output device UID")?;

        let excluded = if exclude_current_process {
            ca::Process::with_pid(std::process::id() as i32)
                .ok()
                .filter(|p| p.0 .0 != 0)
                .map(|p| ns::Number::with_u32(p.0 .0))
        } else {
            None
        };
        let excluded_refs: Vec<&ns::Number> = excluded.iter().map(|n| n.as_ref()).collect();
        let mut tap_desc =
            ca::TapDesc::with_stereo_global_tap_excluding_processes(&ns::Array::from_slice(
                &excluded_refs,
            ));
        tap_desc.set_private(true);

        let tap = tap_desc.create_process_tap().context(
            "AudioHardwareCreateProcessTap failed \
             (requires macOS 14.4+ and system audio recording permission)",
        )?;
        let tap_uid = tap.uid().context("failed to get process tap UID")?;
        let asbd = tap.asbd().context("failed to get process tap format")?;
        let kind = sample_kind(&asbd)?;

        let agg = create_aggregate_device(&output_uid, &tap_uid)?;

        Ok(Self {
            state: DeviceState::Idle(agg),
            _tap: tap,
            output_uid: output_uid.to_string(),
            tap_uid: tap_uid.to_string(),
            asbd,
            kind,
            proc_id: None,
            ctx: None,
        })
    }

    pub fn format(&self) -> AudioFormatInfo {
        AudioFormatInfo {
            sample_rate: self.asbd.sample_rate as u32,
            channels: self.asbd.channels_per_frame as u16,
        }
    }

    pub fn start(&mut self, on_chunk: Arc<dyn Fn(&[f32]) + Send + Sync>) -> Result<()> {
        let agg = match std::mem::replace(&mut self.state, DeviceState::Lost) {
            DeviceState::Idle(agg) => agg,
            running @ DeviceState::Running(_) => {
                self.state = running;
                bail!("system audio capture is already started");
            }
            DeviceState::Lost => {
                bail!("system audio capture device is unavailable after a previous failure")
            }
        };

        let mut ctx = Box::new(IoCtx {
            on_chunk,
            channels: self.asbd.channels_per_frame.max(1) as usize,
            interleaved: self.asbd.is_interleaved(),
            kind: self.kind,
            scratch: Vec::with_capacity(SCRATCH_CAPACITY),
        });

        let proc_id = match agg.create_io_proc_id(io_proc, Some(ctx.as_mut())) {
            Ok(id) => id,
            Err(e) => {
                self.state = DeviceState::Idle(agg);
                return Err(anyhow::Error::new(e).context("AudioDeviceCreateIOProcID failed"));
            }
        };

        match ca::device_start(agg, Some(proc_id)) {
            Ok(started) => {
                self.state = DeviceState::Running(started);
                self.proc_id = Some(proc_id);
                self.ctx = Some(ctx);
                Ok(())
            }
            Err(e) => {
                // device_start consumes and destroys the aggregate device on error
                self.rebuild_device();
                Err(anyhow::Error::new(e).context("AudioDeviceStart failed"))
            }
        }
    }

    pub fn stop(&mut self) {
        match std::mem::replace(&mut self.state, DeviceState::Lost) {
            DeviceState::Running(started) => match started.stop() {
                Ok(agg) => {
                    if let Some(proc_id) = self.proc_id.take() {
                        let dev: &ca::Device = &agg;
                        let res = unsafe { AudioDeviceDestroyIOProcID(*dev, proc_id) };
                        debug_assert!(res.is_ok(), "AudioDeviceDestroyIOProcID failed: {res:?}");
                    }
                    self.ctx = None;
                    self.state = DeviceState::Idle(agg);
                }
                Err(_) => {
                    self.rebuild_device();
                }
            },
            other => self.state = other,
        }
    }

    fn rebuild_device(&mut self) {
        self.proc_id = None;
        self.ctx = None;
        let output_uid = cf::String::from_str(&self.output_uid);
        let tap_uid = cf::String::from_str(&self.tap_uid);
        self.state = match create_aggregate_device(&output_uid, &tap_uid) {
            Ok(agg) => DeviceState::Idle(agg),
            Err(_) => DeviceState::Lost,
        };
    }
}

impl Drop for SystemAudioCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

fn create_aggregate_device(
    output_uid: &cf::String,
    tap_uid: &cf::String,
) -> Result<ca::AggregateDevice> {
    let sub_device =
        cf::DictionaryOf::with_keys_values(&[sub_keys::uid()], &[output_uid.as_type_ref()]);
    let sub_tap = cf::DictionaryOf::with_keys_values(
        &[tap_keys::uid(), tap_keys::drift_compensation()],
        &[tap_uid.as_type_ref(), cf::Boolean::value_true()],
    );
    let desc = cf::DictionaryOf::with_keys_values(
        &[
            agg_keys::is_private(),
            agg_keys::is_stacked(),
            agg_keys::tap_auto_start(),
            agg_keys::name(),
            agg_keys::main_sub_device(),
            agg_keys::uid(),
            agg_keys::sub_device_list(),
            agg_keys::tap_list(),
        ],
        &[
            cf::Boolean::value_true().as_type_ref(),
            cf::Boolean::value_false(),
            cf::Boolean::value_true(),
            cf::str!(c"Fennec System Audio Tap"),
            output_uid,
            &cf::Uuid::new().to_cf_string(),
            &cf::ArrayOf::from_slice(&[sub_device.as_ref()]),
            &cf::ArrayOf::from_slice(&[sub_tap.as_ref()]),
        ],
    );
    ca::AggregateDevice::with_desc(&desc).context("AudioHardwareCreateAggregateDevice failed")
}

fn sample_kind(asbd: &cat::AudioStreamBasicDesc) -> Result<SampleKind> {
    if asbd.format != cat::AudioFormat::LINEAR_PCM {
        bail!("unsupported tap stream format {:?} (expected linear PCM)", asbd.format);
    }
    if !asbd.is_native_endian() {
        bail!("unsupported tap sample endianness");
    }
    let flags = asbd.format_flags;
    if flags.contains(cat::AudioFormatFlags::IS_FLOAT) && asbd.bits_per_channel == 32 {
        Ok(SampleKind::F32)
    } else if flags.contains(cat::AudioFormatFlags::IS_SIGNED_INTEGER) && asbd.bits_per_channel == 16
    {
        Ok(SampleKind::I16)
    } else if flags.contains(cat::AudioFormatFlags::IS_SIGNED_INTEGER) && asbd.bits_per_channel == 32
    {
        Ok(SampleKind::I32)
    } else {
        bail!(
            "unsupported tap sample format ({} bits, flags {:?})",
            asbd.bits_per_channel,
            flags
        );
    }
}

extern "C" fn io_proc(
    _device: ca::Device,
    _now: &cat::AudioTimeStamp,
    input_data: &cat::AudioBufList<1>,
    _input_time: &cat::AudioTimeStamp,
    _output_data: &mut cat::AudioBufList<1>,
    _output_time: &cat::AudioTimeStamp,
    ctx: Option<&mut IoCtx>,
) -> os::Status {
    let Some(ctx) = ctx else {
        return os::Status::default();
    };
    // AudioBufferList is variable-length; number_buffers may exceed the static N=1
    let bufs = unsafe {
        let list: *const cat::AudioBufList<1> = input_data;
        std::slice::from_raw_parts(
            std::ptr::addr_of!((*list).buffers) as *const cat::AudioBuf,
            (*list).number_buffers as usize,
        )
    };
    if ctx.interleaved {
        // tap streams come after any sub-device input streams, so prefer the last match
        let buf = bufs
            .iter()
            .rev()
            .find(|b| b.number_channels as usize == ctx.channels)
            .or_else(|| bufs.first());
        if let Some(buf) = buf {
            emit_interleaved(ctx, buf);
        }
    } else {
        emit_planar(ctx, bufs);
    }
    os::Status::default()
}

fn emit_interleaved(ctx: &mut IoCtx, buf: &cat::AudioBuf) {
    if buf.data.is_null() || buf.data_bytes_size == 0 {
        return;
    }
    match ctx.kind {
        SampleKind::F32 => {
            let samples = unsafe {
                std::slice::from_raw_parts(buf.data as *const f32, buf.data_bytes_size as usize / 4)
            };
            (ctx.on_chunk)(samples);
        }
        SampleKind::I16 => {
            let src = unsafe {
                std::slice::from_raw_parts(buf.data as *const i16, buf.data_bytes_size as usize / 2)
            };
            ctx.scratch.clear();
            ctx.scratch.extend(src.iter().map(|&s| f32::from(s) / 32768.0));
            (ctx.on_chunk)(&ctx.scratch);
        }
        SampleKind::I32 => {
            let src = unsafe {
                std::slice::from_raw_parts(buf.data as *const i32, buf.data_bytes_size as usize / 4)
            };
            ctx.scratch.clear();
            ctx.scratch
                .extend(src.iter().map(|&s| s as f32 / 2_147_483_648.0));
            (ctx.on_chunk)(&ctx.scratch);
        }
    }
}

fn emit_planar(ctx: &mut IoCtx, bufs: &[cat::AudioBuf]) {
    let start = bufs.len().saturating_sub(ctx.channels);
    let chans = &bufs[start..];
    if chans.is_empty() || chans.iter().any(|b| b.data.is_null()) {
        return;
    }
    let bytes_per_sample = match ctx.kind {
        SampleKind::I16 => 2,
        _ => 4,
    };
    let frames = chans
        .iter()
        .map(|b| b.data_bytes_size as usize / bytes_per_sample)
        .min()
        .unwrap_or(0);
    if frames == 0 {
        return;
    }
    let nch = chans.len();
    ctx.scratch.clear();
    ctx.scratch.resize(frames * nch, 0.0);
    for (ci, b) in chans.iter().enumerate() {
        match ctx.kind {
            SampleKind::F32 => {
                let src = unsafe { std::slice::from_raw_parts(b.data as *const f32, frames) };
                for (fi, &s) in src.iter().enumerate() {
                    ctx.scratch[fi * nch + ci] = s;
                }
            }
            SampleKind::I16 => {
                let src = unsafe { std::slice::from_raw_parts(b.data as *const i16, frames) };
                for (fi, &s) in src.iter().enumerate() {
                    ctx.scratch[fi * nch + ci] = f32::from(s) / 32768.0;
                }
            }
            SampleKind::I32 => {
                let src = unsafe { std::slice::from_raw_parts(b.data as *const i32, frames) };
                for (fi, &s) in src.iter().enumerate() {
                    ctx.scratch[fi * nch + ci] = s as f32 / 2_147_483_648.0;
                }
            }
        }
    }
    (ctx.on_chunk)(&ctx.scratch);
}

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C-unwind" {
    fn AudioDeviceDestroyIOProcID(device: ca::Device, proc_id: ca::DeviceIoProcId) -> os::Status;
}
