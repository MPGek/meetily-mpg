//! Alignment model catalog and readiness resolution (task 3.2, design D5).
//!
//! Models are user-downloadable (unlike the bundled diarization models), so
//! they live under `app_data_dir/models/alignment/<id>/` with no resource-dir
//! fallback chain. Readiness = every catalogued file exists with exactly its
//! catalogued size.

use crate::model_download::transfer::ArtifactSpec;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// A word-alignment model available for download.
#[derive(Debug, Clone)]
pub struct AlignmentModelSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub hf_repo: &'static str,
    /// Pinned repository commit; downloads use `resolve/<revision>`, never a branch.
    pub revision: &'static str,
    pub size_mb: u32,
    pub languages: &'static str,
    pub description: &'static str,
    /// Remote path under the repo, local name and exact byte size of every file.
    pub files: &'static [ArtifactSpec],
}

/// Default multilingual CTC aligner (spike 3.1).
pub const DEFAULT_ALIGNMENT_MODEL_ID: &str = "wav2vec2-xlsr-56";

pub static CATALOG: &[AlignmentModelSpec] = &[AlignmentModelSpec {
    id: DEFAULT_ALIGNMENT_MODEL_ID,
    name: "wav2vec2 XLS-R 56 (multilingual)",
    hf_repo: "NewComer00/wav2vec2-xlsr-multilingual-56-ONNX",
    revision: "2d48b01b6429d9018f81914550565112d56f6ba7",
    size_mb: 622,
    languages: "56 languages (Latin, Cyrillic, Greek, Arabic, Hebrew, Devanagari, Thai, CJK romanization)",
    description: "wav2vec2-large CTC forced aligner; raw 16 kHz waveform in, per-frame character posteriors out",
    files: &[
        ArtifactSpec {
            remote: "onnx/model_fp16.onnx",
            local: "model_fp16.onnx",
            exact_bytes: 651_760_843,
        },
        ArtifactSpec::same("config.json", 2_280),
        ArtifactSpec::same("vocab.json", 146_914),
        ArtifactSpec::same("preprocessor_config.json", 214),
        ArtifactSpec::same("special_tokens_map.json", 96),
        ArtifactSpec::same("tokenizer_config.json", 1_132),
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

/// Resolve on-disk readiness for one model: every file exists with exactly its
/// catalogued size. Returns Available / Missing / Corrupted (Downloading is set
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
                min_total += file.exact_bytes;
                if meta.len() != file.exact_bytes {
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
        // Fake every file at exactly its catalogued size (sparse, no 650 MB write).
        let set_len = |name: &str, len: u64| {
            std::fs::File::create(model.join(name))
                .unwrap()
                .set_len(len)
                .unwrap();
        };
        for file in spec.files {
            set_len(file.local, file.exact_bytes);
        }
        assert_eq!(resolve_status(&model, spec), AlignmentModelStatus::Available);

        // One byte short of the exact size -> Corrupted.
        let onnx = spec.files[0].local;
        set_len(onnx, spec.files[0].exact_bytes - 1);
        assert!(matches!(
            resolve_status(&model, spec),
            AlignmentModelStatus::Corrupted { .. }
        ));

        // Truncated far below its size -> Corrupted.
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
