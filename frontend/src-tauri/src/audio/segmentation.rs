//! Enhanced segmentation (segmentation-3.0) loader.

use std::path::Path;

#[derive(Debug, Clone)]
pub struct Segment { pub start: f32, pub end: f32 }

pub trait Segmenter: Send + Sync {
    fn segment(&self, audio: &[f32]) -> Result<Vec<Segment>, String>;
}

pub struct Segmentation30Segmenter { inner: polyvoice::PowersetSegmenter }

impl Segmentation30Segmenter {
    pub fn new(model_path: &Path, pool_size: usize) -> Result<Self, String> {
        use polyvoice::models::default_manifest;
        use polyvoice::models::metadata::{load_model_config, ModelConfigMeta};
        let manifest = default_manifest();
        let profile = manifest.profile(polyvoice::Profile::Balanced.manifest_id()).ok_or_else(|| "polyvoice manifest is missing the balanced profile".to_string())?;
        let seg_entry = manifest.model(&profile.segmenter).ok_or_else(|| "polyvoice manifest is missing the segmenter model".to_string())?;
        let meta = load_model_config(Some(model_path), Some(seg_entry), &ModelConfigMeta::default());
        let mut cfg = polyvoice::PowersetConfig::default().with_model_meta(&meta);
        cfg.window_secs = seg_entry.window_secs.unwrap_or(10.0);
        cfg.hop_secs = seg_entry.hop_secs.unwrap_or(2.0);
        cfg.sample_rate = seg_entry.sample_rate.unwrap_or(16000);
        cfg.pool_size = pool_size.clamp(1, 16);
        let seg = polyvoice::PowersetSegmenter::with_config(model_path, cfg, polyvoice::onnx::ExecutionProvider::Cpu)
            .map_err(|e| format!("Failed to create segmentation-3.0 segmenter: {}", e))?;
        Ok(Self { inner: seg })
    }
}
impl Segmenter for Segmentation30Segmenter {
    fn segment(&self, audio: &[f32]) -> Result<Vec<Segment>, String> {
        use polyvoice::segmentation::Segmenter as _;
        let raw = self.inner.segment(audio).map_err(|e| format!("Enhanced segmentation failed: {}", e))?;
        Ok(raw.into_iter().map(|r| Segment { start: r.time.start as f32, end: r.time.end as f32 }).collect())
    }
}

pub fn create_segmenter(models_dir: &Path, pool_size: usize) -> Result<Box<dyn Segmenter>, String> {
    let (seg_path, _emb_path) = crate::audio::embedder::enhanced_model_paths(models_dir);
    if !crate::audio::embedder::is_enhanced_installed(models_dir) {
        return Err(format!(
            "Enhanced segmentation model not found at {}. The enhanced diarization models (segmentation-3.0 + TitaNet-Large) are bundled at build time; rebuild with network or install a build that includes them.",
            seg_path.display()
        ));
    }
    let seg = Segmentation30Segmenter::new(&seg_path, pool_size)?;
    log::info!("Using enhanced segmentation-3.0 from {}", seg_path.display());
    Ok(Box::new(seg))
}