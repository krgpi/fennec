#[cfg(any(target_os = "windows", target_os = "linux"))]
fn main() -> anyhow::Result<()> {
    use std::sync::{mpsc, Arc};
    use std::time::{Duration, Instant};

    #[cfg(target_os = "linux")]
    use fennec_audio::system_linux::SystemAudioCapture;
    #[cfg(target_os = "windows")]
    use fennec_audio::system_windows::SystemAudioCapture;

    let out_path = "loopback_test.wav";
    let mut capture = SystemAudioCapture::new(true)?;
    let format = capture.format();
    println!("loopback format: {} Hz, {} ch", format.sample_rate, format.channels);

    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    capture.start(Arc::new(move |chunk| {
        let _ = tx.send(chunk.to_vec());
    }))?;

    let spec = hound::WavSpec {
        channels: format.channels,
        sample_rate: format.sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(out_path, spec)?;

    println!("recording system audio for 5 seconds (play something now)...");
    let started = Instant::now();
    let mut total_samples = 0usize;
    let mut peak = 0f32;
    while started.elapsed() < Duration::from_secs(5) {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(chunk) => {
                total_samples += chunk.len();
                for &s in &chunk {
                    peak = peak.max(s.abs());
                    writer.write_sample(s)?;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    capture.stop();
    writer.finalize()?;

    let secs = total_samples as f64 / (format.sample_rate as f64 * format.channels as f64);
    println!("wrote {out_path}: {total_samples} samples ({secs:.2} s), peak {peak:.4}");
    if peak == 0.0 {
        println!("warning: captured only silence (was any audio playing?)");
    }
    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn main() {
    eprintln!("loopback_record only works on Windows and Linux");
}
