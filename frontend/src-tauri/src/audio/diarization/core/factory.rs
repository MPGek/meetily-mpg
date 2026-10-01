//! Building the diarization pipeline: the app-aware and standalone model
//! resolution, the one construction site for the clusterer, and the
//! Tauri-free entry point the eval harness uses.

use std::path::{Path, PathBuf};
use tauri::{AppHandle, Runtime};

use super::super::batch::orchestrator::run_channel_diarization;
use super::super::{ChannelClusters, DiarizationConfig, PolyvoiceDiarizer};
use super::cluster::{clusterer_for_batch, effective_cluster_ceiling};

pub(crate) fn create_polyvoice_diarizer(
    models_dir: &Path,
    max_speakers: Option<i32>,
    config: &DiarizationConfig,
) -> Result<PolyvoiceDiarizer, String> {
    // Enhanced-only engine family: segmentation and embedding construction
    // error with clear messages when the bundled enhanced models are absent
    // (no fallback to a standard/legacy model set).
    let segmenter = crate::audio::segmentation::create_v2_segmenter(
        models_dir,
        config.segmenter_pool_size(),
        config.binarization,
    )?;
    let embedder =
        crate::audio::embedder::create_speaker_embedder(models_dir, config.embedder_pool_size())
            .map_err(|e| format!("Failed to create embedder: {}", e))?;
    let model_tag = embedder.model_tag();

    // Always-on ceiling, resolved by the rule both paths share.
    let max_clusters = effective_cluster_ceiling(config, max_speakers);
    let clusterer = clusterer_for_batch(config, max_clusters)?.inner;
    log::info!(
        "Diarizer using enhanced family tag={} embed_window={:.1}s kind={}",
        model_tag,
        config.embed_window_secs,
        config.clusterer.as_str(),
    );

    Ok(PolyvoiceDiarizer {
        segmenter,
        embedder,
        clusterer,
        resegmenter: polyvoice::resegmentation::OverlapResegmenter::default(),
    })
}

pub(crate) fn create_polyvoice_diarizer_for_app<R: Runtime>(
    app: &AppHandle<R>,
    max_speakers: Option<i32>,
    config: &DiarizationConfig,
) -> Result<PolyvoiceDiarizer, String> {
    if let Some(dir) = crate::audio::embedder::resolve_enhanced_models_dir(app) {
        return create_polyvoice_diarizer(&dir, max_speakers, config);
    }
    let locations = crate::audio::embedder::format_enhanced_search_locations(app);
    Err(format!(
        "Enhanced diarization models not found. Searched: {}. The enhanced models (segmentation-3.0 + TitaNet-Large) are bundled at build time near the executable; rebuild with network or install a build that includes them.",
        locations
    ))
}

// ===== Tauri/DB-free diarization core (shared with the `diarize-eval` bin) =====

/// Candidate model directories without an `AppHandle`, mirroring the app's
/// 3-location fallback: app data dir → resource dir near the executable →
/// dev manifest dir. An explicit override (CLI flag / env) is prepended.
fn standalone_model_candidates(explicit: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(dir) = explicit {
        // Explicit override is strict: no fallback locations are searched.
        dirs.push(dir.to_path_buf());
        return dirs;
    }
    if let Ok(dir) = std::env::var("MEETILY_MODELS_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    // 1. app_data_dir/models (same identifier the Tauri resolver uses)
    #[cfg(target_os = "windows")]
    if let Ok(appdata) = std::env::var("APPDATA") {
        dirs.push(PathBuf::from(appdata).join("com.meetily.ai").join("models"));
    }
    #[cfg(target_os = "macos")]
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("com.meetily.ai")
                .join("models"),
        );
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
            dirs.push(PathBuf::from(xdg).join("com.meetily.ai").join("models"));
        } else if let Ok(home) = std::env::var("HOME") {
            dirs.push(
                PathBuf::from(home)
                    .join(".local")
                    .join("share")
                    .join("com.meetily.ai")
                    .join("models"),
            );
        }
    }
    // 2. resource dir near the executable (bundled install layout)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.join("resources").join("models"));
            dirs.push(parent.join("models"));
        }
    }
    // 3. dev manifest fallback (`cargo run`/`cargo build` from src-tauri)
    dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models"));
    dirs
}

/// Resolve the enhanced models directory without an `AppHandle`. When
/// `explicit` is given it must verify on its own (no fallback); otherwise the
/// standalone 3-location fallback applies. Errors name the missing model
/// files and every searched location.
pub fn resolve_models_dir_standalone(explicit: Option<&Path>) -> Result<PathBuf, String> {
    let candidates = standalone_model_candidates(explicit);
    if let Some(dir) = crate::audio::embedder::resolve_enhanced_models_dir_from_paths(&candidates) {
        return Ok(dir);
    }
    Err(format!(
        "Enhanced diarization models not found ({} and {}). Searched: {}",
        crate::audio::embedder::ENHANCED_SEGMENTATION_FILE,
        crate::audio::embedder::ENHANCED_EMBEDDING_FILE,
        crate::audio::embedder::format_search_locations_from_paths(&candidates),
    ))
}

/// Create the production diarizer without an `AppHandle`, using an explicit
/// models directory or the standalone 3-location fallback.
pub fn create_diarizer_standalone(
    models_dir: Option<&Path>,
    max_speakers: Option<i32>,
    config: &DiarizationConfig,
) -> Result<PolyvoiceDiarizer, String> {
    let dir = resolve_models_dir_standalone(models_dir)?;
    create_polyvoice_diarizer(&dir, max_speakers, config)
}

/// Tauri/DB-free entry point: diarize one channel of in-memory samples with
/// the same chunked pipeline the app uses for offline diarization.
pub fn diarize_wav_samples(
    samples: &[f32],
    sample_rate: u32,
    max_speakers: Option<i32>,
    config: &DiarizationConfig,
    models_dir: Option<&Path>,
) -> Result<ChannelClusters, String> {
    let diarizer = create_diarizer_standalone(models_dir, max_speakers, config)?;
    let (segments, embeddings, _) =
        run_channel_diarization(&diarizer, samples, sample_rate, config, "mono")?;
    Ok(ChannelClusters {
        segments,
        embeddings,
    })
}

/// TitaNet-Large embedder (16 kHz, 192 dims) with layout-correct `[B, 80, T]`
/// adapter. Same embedder family as the offline path, kept concrete because
/// the live engines call `polyvoice::embedder::Embedder` on it directly.
pub(crate) type DiarizationEmbedder = crate::audio::embedder::TitanetAdapter;

/// The live path's embedder, built here so the batch and live paths have one
/// construction site (05 task 2.3). Pool size stays 1: a live session embeds
/// one chunk at a time. The error text is the batch path's, verbatim.
pub(crate) fn create_streaming_embedder(
    embedding_model: &Path,
) -> Result<DiarizationEmbedder, String> {
    if !embedding_model.exists() {
        return Err(format!(
            "Enhanced embedding model not found at {}. The enhanced diarization models (segmentation-3.0 + TitaNet-Large) are bundled at build time; rebuild with network or install a build that includes them.",
            embedding_model.display()
        ));
    }
    DiarizationEmbedder::new(embedding_model, 1)
        .map_err(|e| format!("Failed to create enhanced TitaNet embedder: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Task 2.3 routed the live path through this module; the message a user
    /// sees when the bundled models are missing must still name the exact file
    /// that was looked for, and still say the models are bundled at build time.
    #[test]
    fn streaming_embedder_error_names_the_searched_path() {
        let missing = std::env::temp_dir().join("meetily-no-such-titanet-model.onnx");
        assert!(
            !missing.exists(),
            "fixture path must not exist for this test to mean anything"
        );

        // `expect_err` would need `Debug` on the adapter, which wraps an ONNX
        // session; match instead.
        let err = match create_streaming_embedder(&missing) {
            Err(err) => err,
            Ok(_) => panic!("missing model must error"),
        };
        assert!(
            err.contains(&missing.display().to_string()),
            "error must name the searched path, got: {err}"
        );
        assert!(
            err.contains("bundled at build time"),
            "error must keep the bundled-at-build-time wording, got: {err}"
        );
    }
}
