use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn main() {
    let audio = std::env::args().nth(1).expect("usage: diarize_test <wav>");
    let dir = std::env::temp_dir().join("fennec_diar_models");
    let models = fennec_stt::download_diarization_models(
        &dir,
        &mut |f| eprint!("\rdownload {:>3.0}%", f * 100.0),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("model download failed");
    eprintln!();
    let segments =
        fennec_stt::diarize_file(Path::new(&audio), &models, None).expect("diarize failed");
    for s in &segments {
        println!("speaker{} {:.2}s - {:.2}s", s.speaker_id, s.start, s.end);
    }
    let speakers: std::collections::HashSet<i32> =
        segments.iter().map(|s| s.speaker_id).collect();
    println!("speakers detected: {}", speakers.len());
}
