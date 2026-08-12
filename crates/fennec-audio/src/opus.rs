use crate::decode::{read_wav, resample_mono};
use anyhow::Context;
use audiopus::coder::{Decoder, Encoder};
use audiopus::{Application, Bitrate, Channels, SampleRate};
use ogg::{PacketReader, PacketWriteEndInfo, PacketWriter};
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

const OPUS_RATE: u32 = 48000;
const FRAME_SAMPLES: usize = 960;
const PRE_SKIP: u16 = 312;

pub fn encode_wav_to_opus(wav: &Path, out_ogg: &Path, bitrate_bps: i32) -> anyhow::Result<()> {
    let (samples, rate, channels) = read_wav(wav)?;
    let channels = channels.clamp(1, 2);
    let interleaved = if rate == OPUS_RATE {
        samples
    } else {
        resample_interleaved(&samples, channels, rate, OPUS_RATE)
    };

    let opus_channels = if channels == 1 {
        Channels::Mono
    } else {
        Channels::Stereo
    };
    let mut encoder = Encoder::new(SampleRate::Hz48000, opus_channels, Application::Voip)
        .context("failed to create opus encoder")?;
    encoder
        .set_bitrate(Bitrate::BitsPerSecond(bitrate_bps))
        .context("failed to set opus bitrate")?;

    let file = BufWriter::new(File::create(out_ogg)?);
    let mut writer = PacketWriter::new(file);
    let serial: u32 = 0x46_45_4e_43;

    let mut head = Vec::with_capacity(19);
    head.extend_from_slice(b"OpusHead");
    head.push(1);
    head.push(channels as u8);
    head.extend_from_slice(&PRE_SKIP.to_le_bytes());
    head.extend_from_slice(&rate.to_le_bytes());
    head.extend_from_slice(&0i16.to_le_bytes());
    head.push(0);
    writer.write_packet(head, serial, PacketWriteEndInfo::EndPage, 0)?;

    let vendor = b"fennec";
    let mut tags = Vec::new();
    tags.extend_from_slice(b"OpusTags");
    tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    tags.extend_from_slice(vendor);
    tags.extend_from_slice(&0u32.to_le_bytes());
    writer.write_packet(tags, serial, PacketWriteEndInfo::EndPage, 0)?;

    let frame_len = FRAME_SAMPLES * channels as usize;
    let mut out_buf = vec![0u8; 4096];
    let mut granule: u64 = PRE_SKIP as u64;
    let total_frames = interleaved.len().div_ceil(frame_len);

    for (idx, chunk) in interleaved.chunks(frame_len).enumerate() {
        let frame: Vec<f32>;
        let input: &[f32] = if chunk.len() == frame_len {
            chunk
        } else {
            let mut padded = chunk.to_vec();
            padded.resize(frame_len, 0.0);
            frame = padded;
            &frame
        };
        let written = encoder
            .encode_float(input, &mut out_buf)
            .context("opus encode failed")?;
        granule += FRAME_SAMPLES as u64;
        let end = if idx + 1 == total_frames {
            PacketWriteEndInfo::EndStream
        } else {
            PacketWriteEndInfo::NormalPacket
        };
        writer.write_packet(out_buf[..written].to_vec(), serial, end, granule)?;
    }

    Ok(())
}

pub fn decode_ogg_opus(path: &Path) -> anyhow::Result<(Vec<f32>, u32, u16)> {
    let file = BufReader::new(
        File::open(path).with_context(|| format!("failed to open {}", path.display()))?,
    );
    let mut reader = PacketReader::new(file);

    let head = reader
        .read_packet_expected()
        .context("missing OpusHead packet")?;
    if head.data.len() < 19 || &head.data[..8] != b"OpusHead" {
        anyhow::bail!("not an opus stream: {}", path.display());
    }
    let channels = head.data[9].clamp(1, 2) as u16;
    let pre_skip = u16::from_le_bytes([head.data[10], head.data[11]]) as usize;

    let _tags = reader.read_packet_expected().context("missing OpusTags")?;

    let opus_channels = if channels == 1 {
        Channels::Mono
    } else {
        Channels::Stereo
    };
    let mut decoder =
        Decoder::new(SampleRate::Hz48000, opus_channels).context("failed to create decoder")?;

    let mut samples: Vec<f32> = Vec::new();
    let mut frame_buf = vec![0f32; 5760 * channels as usize];
    while let Some(packet) = reader.read_packet()? {
        let decoded = decoder
            .decode_float(Some(&packet.data), &mut frame_buf, false)
            .context("opus decode failed")?;
        samples.extend_from_slice(&frame_buf[..decoded * channels as usize]);
    }

    let skip = pre_skip * channels as usize;
    if skip < samples.len() {
        samples.drain(..skip);
    } else {
        samples.clear();
    }
    Ok((samples, OPUS_RATE, channels))
}

fn resample_interleaved(samples: &[f32], channels: u16, from: u32, to: u32) -> Vec<f32> {
    if channels <= 1 {
        return resample_mono(samples, from, to);
    }
    let ch = channels as usize;
    let mut planes: Vec<Vec<f32>> = (0..ch)
        .map(|c| samples.iter().skip(c).step_by(ch).copied().collect())
        .collect();
    for plane in &mut planes {
        *plane = resample_mono(plane, from, to);
    }
    let len = planes.iter().map(|p| p.len()).min().unwrap_or(0);
    let mut out = Vec::with_capacity(len * ch);
    for i in 0..len {
        for plane in &planes {
            out.push(plane[i]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::AudioFormat;
    use crate::wav::WavFileWriter;

    #[test]
    fn encode_decode_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let wav_path = dir.path().join("in.wav");
        let ogg_path = dir.path().join("out.ogg");

        let format = AudioFormat { sample_rate: 48000, channels: 1 };
        let mut writer = WavFileWriter::create(&wav_path, format).unwrap();
        let samples: Vec<f32> = (0..48000)
            .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 48000.0).sin() * 0.5)
            .collect();
        writer.write(&samples).unwrap();
        writer.finalize().unwrap();

        encode_wav_to_opus(&wav_path, &ogg_path, 24000).unwrap();
        assert!(ogg_path.exists());
        assert!(std::fs::metadata(&ogg_path).unwrap().len() > 1000);

        let (decoded, rate, channels) = decode_ogg_opus(&ogg_path).unwrap();
        assert_eq!(rate, 48000);
        assert_eq!(channels, 1);
        let duration_error = (decoded.len() as f64 - 48000.0).abs() / 48000.0;
        assert!(duration_error < 0.05, "duration off by {duration_error}");

        let rms: f32 =
            (decoded.iter().map(|s| s * s).sum::<f32>() / decoded.len() as f32).sqrt();
        assert!(rms > 0.2, "decoded rms too low: {rms}");
    }
}
