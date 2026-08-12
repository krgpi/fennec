use crate::download::download_file;
use anyhow::Context;
use fennec_core::types::SpeakerTimedSegment;
use sherpa_rs::diarize::{Diarize, DiarizeConfig};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

const SEGMENTATION_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2";
const EMBEDDING_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx";

const SEGMENTATION_FILE: &str = "pyannote-segmentation-3-0.onnx";
const EMBEDDING_FILE: &str = "3dspeaker-eres2net-base-16k.onnx";

#[derive(Debug, Clone)]
pub struct DiarizationModelPaths {
    pub segmentation: PathBuf,
    pub embedding: PathBuf,
}

pub fn diarization_models_at(dir: &Path) -> Option<DiarizationModelPaths> {
    let paths = DiarizationModelPaths {
        segmentation: dir.join(SEGMENTATION_FILE),
        embedding: dir.join(EMBEDDING_FILE),
    };
    (paths.segmentation.exists() && paths.embedding.exists()).then_some(paths)
}

pub fn delete_diarization_models(dir: &Path) -> std::io::Result<()> {
    for file in [SEGMENTATION_FILE, EMBEDDING_FILE] {
        let path = dir.join(file);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}

pub fn download_diarization_models(
    dir: &Path,
    on_progress: &mut dyn FnMut(f64),
    cancelled: Arc<AtomicBool>,
) -> anyhow::Result<DiarizationModelPaths> {
    std::fs::create_dir_all(dir)?;
    let segmentation = dir.join(SEGMENTATION_FILE);
    let embedding = dir.join(EMBEDDING_FILE);

    if !embedding.exists() {
        let mut progress = |f: f64| on_progress(f * 0.3);
        download_file(EMBEDDING_URL, &embedding, &mut progress, cancelled.clone())?;
    }
    on_progress(0.3);

    if !segmentation.exists() {
        let archive = dir.join("segmentation.tar.bz2");
        let mut progress = |f: f64| on_progress(0.3 + f * 0.6);
        download_file(SEGMENTATION_URL, &archive, &mut progress, cancelled.clone())?;

        extract_segmentation_model(&archive, &segmentation)?;
        let _ = std::fs::remove_file(&archive);
    }
    on_progress(1.0);

    Ok(DiarizationModelPaths {
        segmentation,
        embedding,
    })
}

fn extract_segmentation_model(archive: &Path, dest: &Path) -> anyhow::Result<()> {
    let file = std::fs::File::open(archive)?;
    let decoder = bzip2::read::BzDecoder::new(file);
    let mut tar = tar::Archive::new(decoder);
    for entry in tar.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_path_buf();
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == "model.onnx")
        {
            let mut out = std::fs::File::create(dest)?;
            std::io::copy(&mut entry, &mut out)?;
            return Ok(());
        }
    }
    anyhow::bail!("model.onnx not found in segmentation archive")
}

pub fn diarize_file(
    audio: &Path,
    models: &DiarizationModelPaths,
    num_speakers: Option<i32>,
) -> anyhow::Result<Vec<SpeakerTimedSegment>> {
    let samples = fennec_audio::decode_to_mono_16k(audio)
        .with_context(|| format!("failed to decode {}", audio.display()))?;
    if samples.is_empty() {
        return Ok(Vec::new());
    }

    let config = DiarizeConfig {
        // -1 でクラスタ数自動推定（threshold基準）になる
        num_clusters: Some(num_speakers.unwrap_or(-1)),
        threshold: Some(0.5),
        ..Default::default()
    };
    let mut diarizer = Diarize::new(&models.segmentation, &models.embedding, config)
        .map_err(|e| anyhow::anyhow!("failed to initialize diarizer: {e:?}"))?;
    let segments = diarizer
        .compute(samples, None)
        .map_err(|e| anyhow::anyhow!("diarization failed: {e:?}"))?;

    Ok(segments
        .into_iter()
        .map(|s| SpeakerTimedSegment {
            speaker_id: s.speaker,
            start: s.start as f64,
            end: s.end as f64,
        })
        .collect())
}
