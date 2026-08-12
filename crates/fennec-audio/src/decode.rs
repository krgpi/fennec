use anyhow::Context;
use std::path::Path;

pub const WHISPER_SAMPLE_RATE: u32 = 16000;

pub fn read_wav(path: &Path) -> anyhow::Result<(Vec<f32>, u32, u16)> {
    let mut reader = hound::WavReader::open(path)
        .with_context(|| format!("failed to open wav: {}", path.display()))?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().filter_map(Result::ok).collect(),
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .filter_map(Result::ok)
                .map(|s| s as f32 / max)
                .collect()
        }
    };
    Ok((samples, spec.sample_rate, spec.channels))
}

pub fn mixdown_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }
    let ch = channels as usize;
    samples
        .chunks_exact(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

pub fn resample_mono(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || samples.is_empty() {
        return samples.to_vec();
    }
    if from_rate % to_rate == 0 {
        let factor = (from_rate / to_rate) as usize;
        return samples
            .chunks(factor)
            .map(|c| c.iter().sum::<f32>() / c.len() as f32)
            .collect();
    }
    let ratio = from_rate as f64 / to_rate as f64;
    let out_len = (samples.len() as f64 / ratio).floor() as usize;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let idx = pos as usize;
            let frac = (pos - idx as f64) as f32;
            let a = samples[idx];
            let b = samples.get(idx + 1).copied().unwrap_or(a);
            a + (b - a) * frac
        })
        .collect()
}

pub fn read_audio(path: &Path) -> anyhow::Result<(Vec<f32>, u32, u16)> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("ogg") | Some("opus") => crate::opus::decode_ogg_opus(path),
        Some("m4a") | Some("mp4") | Some("aac") => read_symphonia(path),
        _ => read_wav(path),
    }
}

fn read_symphonia(path: &Path) -> anyhow::Result<(Vec<f32>, u32, u16)> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::errors::Error;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = std::fs::File::open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .with_context(|| format!("unsupported audio container: {}", path.display()))?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| anyhow::anyhow!("no audio track in {}", path.display()))?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .with_context(|| format!("unsupported codec in {}", path.display()))?;

    let mut samples: Vec<f32> = Vec::new();
    let mut rate = 0u32;
    let mut channels = 0u16;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(Error::ResetRequired) => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                rate = spec.rate;
                channels = spec.channels.count() as u16;
                let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                buf.copy_interleaved_ref(decoded);
                samples.extend_from_slice(buf.samples());
            }
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        }
    }

    if rate == 0 || channels == 0 {
        anyhow::bail!("no decodable audio in {}", path.display());
    }
    Ok((samples, rate, channels))
}

pub fn decode_to_mono_16k(path: &Path) -> anyhow::Result<Vec<f32>> {
    let (samples, rate, channels) = read_audio(path)?;
    let mono = mixdown_mono(&samples, channels);
    Ok(resample_mono(&mono, rate, WHISPER_SAMPLE_RATE))
}

pub struct StreamDownmixResampler {
    in_rate: u32,
    out_rate: u32,
    channels: usize,
    mono_carry: Vec<f32>,
    frame_carry: Vec<f32>,
    pos: f64,
}

impl StreamDownmixResampler {
    pub fn new(in_rate: u32, channels: u16, out_rate: u32) -> Self {
        Self {
            in_rate,
            out_rate,
            channels: channels.max(1) as usize,
            mono_carry: Vec::new(),
            frame_carry: Vec::new(),
            pos: 0.0,
        }
    }

    pub fn push(&mut self, interleaved: &[f32]) -> Vec<f32> {
        self.frame_carry.extend_from_slice(interleaved);
        let complete = self.frame_carry.len() / self.channels * self.channels;
        for frame in self.frame_carry[..complete].chunks_exact(self.channels) {
            self.mono_carry
                .push(frame.iter().sum::<f32>() / self.channels as f32);
        }
        self.frame_carry.drain(..complete);

        if self.in_rate == self.out_rate {
            return std::mem::take(&mut self.mono_carry);
        }

        let ratio = self.in_rate as f64 / self.out_rate as f64;
        let mut out = Vec::new();
        while (self.pos as usize) + 1 < self.mono_carry.len() {
            let idx = self.pos as usize;
            let frac = (self.pos - idx as f64) as f32;
            let a = self.mono_carry[idx];
            let b = self.mono_carry[idx + 1];
            out.push(a + (b - a) * frac);
            self.pos += ratio;
        }
        let consumed = (self.pos as usize).min(self.mono_carry.len());
        self.mono_carry.drain(..consumed);
        self.pos -= consumed as f64;
        out
    }
}

pub fn is_silent_audio(path: &Path, threshold: f32) -> anyhow::Result<bool> {
    let (samples, _, channels) = read_audio(path)?;
    if samples.is_empty() {
        return Ok(true);
    }
    let ch = channels.max(1) as usize;
    let mut max_rms = 0.0f32;
    for c in 0..ch {
        let channel: Vec<f32> = samples.iter().skip(c).step_by(ch).copied().collect();
        max_rms = max_rms.max(crate::level::rms(&channel));
    }
    Ok(max_rms < threshold)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixdown_averages_channels() {
        let stereo = [1.0f32, 0.0, 0.5, 0.5];
        assert_eq!(mixdown_mono(&stereo, 2), vec![0.5, 0.5]);
    }

    #[test]
    fn resample_48k_to_16k_uses_box_filter() {
        let samples = [3.0f32, 3.0, 3.0, 6.0, 6.0, 6.0];
        assert_eq!(resample_mono(&samples, 48000, 16000), vec![3.0, 6.0]);
    }

    #[test]
    fn resample_non_integer_ratio() {
        let samples: Vec<f32> = (0..441).map(|i| i as f32).collect();
        let out = resample_mono(&samples, 44100, 16000);
        assert_eq!(out.len(), 160);
        assert!(out.windows(2).all(|w| w[1] >= w[0]));
    }

    #[test]
    fn stream_resampler_output_length_over_time() {
        let mut r = StreamDownmixResampler::new(48000, 2, 16000);
        let mut total = 0usize;
        for _ in 0..100 {
            let chunk = vec![0.5f32; 2 * 480];
            total += r.push(&chunk).len();
        }
        let expected = 48000 / 3;
        assert!((total as i64 - expected as i64).abs() < 16, "total={total}");
    }

    #[test]
    fn stream_resampler_handles_partial_frames() {
        let mut r = StreamDownmixResampler::new(16000, 2, 16000);
        let out1 = r.push(&[1.0, 1.0, 0.0]);
        let out2 = r.push(&[0.0]);
        assert_eq!(out1, vec![1.0]);
        assert_eq!(out2, vec![0.0]);
    }
}
