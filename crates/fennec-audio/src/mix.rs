use crate::decode::{mixdown_mono, read_audio, resample_mono};
use crate::opus::encode_pcm_to_opus;
use crate::traits::AudioFormat;
use crate::wav::WavFileWriter;
use anyhow::Context;
use std::path::Path;

const EXPORT_RATE: u32 = 48000;
const EXPORT_BITRATE: i32 = 64000;

pub fn mix_tracks(tracks: &[Vec<f32>]) -> Vec<f32> {
    let len = tracks.iter().map(|t| t.len()).max().unwrap_or(0);
    let mut out = vec![0.0f32; len];
    for track in tracks {
        for (o, s) in out.iter_mut().zip(track) {
            *o += s;
        }
    }
    // 加算でピークが 1.0 を超えると 16bit 化やエンコードで歪むため、超えた分だけ全体を下げる
    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if peak > 1.0 {
        for s in &mut out {
            *s /= peak;
        }
    }
    out
}

pub fn mix_files_to_mono(paths: &[&Path]) -> anyhow::Result<Vec<f32>> {
    let mut tracks = Vec::with_capacity(paths.len());
    for path in paths {
        let (samples, rate, channels) = read_audio(path)?;
        let mono = mixdown_mono(&samples, channels);
        tracks.push(resample_mono(&mono, rate, EXPORT_RATE));
    }
    Ok(mix_tracks(&tracks))
}

/// PC音声とマイクを1本のモノラル音声に結合して書き出す。拡張子が ogg/opus なら Opus、それ以外は WAV
pub fn export_mixed_audio(paths: &[&Path], out: &Path) -> anyhow::Result<()> {
    if paths.is_empty() {
        anyhow::bail!("no audio files to export");
    }
    let mixed = mix_files_to_mono(paths)?;
    let ext = out
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "ogg" | "opus" => encode_pcm_to_opus(&mixed, EXPORT_RATE, 1, out, EXPORT_BITRATE)
            .with_context(|| format!("failed to write {}", out.display())),
        _ => {
            let format = AudioFormat {
                sample_rate: EXPORT_RATE,
                channels: 1,
            };
            let mut writer = WavFileWriter::create(out, format)?;
            writer.write(&mixed)?;
            writer.finalize()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::read_wav;

    fn write_wav(path: &Path, samples: &[f32], rate: u32, channels: u16) {
        let format = AudioFormat {
            sample_rate: rate,
            channels,
        };
        let mut writer = WavFileWriter::create(path, format).unwrap();
        writer.write(samples).unwrap();
        writer.finalize().unwrap();
    }

    #[test]
    fn mix_pads_to_longest_track() {
        let mixed = mix_tracks(&[vec![0.1, 0.1, 0.1], vec![0.2]]);
        assert_eq!(mixed.len(), 3);
        assert!((mixed[0] - 0.3).abs() < 1e-6);
        assert!((mixed[1] - 0.1).abs() < 1e-6);
    }

    #[test]
    fn mix_scales_down_instead_of_clipping() {
        let mixed = mix_tracks(&[vec![0.8, -0.8], vec![0.8, -0.8]]);
        assert!((mixed[0] - 1.0).abs() < 1e-6);
        assert!((mixed[1] + 1.0).abs() < 1e-6);
    }

    #[test]
    fn exports_wav_with_both_sources_merged() {
        let dir = tempfile::tempdir().unwrap();
        let sys = dir.path().join("system.wav");
        let mic = dir.path().join("mic.wav");
        let out = dir.path().join("merged.wav");
        write_wav(&sys, &vec![0.25f32; 48000 * 2], 48000, 1);
        write_wav(&mic, &vec![0.25f32; 24000], 24000, 1);

        export_mixed_audio(&[sys.as_path(), mic.as_path()], &out).unwrap();

        let (samples, rate, channels) = read_wav(&out).unwrap();
        assert_eq!(rate, 48000);
        assert_eq!(channels, 1);
        assert_eq!(samples.len(), 48000 * 2);
        assert!((samples[100] - 0.5).abs() < 0.01, "{}", samples[100]);
        assert!((samples[96000 - 100] - 0.25).abs() < 0.01);
    }

    #[test]
    fn exports_opus_when_target_is_ogg() {
        let dir = tempfile::tempdir().unwrap();
        let mic = dir.path().join("mic.wav");
        let out = dir.path().join("merged.ogg");
        let samples: Vec<f32> = (0..48000)
            .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 48000.0).sin() * 0.5)
            .collect();
        write_wav(&mic, &samples, 48000, 1);

        export_mixed_audio(&[mic.as_path()], &out).unwrap();

        let (decoded, rate, channels) = crate::opus::decode_ogg_opus(&out).unwrap();
        assert_eq!(rate, 48000);
        assert_eq!(channels, 1);
        assert!(decoded.len() as f64 / 48000.0 > 0.95);
    }
}
