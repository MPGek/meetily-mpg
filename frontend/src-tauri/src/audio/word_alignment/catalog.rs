//! Alignment model catalog and readiness resolution (task 3.2, design D5).
//!
//! Models are user-downloadable (unlike the bundled diarization models), so
//! they live under `app_data_dir/models/alignment/<id>/` with no resource-dir
//! fallback chain. Readiness = every catalogued file exists with at least its
//! minimum size.

use serde::Serialize;
use std::path::{Path, PathBuf};

/// One file in a catalogued model: remote path under the HF repo, local name,
/// expected size (for weighted progress) and minimum valid size.
#[derive(Debug, Clone)]
pub struct ModelFile {
    pub remote: &'static str,
    pub local: &'static str,
    pub expected_bytes: u64,
    pub min_bytes: u64,
}

/// A word-alignment model available for download.
#[derive(Debug, Clone)]
pub struct AlignmentModelSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub hf_repo: &'static str,
    pub size_mb: u32,
    pub languages: &'static str,
    pub description: &'static str,
    pub files: &'static [ModelFile],
}

/// Default multilingual CTC aligner (spike 3.1).
pub const DEFAULT_ALIGNMENT_MODEL_ID: &str = "wav2vec2-xlsr-56";

const fn model_file(remote: &'static str, local: &'static str, expected: u64) -> ModelFile {
    // Min size = ~92% of expected for large binaries (catches truncated
    // downloads), exact-ish small floor for metadata files.
    let min = if expected > 1_000_000 {
        expected * 92 / 100
    } else {
        expected * 90 / 100
    };
    ModelFile {
        remote,
        local,
        expected_bytes: expected,
        min_bytes: min,
    }
}

pub static CATALOG: &[AlignmentModelSpec] = &[AlignmentModelSpec {
    id: DEFAULT_ALIGNMENT_MODEL_ID,
    name: "wav2vec2 XLS-R 56 (multilingual)",
    hf_repo: "NewComer00/wav2vec2-xlsr-multilingual-56-ONNX",
    size_mb: 622,
    languages: "56 languages (Latin, Cyrillic, Greek, Arabic, Hebrew, Devanagari, Thai, CJK romanization)",
    description: "wav2vec2-large CTC forced aligner; raw 16 kHz waveform in, per-frame character posteriors out",
    files: &[
        model_file("onnx/model_fp16.onnx", "model_fp16.onnx", 651_760_843),
        model_file("config.json", "config.json", 2_280),
        model_file("vocab.json", "vocab.json", 146_914),
        model_file("preprocessor_config.json", "preprocessor_config.json", 214),
        model_file("special_tokens_map.json", "special_tokens_map.json", 96),
        model_file("tokenizer_config.json", "tokenizer_config.json", 1_132),
    ],
}];

/// The catalog of downloadable alignment models.
pub fn catalog() -> &'static [AlignmentModelSpec] {
    CATALOG
}

/// Look up a catalog entry by id.
pub fn spec_by_id(id: &str) -> Option<&'static AlignmentModelSpec> {
    catalog().iter().find(|s| s.id == id)
}

/// Root directory for alignment models: `<models_root>/alignment`.
pub fn alignment_models_root(models_root: &Path) -> PathBuf {
    models_root.join("alignment")
}

/// Directory for one model id.
pub fn model_dir(models_root: &Path, id: &str) -> PathBuf {
    alignment_models_root(models_root).join(id)
}

/// Readiness of an alignment model, mirroring the Parakeet status enum.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "state", content = "detail")]
pub enum AlignmentModelStatus {
    Available,
    Missing,
    Downloading { progress: u8 },
    Corrupted { file_size: u64, expected_min_size: u64 },
}

/// Resolve on-disk readiness for one model: every file exists and meets its
/// minimum size. Returns Available / Missing / Corrupted (Downloading is set
/// by the manager, not the filesystem).
pub fn resolve_status(dir: &Path, spec: &AlignmentModelSpec) -> AlignmentModelStatus {
    if !dir.exists() {
        return AlignmentModelStatus::Missing;
    }
    let mut total = 0u64;
    let mut min_total = 0u64;
    for file in spec.files {
        let path = dir.join(file.local);
        match std::fs::metadata(&path) {
            Ok(meta) => {
                total += meta.len();
                min_total += file.min_bytes;
                if meta.len() < file.min_bytes {
                    return AlignmentModelStatus::Corrupted {
                        file_size: total,
                        expected_min_size: min_total,
                    };
                }
            }
            Err(_) => return AlignmentModelStatus::Missing,
        }
    }
    AlignmentModelStatus::Available
}

/// Catalogued model with its resolved status (frontend-facing shape).
#[derive(Debug, Clone, Serialize)]
pub struct AlignmentModelInfo {
    pub id: String,
    pub name: String,
    pub size_mb: u32,
    pub languages: String,
    pub description: String,
    pub path: PathBuf,
    pub status: AlignmentModelStatus,
}

/// Build the frontend-facing list for all catalogued models.
pub fn list_models(models_root: &Path) -> Vec<AlignmentModelInfo> {
    catalog()
        .iter()
        .map(|spec| AlignmentModelInfo {
            id: spec.id.to_string(),
            name: spec.name.to_string(),
            size_mb: spec.size_mb,
            languages: spec.languages.to_string(),
            description: spec.description.to_string(),
            path: model_dir(models_root, spec.id),
            status: resolve_status(&model_dir(models_root, spec.id), spec),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "meetily_align_catalog_{}_{}_{}",
            tag,
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_dir_reports_missing() {
        let dir = temp_dir("missing");
        let spec = spec_by_id(DEFAULT_ALIGNMENT_MODEL_ID).unwrap();
        let model = model_dir(&dir, spec.id);
        assert_eq!(resolve_status(&model, spec), AlignmentModelStatus::Missing);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn fake_complete_dir_reports_available_and_short_file_corrupted() {
        let dir = temp_dir("fake");
        let spec = spec_by_id(DEFAULT_ALIGNMENT_MODEL_ID).unwrap();
        let model = model_dir(&dir, spec.id);
        std::fs::create_dir_all(&model).unwrap();
        // Fake every file at exactly its minimum size.
        for file in spec.files {
            std::fs::write(
                model.join(file.local),
                vec![0u8; file.min_bytes as usize],
            )
            .unwrap();
        }
        assert_eq!(resolve_status(&model, spec), AlignmentModelStatus::Available);

        // Truncate the ONNX file below its minimum -> Corrupted.
        let onnx = spec.files[0].local;
        std::fs::write(model.join(onnx), vec![0u8; 1024]).unwrap();
        assert!(matches!(
            resolve_status(&model, spec),
            AlignmentModelStatus::Corrupted { .. }
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn list_models_reports_per_model_status() {
        let dir = temp_dir("list");
        let infos = list_models(&dir);
        assert_eq!(infos.len(), catalog().len());
        assert!(infos.iter().all(|i| i.status == AlignmentModelStatus::Missing));
        std::fs::remove_dir_all(&dir).ok();
    }
}
