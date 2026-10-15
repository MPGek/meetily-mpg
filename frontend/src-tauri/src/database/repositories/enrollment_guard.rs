//! Coherence guard for promoting cache rows to prototypes
//! (guard-prototype-enrollment). Pure vector math, no database access.

use crate::audio::diarization::identity::matching::{cosine_similarity, l2_normalize};

/// A candidate whose cosine to the mean of the other candidates is below this
/// is not enrolled. Also the suspect threshold for existing prototypes
/// (design D3): it cuts the clearly foreign samples and leaves the normal
/// intra-speaker spread (p10 around 0.6) alone.
pub const COHERENCE_THRESHOLD: f32 = 0.5;

/// Fewer candidates than this cannot be judged against each other.
const MIN_JUDGED: usize = 3;

/// Cosine of each vector to the mean of all the others. `None` when the set is
/// too small to judge (fewer than [`MIN_JUDGED`] vectors).
pub fn leave_one_out_similarity(embeddings: &[&[f32]]) -> Option<Vec<f32>> {
    if embeddings.len() < MIN_JUDGED {
        return None;
    }
    let dim = embeddings.iter().map(|e| e.len()).max().unwrap_or(0);
    let normalized: Vec<Vec<f32>> = embeddings.iter().map(|e| l2_normalize(e)).collect();
    let mut sum = vec![0.0f32; dim];
    for v in &normalized {
        for (s, x) in sum.iter_mut().zip(v) {
            *s += x;
        }
    }
    Some(
        normalized
            .iter()
            .map(|v| {
                let others: Vec<f32> = sum
                    .iter()
                    .enumerate()
                    .map(|(i, s)| s - v.get(i).copied().unwrap_or(0.0))
                    .collect();
                cosine_similarity(v, &others)
            })
            .collect(),
    )
}

/// Indexes of the candidates that cohere with the rest, in input order.
/// A set too small to judge is returned whole.
pub fn filter_coherent(embeddings: &[&[f32]], threshold: f32) -> Vec<usize> {
    match leave_one_out_similarity(embeddings) {
        None => (0..embeddings.len()).collect(),
        Some(sims) => sims
            .iter()
            .enumerate()
            .filter(|(_, s)| **s >= threshold)
            .map(|(i, _)| i)
            .collect(),
    }
}

/// Walk a duration-ordered pool and take the first `k` rows that pass the
/// guard. Returns the picked indexes and how many pool rows ahead of the last
/// pick were dropped as incoherent.
pub fn pick_coherent(embeddings: &[&[f32]], k: usize) -> (Vec<usize>, usize) {
    let passing = filter_coherent(embeddings, COHERENCE_THRESHOLD);
    let picked: Vec<usize> = passing.into_iter().take(k).collect();
    let reach = picked.last().map(|i| i + 1).unwrap_or(embeddings.len());
    let dropped = reach - picked.len();
    (picked, dropped)
}

/// Result of assessing one enrolled prototype against its owner's other
/// prototypes and against everybody else's (design D5).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrototypeAssessment {
    /// Cosine to the mean of the owner's other prototypes.
    pub own_similarity: f32,
    pub suspect: bool,
}

/// Assess enrolled prototypes given as `(row id, speaker id, embedding)`.
/// Only owners with at least [`MIN_JUDGED`] prototypes are assessed. A
/// prototype is suspect when its own similarity is below
/// [`COHERENCE_THRESHOLD`], or when it is closer to another owner's mean than
/// to its own. Pure: nothing is written anywhere.
pub fn assess_prototypes(
    items: &[(String, String, Vec<f32>)],
) -> std::collections::HashMap<String, PrototypeAssessment> {
    use std::collections::HashMap;
    let mut by_owner: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, (_, owner, _)) in items.iter().enumerate() {
        by_owner.entry(owner.as_str()).or_default().push(i);
    }
    let normalized: Vec<Vec<f32>> = items.iter().map(|(_, _, e)| l2_normalize(e)).collect();
    // Sum of normalized prototypes per owner; cosine ignores the scale, so a
    // sum stands in for the mean.
    let sums: HashMap<&str, Vec<f32>> = by_owner
        .iter()
        .map(|(owner, idx)| {
            let dim = idx.iter().map(|&i| normalized[i].len()).max().unwrap_or(0);
            let mut sum = vec![0.0f32; dim];
            for &i in idx {
                for (s, x) in sum.iter_mut().zip(&normalized[i]) {
                    *s += x;
                }
            }
            (*owner, sum)
        })
        .collect();

    let mut out = HashMap::new();
    for (owner, idx) in &by_owner {
        if idx.len() < MIN_JUDGED {
            continue;
        }
        let own_sum = &sums[owner];
        for &i in idx {
            let v = &normalized[i];
            let others_of_owner: Vec<f32> = own_sum
                .iter()
                .enumerate()
                .map(|(k, s)| s - v.get(k).copied().unwrap_or(0.0))
                .collect();
            let own = cosine_similarity(v, &others_of_owner);
            let nearest_other = sums
                .iter()
                .filter(|(o, _)| *o != owner)
                .map(|(_, sum)| cosine_similarity(v, sum))
                .fold(f32::MIN, f32::max);
            out.insert(
                items[i].0.clone(),
                PrototypeAssessment {
                    own_similarity: own,
                    suspect: own < COHERENCE_THRESHOLD || nearest_other > own,
                },
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(angle: f32) -> Vec<f32> {
        vec![angle.cos(), angle.sin(), 0.0]
    }

    fn refs(v: &[Vec<f32>]) -> Vec<&[f32]> {
        v.iter().map(|e| e.as_slice()).collect()
    }

    #[test]
    fn an_outlier_among_seven_coherent_vectors_is_dropped() {
        let mut set: Vec<Vec<f32>> = (0..7).map(|i| unit(0.05 * i as f32)).collect();
        set.push(vec![0.0, 0.0, 1.0]);
        let kept = filter_coherent(&refs(&set), COHERENCE_THRESHOLD);
        assert_eq!(kept, vec![0, 1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn fewer_than_three_inputs_are_not_judged() {
        let set = vec![vec![1.0, 0.0, 0.0], vec![0.0, 0.0, 1.0]];
        assert_eq!(filter_coherent(&refs(&set), COHERENCE_THRESHOLD), vec![0, 1]);
    }

    #[test]
    fn identical_vectors_are_all_kept() {
        let set = vec![vec![0.3, 0.4, 0.5]; 5];
        assert_eq!(filter_coherent(&refs(&set), COHERENCE_THRESHOLD).len(), 5);
    }

    #[test]
    fn a_zero_threshold_keeps_the_outlier() {
        // Mutation guard: the drop in the first test comes from the threshold.
        let mut set: Vec<Vec<f32>> = (0..7).map(|i| unit(0.05 * i as f32)).collect();
        set.push(vec![0.0, 0.0, 1.0]);
        assert_eq!(filter_coherent(&refs(&set), 0.0).len(), 8);
    }

    #[test]
    fn pick_takes_k_passing_rows_and_counts_the_drops_ahead_of_them() {
        // Pool order is duration order: row 1 is the outlier, ahead of the cut.
        let mut set: Vec<Vec<f32>> = (0..6).map(|i| unit(0.04 * i as f32)).collect();
        set.insert(1, vec![0.0, 0.0, 1.0]);
        let (picked, dropped) = pick_coherent(&refs(&set), 4);
        assert_eq!(picked, vec![0, 2, 3, 4]);
        assert_eq!(dropped, 1);
    }

    #[test]
    fn pick_does_not_count_rows_past_the_cut() {
        let mut set: Vec<Vec<f32>> = (0..6).map(|i| unit(0.04 * i as f32)).collect();
        set.push(vec![0.0, 0.0, 1.0]);
        let (picked, dropped) = pick_coherent(&refs(&set), 4);
        assert_eq!(picked.len(), 4);
        assert_eq!(dropped, 0);
    }

    fn proto(id: &str, owner: &str, v: Vec<f32>) -> (String, String, Vec<f32>) {
        (id.to_string(), owner.to_string(), v)
    }

    #[test]
    fn a_prototype_nearer_to_another_speaker_is_suspect() {
        let mut items: Vec<_> = (0..4)
            .map(|i| proto(&format!("a{i}"), "alice", unit(0.02 * i as f32)))
            .collect();
        items.extend((0..4).map(|i| proto(&format!("b{i}"), "bob", vec![0.0, 1.0, 0.1 * i as f32])));
        // Filed under Alice, but it sounds like Bob.
        items.push(proto("odd", "alice", vec![0.0, 1.0, 0.15]));
        let r = assess_prototypes(&items);
        assert!(r["odd"].suspect);
        assert!(r["odd"].own_similarity < COHERENCE_THRESHOLD);
        assert!(!r["a0"].suspect && !r["b1"].suspect);
    }

    #[test]
    fn an_owner_with_two_prototypes_is_not_assessed() {
        let items = vec![
            proto("a0", "alice", vec![1.0, 0.0, 0.0]),
            proto("a1", "alice", vec![0.0, 1.0, 0.0]),
        ];
        assert!(assess_prototypes(&items).is_empty());
    }

    #[test]
    fn coherent_prototypes_are_not_suspect() {
        let items: Vec<_> = (0..5)
            .map(|i| proto(&format!("a{i}"), "alice", unit(0.03 * i as f32)))
            .chain((0..5).map(|i| proto(&format!("b{i}"), "bob", vec![0.0, 0.3, 1.0 + 0.02 * i as f32])))
            .collect();
        let r = assess_prototypes(&items);
        assert_eq!(r.len(), 10);
        assert!(r.values().all(|a| !a.suspect));
    }


    /// Manual measurement for guard-prototype-enrollment 4.1: run the shipped
    /// browser listing against a *copy* of a real database (opened read-only)
    /// and print how many prototypes it flags. Gated on `MEETILY_VERIFY_DB`,
    /// so it skips everywhere else.
    #[tokio::test]
    async fn suspect_count_on_a_real_database() {
        let Ok(db) = std::env::var("MEETILY_VERIFY_DB") else {
            eprintln!("skipping: set MEETILY_VERIFY_DB (a copy!)");
            return;
        };
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!("sqlite:{db}?mode=ro"))
            .await
            .expect("open the database copy");
        let browser = crate::database::repositories::speaker::SpeakerRepository::list_voiceprints(
            &pool, None, false, None, None,
        )
        .await
        .unwrap();
        let rows: Vec<_> = browser.speakers.iter().flat_map(|s| &s.prototypes).collect();
        let assessed = rows.iter().filter(|r| r.own_similarity.is_some()).count();
        let suspect = rows.iter().filter(|r| r.suspect).count();
        let low = rows
            .iter()
            .filter(|r| r.own_similarity.is_some_and(|s| s < COHERENCE_THRESHOLD))
            .count();
        eprintln!(
            "prototypes={} assessed={} suspect={} below_threshold={}",
            rows.len(),
            assessed,
            suspect,
            low
        );
        for s in browser.speakers.iter().filter(|s| s.suspect_count > 0) {
            eprintln!("  {}: {} suspect of {}", s.speaker_name, s.suspect_count, s.prototype_count);
        }
        assert!(suspect <= assessed);
    }

}
