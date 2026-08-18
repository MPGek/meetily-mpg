//! Speaker recognition core: cosine-similarity matching of a query embedding
//! (a cluster centroid or a live chunk embedding) against enrolled prototype
//! embeddings grouped by speaker.
//!
//! Design D4: prototypes are L2-normalized 256-d f32; matching is max cosine
//! over a candidate's prototypes, best candidate wins, assigned if score > τ.
//! When a candidate has prototypes captured on the same channel as the query,
//! only those are considered (same-channel preference); otherwise all of the
//! candidate's prototypes are used (cross-channel fallback). The `model` tag
//! filter is applied upstream by `SpeakerRepository::load_prototypes`, so the
//! prototypes passed here are already constrained to the current extractor.

use std::collections::HashMap;

/// Recognition threshold τ (design: start at 0.7, tune after field data).
/// A match is assigned only when the best cosine score is strictly above τ.
pub const RECOGNITION_THRESHOLD: f32 = 0.7;

/// A prototype used by the matcher. Owned so the matcher is pure and testable
/// without a database. Convert from `PrototypeRow` (see the `From` impl below).
#[derive(Debug, Clone)]
pub struct Prototype {
    pub speaker_id: String,
    pub channel: String,
    pub embedding: Vec<f32>,
}

impl From<crate::database::repositories::speaker::PrototypeRow> for Prototype {
    fn from(row: crate::database::repositories::speaker::PrototypeRow) -> Self {
        Prototype {
            speaker_id: row.speaker_id,
            channel: row.channel,
            embedding: row.embedding,
        }
    }
}

/// A successful recognition: the matched speaker id and its cosine score.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchResult {
    pub speaker_id: String,
    pub score: f32,
}

/// L2-normalize a vector in place. A zero vector is left untouched (norm 0).
pub fn l2_normalize_in_place(v: &mut [f32]) {
    let mut norm = 0.0f32;
    for x in v.iter() {
        norm += x * x;
    }
    norm = norm.sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// Return an L2-normalized copy of a vector.
pub fn l2_normalize(v: &[f32]) -> Vec<f32> {
    let mut out = v.to_vec();
    l2_normalize_in_place(&mut out);
    out
}

/// Cosine similarity between two vectors. Handles mismatched lengths by
/// comparing the overlapping prefix, and returns 0.0 when either vector has
/// zero magnitude (so identical zero-vectors yield 0.0, not NaN).
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..n {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

/// Find the best-matching speaker for `query` among `prototypes`.
///
/// `query_channel` is the channel the query embedding was captured on
/// ('mic'/'system'), used for the same-channel preference. When `None`, all
/// of each candidate's prototypes are considered (no preference).
///
/// Returns `Some(MatchResult)` only when the best score is strictly above
/// `RECOGNITION_THRESHOLD`; otherwise `None` (the cluster stays anonymous).
pub fn best_match(
    query: &[f32],
    query_channel: Option<&str>,
    prototypes: &[Prototype],
) -> Option<MatchResult> {
    if prototypes.is_empty() {
        return None;
    }

    // Group prototypes by speaker.
    let mut by_speaker: HashMap<&str, Vec<&Prototype>> = HashMap::new();
    for p in prototypes {
        by_speaker.entry(p.speaker_id.as_str()).or_default().push(p);
    }

    let mut best: Option<MatchResult> = None;
    for (speaker_id, protos) in &by_speaker {
        // Same-channel preference: if the candidate has prototypes on the
        // query's channel, consider only those; otherwise use all.
        let same_channel: Vec<&&Prototype> = match query_channel {
            Some(ch) => protos.iter().filter(|p| p.channel == ch).collect(),
            None => Vec::new(),
        };
        let pool: Vec<&&Prototype> = if same_channel.is_empty() {
            protos.iter().collect()
        } else {
            same_channel
        };

        let mut spk_best = f32::MIN;
        for p in &pool {
            let s = cosine_similarity(query, &p.embedding);
            if s > spk_best {
                spk_best = s;
            }
        }

        match best {
            Some(ref b) if spk_best <= b.score => {}
            _ => best = Some(MatchResult {
                speaker_id: speaker_id.to_string(),
                score: spk_best,
            }),
        }
    }

    best.and_then(|m| {
        if m.score > RECOGNITION_THRESHOLD {
            Some(m)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proto(id: &str, channel: &str, emb: Vec<f32>) -> Prototype {
        Prototype {
            speaker_id: id.to_string(),
            channel: channel.to_string(),
            embedding: emb,
        }
    }

    #[test]
    fn cosine_identical_is_one_orthogonal_is_zero() {
        let a = vec![1.0, 0.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &a) - 1.0).abs() < 1e-6);
        assert!(cosine_similarity(&a, &b).abs() < 1e-6);
        // zero vector -> 0.0 (not NaN)
        let z = vec![0.0, 0.0, 0.0, 0.0];
        assert_eq!(cosine_similarity(&z, &z), 0.0);
    }

    #[test]
    fn identical_query_matches_above_threshold() {
        let emb = l2_normalize(&[1.0, 2.0, 3.0, 4.0]);
        let protos = vec![proto("alice", "mic", emb.clone())];
        let m = best_match(&emb, Some("mic"), &protos).expect("identical should match");
        assert_eq!(m.speaker_id, "alice");
        assert!((m.score - 1.0).abs() < 1e-5);
    }

    #[test]
    fn orthogonal_query_does_not_match() {
        let proto_emb = vec![1.0, 0.0, 0.0, 0.0];
        let query = vec![0.0, 1.0, 0.0, 0.0];
        let protos = vec![proto("alice", "mic", proto_emb)];
        assert!(best_match(&query, Some("mic"), &protos).is_none());
    }

    #[test]
    fn empty_candidate_set_returns_none() {
        let query = vec![1.0, 0.0, 0.0, 0.0];
        assert!(best_match(&query, Some("mic"), &[]).is_none());
    }

    #[test]
    fn best_candidate_wins() {
        let query = l2_normalize(&[1.0, 0.0, 0.0, 0.0]);
        // alice is a near-match (0.9), bob is a weak match.
        let alice_emb = l2_normalize(&[0.9, 0.43589, 0.0, 0.0]);
        let bob_emb = l2_normalize(&[0.1, 0.99498, 0.0, 0.0]);
        let protos = vec![
            proto("alice", "mic", alice_emb),
            proto("bob", "mic", bob_emb),
        ];
        let m = best_match(&query, Some("mic"), &protos).expect("should match alice");
        assert_eq!(m.speaker_id, "alice");
        assert!(m.score > 0.7);
    }

    #[test]
    fn threshold_is_strict_above() {
        // cosine(query, proto) == 0.6 (< τ) -> no match
        let query = vec![1.0_f32, 0.0, 0.0, 0.0];
        // proto normalized with x=0.6 -> cosine 0.6
        let proto_emb = l2_normalize(&[0.6, 0.8, 0.0, 0.0]);
        assert_eq!(cosine_similarity(&query, &proto_emb), 0.6);
        let protos = vec![proto("alice", "mic", proto_emb)];
        assert!(
            best_match(&query, Some("mic"), &protos).is_none(),
            "score below threshold must not match"
        );

        // cosine == 0.8 (> τ) -> match
        let proto_emb_hi = l2_normalize(&[0.8, 0.6, 0.0, 0.0]);
        assert!((cosine_similarity(&query, &proto_emb_hi) - 0.8).abs() < 1e-5);
        let protos_hi = vec![proto("alice", "mic", proto_emb_hi)];
        assert!(best_match(&query, Some("mic"), &protos_hi).is_some());
    }

    #[test]
    fn channel_preference_excludes_cross_channel_when_same_channel_exists() {
        let query = vec![1.0, 0.0, 0.0, 0.0];
        // alice has a strong SYSTEM prototype (would match 0.95) and a weak
        // MIC prototype (0.3). Query is MIC -> same-channel preference uses the
        // MIC prototype only -> below threshold -> no match.
        let sys_strong = l2_normalize(&[0.95, 0.31225, 0.0, 0.0]);
        let mic_weak = l2_normalize(&[0.3, 0.95394, 0.0, 0.0]);
        let protos = vec![
            proto("alice", "system", sys_strong),
            proto("alice", "mic", mic_weak),
        ];
        assert!(
            best_match(&query, Some("mic"), &protos).is_none(),
            "same-channel preference must exclude the strong cross-channel prototype"
        );
    }

    #[test]
    fn channel_fallback_when_no_same_channel_prototypes() {
        let query = vec![1.0, 0.0, 0.0, 0.0];
        // alice has only a SYSTEM prototype; query is MIC -> no same-channel
        // prototypes -> fall back to all -> matches the system prototype.
        let sys_strong = l2_normalize(&[0.95, 0.31225, 0.0, 0.0]);
        let protos = vec![proto("alice", "system", sys_strong)];
        let m = best_match(&query, Some("mic"), &protos).expect("fallback should match");
        assert_eq!(m.speaker_id, "alice");
        assert!(m.score > 0.7);
    }

    #[test]
    fn no_channel_query_considers_all_prototypes() {
        let query = vec![1.0, 0.0, 0.0, 0.0];
        let sys_strong = l2_normalize(&[0.95, 0.31225, 0.0, 0.0]);
        let protos = vec![proto("alice", "system", sys_strong)];
        let m = best_match(&query, None, &protos).expect("should match without channel pref");
        assert_eq!(m.speaker_id, "alice");
    }

    #[test]
    fn l2_normalize_unit_vector() {
        let mut v = vec![3.0, 4.0];
        l2_normalize_in_place(&mut v);
        assert!((v[0] - 0.6).abs() < 1e-6);
        assert!((v[1] - 0.8).abs() < 1e-6);
    }
}
