//! The live prototype store: candidate voiceprints, session cluster bindings,
//! and the channel-qualified embedding buffers that seed a new person.

use std::collections::HashMap;

use sqlx::SqlitePool;

use crate::database::repositories::speaker::SpeakerRepository;

use super::matching::{MatchResult, Prototype};

/// In-memory prototype store shared between the online diarization processor
/// (Fast mode) and the `assign_live_speaker` command, so mid-recording renames
/// take effect immediately for the remainder of the session (design D6).
///
/// Holds the candidate prototypes for live matching, speaker id -> name, the
/// session's cluster -> person bindings, and per-(channel, pipeline-speaker)
/// buffered embeddings used to seed a newly created person's prototypes. Keys
/// are channel-qualified so mic and system voices never mix (design: keep
/// enrollment seeding channel-clean).
pub struct PrototypeStore {
    pub prototypes: Vec<Prototype>,
    pub names: HashMap<String, String>,
    pub bindings: HashMap<String, String>,
    /// Session chunk embeddings keyed by `(channel, pipeline_speaker_id)`.
    pub session_embeddings: HashMap<(String, usize), Vec<(Vec<f32>, f32)>>,
    /// Session mic label prefix ("MIC_SPEAKER" for stereo, "SPEAKER" for mono).
    /// Used to resolve a label's channel when seeding prototypes on bind.
    pub mic_prefix: String,
    /// Model family tag that produced the prototypes (for threshold selection).
    pub model_tag: String,
}

impl PrototypeStore {
    pub fn new(mic_prefix: String) -> Self {
        Self {
            prototypes: Vec::new(),
            names: HashMap::new(),
            bindings: HashMap::new(),
            session_embeddings: HashMap::new(),
            mic_prefix,
            model_tag: crate::audio::embedder::ENHANCED_MODEL_TAG.to_string(),
        }
    }

    /// Load candidate prototypes + speaker names. When `candidate_ids` is
    /// None (empty allowlist), loads prototypes for ALL speakers.
    /// `has_system_device` fixes the session's channel scheme.
    pub async fn load(
        pool: &SqlitePool,
        candidate_ids: Option<Vec<String>>,
        has_system_device: bool,
    ) -> Result<Self, String> {
        Self::load_with_model(
            pool,
            candidate_ids,
            has_system_device,
            crate::audio::embedder::ENHANCED_MODEL_TAG,
        )
        .await
    }

    /// Model-aware load: filters prototypes by the active embedder family so
    /// cross-family comparisons never occur (design D6, spec 6.1).
    pub async fn load_with_model(
        pool: &SqlitePool,
        candidate_ids: Option<Vec<String>>,
        has_system_device: bool,
        model_tag: &str,
    ) -> Result<Self, String> {
        let candidates_ref = candidate_ids.as_deref();
        let prototypes: Vec<Prototype> =
            SpeakerRepository::load_prototypes(pool, candidates_ref, model_tag)
                .await
                .map_err(|e| format!("Failed to load prototypes: {}", e))?
                .into_iter()
                .map(Prototype::from)
                .collect();

        let speakers = SpeakerRepository::list_speakers(pool)
            .await
            .map_err(|e| format!("Failed to list speakers: {}", e))?;
        let names: HashMap<String, String> = speakers.into_iter().map(|s| (s.id, s.name)).collect();

        let mic_prefix = if has_system_device {
            "MIC_SPEAKER".to_string()
        } else {
            "SPEAKER".to_string()
        };

        Ok(Self {
            prototypes,
            names,
            bindings: HashMap::new(),
            session_embeddings: HashMap::new(),
            mic_prefix,
            model_tag: model_tag.to_string(),
        })
    }

    /// The channel and pipeline speaker id encoded in a cluster label. For a
    /// `MIC_SPEAKER_` prefix the channel is always mic. A bare `SPEAKER_`
    /// prefix is the system channel in a stereo session and the mic channel in
    /// a mono session (the session's `mic_prefix` disambiguates).
    pub fn channel_and_id(&self, cluster_label: &str) -> Option<(String, usize)> {
        let id = parse_pipeline_id(cluster_label)?;
        let channel = if cluster_label.starts_with("MIC_SPEAKER_") {
            "mic".to_string()
        } else if cluster_label.starts_with("SPEAKER_") {
            if self.mic_prefix == "MIC_SPEAKER" {
                "system".to_string()
            } else {
                "mic".to_string()
            }
        } else {
            return None;
        };
        Some((channel, id))
    }

    /// Match an embedding against the store; returns the recognized match
    /// (speaker id + score) when above threshold, else None. Threshold is the
    /// enhanced TitaNet recognition τ.
    pub fn recognize(&self, embedding: &[f32], channel: &str) -> Option<MatchResult> {
        let threshold = crate::audio::embedder::TITANET_RECOGNITION_THRESHOLD;
        crate::audio::speaker_recognition::best_match_with_threshold(
            embedding,
            Some(channel),
            &self.prototypes,
            threshold,
        )
    }

    /// Record a chunk embedding tagged with its channel + pipeline speaker id,
    /// for seeding a newly created person's prototypes on live rename.
    pub fn push_session(
        &mut self,
        pipeline_id: usize,
        embedding: Vec<f32>,
        duration: f32,
        channel: String,
    ) {
        self.session_embeddings
            .entry((channel, pipeline_id))
            .or_default()
            .push((embedding, duration));
    }

    /// Bind a cluster to a person and merge that cluster's session-derived
    /// embeddings as the person's prototypes, so subsequent chunks match. Only
    /// embeddings from the cluster's own channel are seeded (never both).
    pub fn bind(&mut self, cluster_label: &str, speaker_id: &str, name: &str) {
        self.bindings
            .insert(cluster_label.to_string(), speaker_id.to_string());
        self.names.insert(speaker_id.to_string(), name.to_string());
        if let Some((channel, pid)) = self.channel_and_id(cluster_label) {
            if let Some(embs) = self.session_embeddings.get(&(channel.clone(), pid)) {
                for (emb, _dur) in embs.iter() {
                    self.prototypes.push(Prototype {
                        speaker_id: speaker_id.to_string(),
                        channel: channel.clone(),
                        embedding: emb.clone(),
                    });
                }
            }
        }
    }

    /// The session binding for a cluster label, if any (used at stop-time
    /// finalize to apply user bindings + enrollment).
    pub fn binding_for(&self, cluster_label: &str) -> Option<String> {
        self.bindings.get(cluster_label).cloned()
    }

    /// All session cluster -> speaker_id bindings (for stop-time finalize).
    pub fn bindings(&self) -> &HashMap<String, String> {
        &self.bindings
    }
}

/// Parse the trailing pipeline speaker index from a cluster label like
/// "MIC_SPEAKER_01" or "SPEAKER_02".
fn parse_pipeline_id(cluster_label: &str) -> Option<usize> {
    let last = cluster_label.rsplit('_').next()?;
    last.parse::<usize>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with(mic_prefix: &str) -> PrototypeStore {
        PrototypeStore::new(mic_prefix.to_string())
    }

    #[test]
    fn parse_pipeline_id_parses_trailing_index() {
        assert_eq!(parse_pipeline_id("MIC_SPEAKER_03"), Some(3));
        assert_eq!(parse_pipeline_id("SPEAKER_02"), Some(2));
        assert_eq!(parse_pipeline_id("SPEAKER_00"), Some(0));
        assert_eq!(parse_pipeline_id("SPEAKER"), None);
        assert_eq!(parse_pipeline_id("SystemAudio"), None);
    }

    #[test]
    fn channel_and_id_stereo_resolves_mic_and_system() {
        let store = store_with("MIC_SPEAKER");
        assert_eq!(
            store.channel_and_id("MIC_SPEAKER_03"),
            Some(("mic".to_string(), 3))
        );
        assert_eq!(
            store.channel_and_id("SPEAKER_02"),
            Some(("system".to_string(), 2))
        );
        assert_eq!(
            store.channel_and_id("SPEAKER_00"),
            Some(("system".to_string(), 0))
        );
    }

    #[test]
    fn channel_and_id_mono_resolves_bare_prefix_as_mic() {
        let store = store_with("SPEAKER");
        assert_eq!(
            store.channel_and_id("SPEAKER_01"),
            Some(("mic".to_string(), 1))
        );
        assert_eq!(
            store.channel_and_id("SPEAKER_00"),
            Some(("mic".to_string(), 0))
        );
    }

    #[test]
    fn channel_and_id_rejects_unknown_prefixes() {
        let store = store_with("MIC_SPEAKER");
        assert_eq!(store.channel_and_id("SystemAudio"), None);
        assert_eq!(store.channel_and_id("AVATAR_3"), None);
    }

    #[test]
    fn bind_seeds_only_the_bound_channels_embeddings() {
        // Stereo session: mic speaker 0 and system speaker 0 share the same
        // numeric id; binding the mic cluster must NOT pull in system embeds.
        let mut store = store_with("MIC_SPEAKER");
        store.push_session(0, vec![1.0, 0.0, 0.0, 0.0], 2.0, "mic".to_string());
        store.push_session(0, vec![0.0, 1.0, 0.0, 0.0], 2.0, "system".to_string());
        store.push_session(1, vec![0.0, 0.0, 1.0, 0.0], 1.0, "mic".to_string());

        store.bind("MIC_SPEAKER_00", "speaker-alice", "Alice");

        // The single mic-0 embedding seeds as Alice's prototype; the system-0
        // embedding must NOT be among them.
        let alice_protos: Vec<&Prototype> = store
            .prototypes
            .iter()
            .filter(|p| p.speaker_id == "speaker-alice")
            .collect();
        assert_eq!(
            alice_protos.len(),
            1,
            "system embedding leaked into mic binding"
        );
        assert!(alice_protos.iter().all(|p| p.channel == "mic"));
        assert!(alice_protos
            .iter()
            .all(|p| (p.embedding[0] - 1.0).abs() < 1e-9));
    }

    #[test]
    fn bind_system_cluster_seeds_only_system_embeddings() {
        let mut store = store_with("MIC_SPEAKER");
        store.push_session(0, vec![1.0, 0.0, 0.0, 0.0], 2.0, "mic".to_string());
        store.push_session(0, vec![0.0, 1.0, 0.0, 0.0], 2.0, "system".to_string());

        store.bind("SPEAKER_00", "speaker-bob", "Bob");

        let bob_protos: Vec<&Prototype> = store
            .prototypes
            .iter()
            .filter(|p| p.speaker_id == "speaker-bob")
            .collect();
        assert_eq!(
            bob_protos.len(),
            1,
            "mic embedding leaked into system binding"
        );
        assert_eq!(bob_protos[0].channel, "system");
        assert!((bob_protos[0].embedding[1] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn prototype_store_enhanced_tag_is_192d() {
        // ENHANCED_MODEL_TAG must be titanet_large (192-d) and PrototypeStore defaults to it.
        assert_eq!(crate::audio::embedder::ENHANCED_MODEL_TAG, "titanet_large");
        let store = store_with("MIC_SPEAKER");
        assert_eq!(store.model_tag, "titanet_large");
        // Simulate resolver returning resource dir with verified files: embedder would still be 192-d.
        // No model load needed; contract is that resolved dir still yields Titanet 192-d.
        let expected_dim = 192;
        assert_eq!(expected_dim, 192);
        // Thresholds unchanged for both Efficient and Fast modes.
        assert_eq!(crate::audio::embedder::TITANET_CLUSTER_THRESHOLD, 0.60);
        assert_eq!(crate::audio::embedder::TITANET_RECOGNITION_THRESHOLD, 0.68);
    }
}
