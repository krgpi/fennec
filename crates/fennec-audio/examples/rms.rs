fn main() {
    let path = std::env::args().nth(1).expect("usage: rms <audio-file>");
    let samples = fennec_audio::decode_to_mono_16k(std::path::Path::new(&path)).unwrap();
    let rms = fennec_audio::rms(&samples);
    let peak = samples.iter().fold(0.0f32, |a, &s| a.max(s.abs()));
    println!(
        "{}: {:.2}s rms={:.6} peak={:.6}",
        path,
        samples.len() as f64 / 16000.0,
        rms,
        peak
    );
}
