use anyhow::Context;
use fennec_core::models::WhisperModelInfo;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub fn download_model(
    model: &WhisperModelInfo,
    dest_dir: &Path,
    on_progress: &mut dyn FnMut(f64),
    cancelled: Arc<AtomicBool>,
) -> anyhow::Result<PathBuf> {
    fs::create_dir_all(dest_dir)?;
    let dest = dest_dir.join(model.file_name);
    download_file(model.url, &dest, on_progress, cancelled)?;
    Ok(dest)
}

pub fn download_file(
    url: &str,
    dest: &Path,
    on_progress: &mut dyn FnMut(f64),
    cancelled: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    if dest.exists() {
        return Ok(());
    }
    let part = dest.with_extension("part");

    let response = ureq::get(url)
        .call()
        .with_context(|| format!("failed to download {url}"))?;
    let total: Option<u64> = response
        .header("Content-Length")
        .and_then(|v| v.parse().ok());

    let mut reader = response.into_reader();
    let mut file = fs::File::create(&part)?;
    let mut buf = [0u8; 1024 * 256];
    let mut downloaded: u64 = 0;

    loop {
        if cancelled.load(Ordering::Relaxed) {
            drop(file);
            let _ = fs::remove_file(&part);
            anyhow::bail!("cancelled");
        }
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        downloaded += n as u64;
        if let Some(total) = total {
            if total > 0 {
                on_progress(downloaded as f64 / total as f64);
            }
        }
    }
    file.flush()?;
    drop(file);
    fs::rename(&part, dest)?;
    on_progress(1.0);
    Ok(())
}
