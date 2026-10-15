## Context

`speaker-turn-display-grouping` (in-flight, 11/12 tasks, not archived) introduced render-only turn merging keyed on the raw cluster label + `source_device`. Two gaps surfaced in real recordings: (1) the same human is often split by diarization into several clusters that each auto-recognize to the same person name — different raw labels, so they do not merge despite one identity; (2) once merged, member records are unreachable in the UI, so per-record relabeling inside a merged turn is impossible — an expand/collapse control was implemented for this and withdrawn on 2026-09-17 (its appearance and position churn were not wanted), so gap (2) stays open and is recorded as a Non-Goal below. The capability `speaker-turn-grouping` is not yet archived, so this change's delta **MODIFIED** targets it and must be archived *after* the display-grouping change.

## Goals / Non-Goals

**Goals:**
- Merging decided by resolved speaker identity (registry person) with fallback to raw cluster label.

**Non-Goals:**
- No change to the backend diarization/clustering itself (merging several clusters into one starts at display level; the registry already supports per-person display names from multiple clusters).
- No expand/collapse control on merged turns (withdrawn 2026-09-17): member records behind a merged turn are not individually reachable in the UI, and only the turn header edits its first member.
- No new speaker-resolution logic: the resolver only reads fields already on the transcript (`speaker`, `speaker_label`, `speaker_matched_by`, `speaker_match_score`).

## Decisions

1. **Identity resolution in the grouping helper, one function, used by both views.**
   `identityKey(row)` returns: (a) 'user'/'auto' with a `speaker_label` value → display binding `"<matched_by>:<label>"` (label is the registry name both cluster labels were matched against — same human ⇒ same label); (b) unresolved → `"<cluster>:<device>"` as today. Key includes `speaker_matched_by` so an auto "Alice" and a user-typed "Alice" do not cross-merge — user intent outranks name equality and splitting there is safe.
   - Alternative: pass registry person ids down from `rematchTranscripts`/`matchSpeakerToTranscript` outputs. Rejected: available data alone already distinguishes the observed failure mode (same auto name, different clusters); threading person ids into display rows touches more layers for no behavior gain.
   - Caveat recorded: two *different* registry people with the identical display name would merge; matches usually carry distinct names, and a true collision resolves via the user relabeling one cluster. Accepted.
2. **Fallback keeps the current key**, so unlabeled clusters and negative evidence behave exactly as the unmodified capability.
3. **Archive ordering**: `speaker-turn-display-grouping` first (establishes `speaker-turn-grouping` main spec), then `identity-turn-grouping` (its MODIFIED applies). This change must not be implemented before its predecessor's capability is archived.

## Risks / Trade-offs

- [Display-name-based identity merges clusters that merely share a name] → mitigated by keeping resolution kind (`user` vs `auto`) in the key; a wrong merge is cosmetic and changes no stored data, but with the expand control withdrawn it can no longer be split from the UI by relabeling a member record (see Non-Goals).
- [Color choice for merged cross-cluster turns] → the turn header dot uses the first member's raw cluster color as today; cross-cluster merged turns show the first cluster's color. Acceptable; a future palette-per-person refinement is out of scope.

## Open Questions

- Per-member relabeling inside a merged turn stays unreachable (Non-Goal). A future affordance other than a row control — for example an inline action on the turn that reveals its member records — would be a new change.
