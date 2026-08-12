use crate::traits::AudioFormat;
use anyhow::Context;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

pub struct WavFileWriter {
    writer: Option<hound::WavWriter<BufWriter<File>>>,
    samples_since_flush: usize,
    flush_interval_samples: usize,
}

impl WavFileWriter {
    pub fn create(path: &Path, format: AudioFormat) -> anyhow::Result<Self> {
        let spec = hound::WavSpec {
            channels: format.channels,
            sample_rate: format.sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let writer = hound::WavWriter::create(path, spec)
            .with_context(|| format!("failed to create wav file: {}", path.display()))?;
        Ok(Self {
            writer: Some(writer),
            samples_since_flush: 0,
            flush_interval_samples: (format.sample_rate as usize) * (format.channels as usize),
        })
    }

    pub fn write(&mut self, samples: &[f32]) -> anyhow::Result<()> {
        let Some(writer) = self.writer.as_mut() else {
            return Ok(());
        };
        for &s in samples {
            let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            writer.write_sample(v)?;
        }
        self.samples_since_flush += samples.len();
        if self.samples_since_flush >= self.flush_interval_samples {
            writer.flush()?;
            self.samples_since_flush = 0;
        }
        Ok(())
    }

    pub fn finalize(&mut self) -> anyhow::Result<()> {
        if let Some(writer) = self.writer.take() {
            writer.finalize()?;
        }
        Ok(())
    }
}

impl Drop for WavFileWriter {
    fn drop(&mut self) {
        let _ = self.finalize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_valid_wav() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.wav");
        let format = AudioFormat { sample_rate: 48000, channels: 1 };
        let mut w = WavFileWriter::create(&path, format).unwrap();
        let samples: Vec<f32> = (0..4800).map(|i| (i as f32 / 100.0).sin() * 0.5).collect();
        w.write(&samples).unwrap();
        w.finalize().unwrap();

        let reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().sample_rate, 48000);
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.len(), 4800);
    }
}
