//! Resolved diarization parameters (05 D1): the built-in defaults, the
//! persisted clustering overrides the frontend mirrors to the backend, and the
//! `DiarizationConfig` both the batch pass and a live session resolve from.

use std::sync::atomic::{AtomicU64, Ordering};

/// Fixed concurrency profile: the ONNX session pool size is the smaller of 8
/// or 75% of the logical CPU core count (rounded up, minimum 1). There is no
/// user-facing memory-mode or session-count setting.
pub(crate) fn fixed_pool_size() -> usize {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let seventy_five_percent = ((cores as f64) * 0.75).ceil() as usize;
    seventy_five_percent.clamp(1, 8)
}

/// Built-in default speaker-count ceiling: sweep-selected (extended grid,
/// 2026-09-04) — tight ceilings force below-threshold merges and inflate
/// confusion, so the ceiling sits well above real meeting speaker counts while
/// still guaranteeing the clusterer never runs unbounded.
pub const DEFAULT_CLUSTER_CEILING: usize = 128;

/// Built-in default same-speaker gap-merge window (sweep-selected, 2026-09-04).
pub const DEFAULT_GAP_MERGE_SECS: f32 = 0.3;

/// Built-in default for the stop-time refinement pass (05b D1): on. The pass
/// re-clusters a finished live session's buffered embeddings so incrementally
/// merged identities can be separated before anything is persisted; the
/// fallback to the incremental identities is unconditional, so leaving it on
/// cannot fail a stop.
pub const DEFAULT_FINAL_RECLUSTER: bool = true;

/// Built-in default merge threshold for the stop-time refinement (05b D1).
///
/// Its own constant because the offline threshold does not carry over: that one
/// (`TITANET_CLUSTER_THRESHOLD`) was tuned on dense 5 s windows, while the
/// refinement clusters one embedding per merged speech chunk of up to 25 s. At
/// the offline value the pass under-clusters (measured: 3.8 speakers per
/// recording against 7.8 in the reference on long recordings). A threshold the
/// user stored still wins; this applies only when none is stored. Compiled-in
/// and harness-sweepable (`online-eval --final-recluster-threshold`), not a
/// persisted setting, like the binarization constants.
///
/// Sweep-selected (2026-09-30) on every 4th recording of `voxconverse-dev`, the
/// tuning set: online DER 29.99 / 28.64 / 25.15 / 22.88 / 24.09 at
/// 0.60 / 0.65 / 0.70 / 0.75 / 0.80, so the optimum is bracketed rather than
/// at an edge of the grid. Confirmed on the held-out sets before it shipped.
pub const DEFAULT_FINAL_RECLUSTER_THRESHOLD: f32 = 0.75;

/// Built-in default for the wholesale final relabel (05b D2): off. The final
/// pass revises the blocks whose speaker actually changed; re-emitting every
/// block reads as a bug to someone watching the transcript, even when every
/// label improved, so it is opt-in.
pub const DEFAULT_FINAL_RELABEL_ALL: bool = false;

/// Pipeline's clustering-backend maximum speaker count (`u8` local labels).
pub(crate) const MAX_CLUSTERERS: usize = 255;

/// Dense embedding window length (seconds, w/2 hop) — compiled-in constant
/// (pipeline-v2 D4), harness-sweepable via `--embed-window`, not a persisted
/// setting.
pub const DEFAULT_EMBED_WINDOW_SECS: f32 = 5.0;

/// Minimum segment length (seconds) accepted for embedding: sub-0.2 s windows
/// collapse the pooling std toward NaN on the TitaNet time-downsample path
/// (mirrors the vendored `MIN_EMBED_SECS`).
pub(crate) const MIN_EMBED_SECS: f64 = 0.20;

/// Calibrated binarization constants (pipeline-v2 D4): the vendored
/// `BinarizationConfig` default is plain thresholding (0.5/0.5/0/0), so the
/// spike selects hysteresis + min-duration smoothing constants; harness-
/// sweepable via `--binarization`, not a persisted setting.
pub const DEFAULT_BINARIZATION: polyvoice::segmentation::BinarizationConfig =
    polyvoice::segmentation::BinarizationConfig {
        onset: 0.5,
        offset: 0.4,
        min_duration_on: 0.2,
        min_duration_off: 0.2,
    };

/// Built-in minimum turn duration (seconds) after resegmentation (mirrors
/// `PipelineConfig::min_speech_secs`).
pub const DEFAULT_MIN_SPEECH_SECS: f32 = 0.25;

/// Offline clusterer kind (diarization-param-tuning, pipeline-v2 D2).
/// `ahc` is the built-in default (fixed cosine threshold, selected by the 6.2
/// sweep: NME-SC under-clusters dense TitaNet windows into ~1 speaker/file).
/// `nmesc` remains selectable (automatic count over cosine-affinity spectral
/// clustering, dimension-agnostic). `vbx` stays parseable but the clusterer
/// factory rejects it for the enhanced 192-d family (the vendored PLDA params
/// require 256-d embeddings) — no silent kind switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClustererKindSetting {
    Nmesc,
    Vbx,
    Ahc,
}

impl ClustererKindSetting {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Nmesc => "nmesc",
            Self::Vbx => "vbx",
            Self::Ahc => "ahc",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "nmesc" => Some(Self::Nmesc),
            "vbx" => Some(Self::Vbx),
            "ahc" => Some(Self::Ahc),
            _ => None,
        }
    }

    pub fn is_automatic_count(self) -> bool {
        matches!(self, Self::Nmesc | Self::Vbx)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DiarizationConfig {
    pub max_sessions: usize,
    pub chunk_overlap_secs: f32,
    /// AHC merge criterion: minimum cosine similarity to merge two clusters.
    /// Applies to the `ahc` kind only (ignored under automatic count).
    pub cluster_threshold: f32,
    /// Hard ceiling on distinct speaker labels per channel per pass.
    pub cluster_ceiling: usize,
    /// Merge consecutive same-speaker segments when the gap between them is
    /// within this window (0 disables gap-merging).
    pub gap_merge_secs: f32,
    /// Clusterer kind (nmesc|vbx|ahc); default `ahc` (6.2 sweep).
    pub clusterer: ClustererKindSetting,
    /// Dense embedding window (0 = sparse one-embedding-per-segment).
    pub embed_window_secs: f32,
    /// Calibrated binarization of segmentation posteriors (None = argmax).
    pub binarization: Option<polyvoice::segmentation::BinarizationConfig>,
    /// Minimum output turn duration.
    pub min_speech_secs: f32,
    /// Re-cluster a live session's buffered embeddings when the recording
    /// stops (05b D1). Read by the streaming path only; the batch pass always
    /// clusters, so it ignores this.
    pub final_recluster: bool,
    /// Have the stop-time display pass re-emit every live block instead of
    /// only the ones whose speaker changed (05b D2). Display-only.
    pub final_relabel_all: bool,
    /// AHC merge criterion for the stop-time refinement only (05b D1); see
    /// `DEFAULT_FINAL_RECLUSTER_THRESHOLD` for why it is not `cluster_threshold`.
    pub final_recluster_threshold: f32,
}

impl Default for DiarizationConfig {
    fn default() -> Self {
        Self {
            max_sessions: fixed_pool_size(),
            chunk_overlap_secs: 5.0,
            cluster_threshold: crate::audio::embedder::TITANET_CLUSTER_THRESHOLD,
            cluster_ceiling: DEFAULT_CLUSTER_CEILING,
            gap_merge_secs: DEFAULT_GAP_MERGE_SECS,
            clusterer: ClustererKindSetting::Ahc,
            embed_window_secs: DEFAULT_EMBED_WINDOW_SECS,
            binarization: Some(DEFAULT_BINARIZATION),
            min_speech_secs: DEFAULT_MIN_SPEECH_SECS,
            final_recluster: DEFAULT_FINAL_RECLUSTER,
            final_relabel_all: DEFAULT_FINAL_RELABEL_ALL,
            final_recluster_threshold: DEFAULT_FINAL_RECLUSTER_THRESHOLD,
        }
    }
}

impl DiarizationConfig {
    pub(crate) fn chunk_duration_secs(&self) -> f32 {
        DIARIZATION_CHUNK_DURATION_SECS
    }

    pub(crate) fn embedder_pool_size(&self) -> usize {
        self.max_sessions.clamp(1, 16)
    }

    pub(crate) fn segmenter_pool_size(&self) -> usize {
        self.max_sessions.clamp(1, 16)
    }

    /// App-path config: built-in defaults overlaid with persisted clustering
    /// settings (diarization-param-tuning D2). A stored override wins per key;
    /// unset keys fall back to the built-in defaults.
    pub fn resolved() -> Self {
        Self {
            cluster_threshold: stored_cluster_threshold()
                .unwrap_or(crate::audio::embedder::TITANET_CLUSTER_THRESHOLD),
            cluster_ceiling: stored_cluster_ceiling().unwrap_or(DEFAULT_CLUSTER_CEILING),
            gap_merge_secs: stored_gap_merge_secs().unwrap_or(DEFAULT_GAP_MERGE_SECS),
            clusterer: stored_clusterer_kind().unwrap_or(ClustererKindSetting::Ahc),
            final_recluster: stored_final_recluster().unwrap_or(DEFAULT_FINAL_RECLUSTER),
            final_relabel_all: stored_final_relabel_all().unwrap_or(DEFAULT_FINAL_RELABEL_ALL),
            // A threshold the user stored applies to the refinement too; the
            // dedicated default only fills the gap when none is stored.
            final_recluster_threshold: stored_cluster_threshold()
                .unwrap_or(DEFAULT_FINAL_RECLUSTER_THRESHOLD),
            ..Self::default()
        }
    }
}

// ===== Persisted clustering-settings holder (diarization-param-tuning D2) ====
//
// Mirrors `word_alignment::settings`: the frontend persists the keys in its
// settings store and mirrors them to the backend via
// `set_diarization_clustering_settings`; offline diarization reads them when
// building `DiarizationConfig`. The `diarize-eval` harness never consults
// these globals (D4: it measures defaults + explicit CLI flags).

static CLUSTER_THRESHOLD_OVERRIDE: AtomicU64 = AtomicU64::new(0); // 0 = unset, else f32 bits + 1
static CLUSTER_CEILING_OVERRIDE: AtomicU64 = AtomicU64::new(0); // 0 = unset, else value
static GAP_MERGE_SECS_OVERRIDE: AtomicU64 = AtomicU64::new(0); // 0 = unset, else f32 bits + 1
static CLUSTERER_KIND_OVERRIDE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0); // 0 = unset
static FINAL_RECLUSTER_OVERRIDE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0); // 0 = unset, 1 = off, 2 = on
static FINAL_RELABEL_ALL_OVERRIDE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0); // 0 = unset, 1 = off, 2 = on

fn store_f32_option(cell: &AtomicU64, value: Option<f32>) {
    match value {
        Some(v) => cell.store((v.to_bits() as u64) + 1, Ordering::SeqCst),
        None => cell.store(0, Ordering::SeqCst),
    }
}

fn load_f32_option(cell: &AtomicU64) -> Option<f32> {
    let raw = cell.load(Ordering::SeqCst);
    if raw == 0 {
        None
    } else {
        Some(f32::from_bits((raw - 1) as u32))
    }
}

pub(crate) fn stored_cluster_threshold() -> Option<f32> {
    load_f32_option(&CLUSTER_THRESHOLD_OVERRIDE)
}

pub(crate) fn stored_cluster_ceiling() -> Option<usize> {
    match CLUSTER_CEILING_OVERRIDE.load(Ordering::SeqCst) {
        0 => None,
        v => Some(v as usize),
    }
}

pub(crate) fn stored_gap_merge_secs() -> Option<f32> {
    load_f32_option(&GAP_MERGE_SECS_OVERRIDE)
}

fn kind_code(kind: ClustererKindSetting) -> u8 {
    match kind {
        ClustererKindSetting::Nmesc => 1,
        ClustererKindSetting::Vbx => 2,
        ClustererKindSetting::Ahc => 3,
    }
}

pub(crate) fn stored_clusterer_kind() -> Option<ClustererKindSetting> {
    match CLUSTERER_KIND_OVERRIDE.load(Ordering::SeqCst) {
        0 => None,
        1 => Some(ClustererKindSetting::Nmesc),
        2 => Some(ClustererKindSetting::Vbx),
        3 => Some(ClustererKindSetting::Ahc),
        _ => None,
    }
}

pub(crate) fn stored_final_recluster() -> Option<bool> {
    tri_state(&FINAL_RECLUSTER_OVERRIDE)
}

pub(crate) fn stored_final_relabel_all() -> Option<bool> {
    tri_state(&FINAL_RELABEL_ALL_OVERRIDE)
}

/// 0 = unset (the built-in default applies), 1 = off, 2 = on.
fn tri_state(cell: &std::sync::atomic::AtomicU8) -> Option<bool> {
    match cell.load(Ordering::SeqCst) {
        1 => Some(false),
        2 => Some(true),
        _ => None,
    }
}

fn store_tri_state(cell: &std::sync::atomic::AtomicU8, value: Option<bool>) {
    cell.store(
        match value {
            Some(true) => 2,
            Some(false) => 1,
            None => 0,
        },
        Ordering::SeqCst,
    );
}

/// Update the persisted clustering overrides (None clears a key back to the
/// built-in default).
pub fn set_clustering_overrides(
    cluster_threshold: Option<f32>,
    cluster_ceiling: Option<usize>,
    gap_merge_secs: Option<f32>,
    clusterer: Option<ClustererKindSetting>,
    final_recluster: Option<bool>,
    final_relabel_all: Option<bool>,
) {
    store_f32_option(&CLUSTER_THRESHOLD_OVERRIDE, cluster_threshold);
    CLUSTER_CEILING_OVERRIDE.store(
        cluster_ceiling.map(|v| v as u64).unwrap_or(0),
        Ordering::SeqCst,
    );
    store_f32_option(&GAP_MERGE_SECS_OVERRIDE, gap_merge_secs);
    CLUSTERER_KIND_OVERRIDE
        .store(clusterer.map(kind_code).unwrap_or(0), Ordering::SeqCst);
    store_tri_state(&FINAL_RECLUSTER_OVERRIDE, final_recluster);
    store_tri_state(&FINAL_RELABEL_ALL_OVERRIDE, final_relabel_all);
    log::info!(
        "Diarization clustering settings updated: threshold={:?}, ceiling={:?}, gap_merge={:?}, clusterer={:?}, final_recluster={:?}, final_relabel_all={:?}",
        stored_cluster_threshold(),
        stored_cluster_ceiling(),
        stored_gap_merge_secs(),
        stored_clusterer_kind(),
        stored_final_recluster(),
        stored_final_relabel_all(),
    );
}

pub(crate) const DIARIZATION_SAMPLE_RATE: u32 = 16000;

/// Fixed chunk duration for offline diarization (seconds). Diarization always
/// processes recordings in chunks to keep peak memory bounded.
pub(crate) const DIARIZATION_CHUNK_DURATION_SECS: f32 = 600.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diarization_config_uses_fixed_profile() {
        let cfg = DiarizationConfig::default();
        assert!(cfg.max_sessions >= 1 && cfg.max_sessions <= 8);
        assert_eq!(cfg.chunk_overlap_secs, 5.0);
        assert_eq!(cfg.chunk_duration_secs(), 600.0);
    }

    #[test]
    fn diarization_config_clustering_defaults() {
        let cfg = DiarizationConfig::default();
        assert_eq!(cfg.cluster_threshold, 0.60);
        assert_eq!(cfg.cluster_ceiling, DEFAULT_CLUSTER_CEILING);
        assert_eq!(cfg.cluster_ceiling, 128);
        assert_eq!(cfg.gap_merge_secs, 0.3);
        // Built-in default kind is ahc (6.2 sweep: nmesc under-clusters).
        assert_eq!(cfg.clusterer, ClustererKindSetting::Ahc);
        assert_eq!(cfg.embed_window_secs, DEFAULT_EMBED_WINDOW_SECS);
        assert_eq!(cfg.embed_window_secs, 5.0);
        assert_eq!(cfg.min_speech_secs, 0.25);
        let bin = cfg.binarization.expect("v2 default enables calibrated binarization");
        assert!(bin.offset < bin.onset, "hysteresis: offset below onset");
        assert!(bin.min_duration_on > 0.0 && bin.min_duration_off > 0.0);
        // Built-in threshold equals the family default (single source of truth).
        assert_eq!(
            cfg.cluster_threshold,
            crate::audio::embedder::TITANET_CLUSTER_THRESHOLD
        );
    }

    #[test]
    fn resolved_config_prefers_stored_overrides_and_falls_back_to_defaults() {
        // Save/restore the process globals so the test is parallel-safe:
        // no other test reads the clustering overrides.
        let saved = (
            stored_cluster_threshold(),
            stored_cluster_ceiling(),
            stored_gap_merge_secs(),
            stored_clusterer_kind(),
            stored_final_recluster(),
            stored_final_relabel_all(),
        );
        // Unset -> built-in defaults.
        set_clustering_overrides(None, None, None, None, None, None);
        let cfg = DiarizationConfig::resolved();
        assert_eq!(cfg.cluster_threshold, crate::audio::embedder::TITANET_CLUSTER_THRESHOLD);
        assert_eq!(cfg.cluster_ceiling, DEFAULT_CLUSTER_CEILING);
        assert_eq!(cfg.gap_merge_secs, DEFAULT_GAP_MERGE_SECS);
        assert_eq!(cfg.clusterer, ClustererKindSetting::Ahc);
        assert!(
            cfg.final_recluster,
            "the stop-time refinement is on by default"
        );
        assert!(
            !cfg.final_relabel_all,
            "the wholesale final relabel is opt-in"
        );
        assert_eq!(
            cfg.final_recluster_threshold, DEFAULT_FINAL_RECLUSTER_THRESHOLD,
            "with no stored threshold the refinement uses its own default"
        );
        // Stored override wins per key.
        set_clustering_overrides(
            Some(0.35),
            Some(12),
            Some(0.3),
            Some(ClustererKindSetting::Ahc),
            Some(false),
            Some(true),
        );
        let cfg = DiarizationConfig::resolved();
        assert_eq!(cfg.cluster_threshold, 0.35);
        assert_eq!(cfg.cluster_ceiling, 12);
        assert_eq!(cfg.gap_merge_secs, 0.3);
        assert_eq!(cfg.clusterer, ClustererKindSetting::Ahc);
        assert!(!cfg.final_recluster, "an explicit off is honoured");
        assert_eq!(
            cfg.final_recluster_threshold, 0.35,
            "a threshold the user stored applies to the refinement as well"
        );
        assert!(cfg.final_relabel_all, "an explicit on is honoured");
        // Partial override: only the ceiling is stored, others fall back.
        set_clustering_overrides(None, Some(7), None, None, None, None);
        let cfg = DiarizationConfig::resolved();
        assert_eq!(cfg.cluster_threshold, crate::audio::embedder::TITANET_CLUSTER_THRESHOLD);
        assert_eq!(cfg.cluster_ceiling, 7);
        assert_eq!(cfg.gap_merge_secs, DEFAULT_GAP_MERGE_SECS);
        assert_eq!(cfg.clusterer, ClustererKindSetting::Ahc);
        assert_eq!(cfg.final_recluster, DEFAULT_FINAL_RECLUSTER);
        assert_eq!(cfg.final_relabel_all, DEFAULT_FINAL_RELABEL_ALL);
        set_clustering_overrides(saved.0, saved.1, saved.2, saved.3, saved.4, saved.5);
    }

    #[test]
    fn fixed_pool_size_respects_floor_and_cap() {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let expected = (((cores as f64) * 0.75).ceil() as usize).clamp(1, 8);
        assert_eq!(fixed_pool_size(), expected);
        assert!(fixed_pool_size() >= 1);
        assert!(fixed_pool_size() <= 8);
    }
}
