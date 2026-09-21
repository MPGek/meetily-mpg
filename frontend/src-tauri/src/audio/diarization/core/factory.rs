//! Building the diarization pipeline: the app-aware and standalone model
//! resolution, the one construction site for the clusterer, and the
//! Tauri-free entry point the eval harness uses.

use std::path::{Path, PathBuf};
use tauri::{AppHandle, Runtime};

use super::super::batch::orchestrator::run_channel_diarization;
use super::super::{ChannelClusters, DiarizationConfig, PolyvoiceDiarizer};
use super::cluster::{clusterer_for_batch, effective_cluster_ceiling};

pub(crate) fn create_polyvoice_diarizer(
    models_dir: &PathBuf,
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
