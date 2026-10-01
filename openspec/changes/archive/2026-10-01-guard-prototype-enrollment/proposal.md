# Proposal

## Why

A measurement on a copy of the user's database (2026-10-01, 16 speakers, 151 prototypes, 2331 cache rows) showed
that unconfirmed caches are mostly clean (median cosine to their cluster centroid 0.85, longest exemplars the
most coherent) and are never read across meetings, but enrollment copies cache rows into prototypes with no
coherence check. 25 of 151 prototypes (17%) are closer to another speaker's centroid than to their own person's,
including 8-23 s blocks (Artsiom Karane 0.41 vs Dmitry K. 0.84). Prototypes take part in every future
recognition, so a wrong one degrades every later diarization, and nothing in the product lets the user find it.

## What Changes

- Enrollment (cluster-wide, single block, and buffer paths) drops candidate rows that do not cohere with the
  rest of the candidate set, before they become prototypes. Dropped rows stay in the unconfirmed cache.
- The voiceprint browser marks prototypes that look suspect (low similarity to the person's other prototypes, or
  nearer to another person's voiceprint) and can filter to them, so the user can verify or reject them. Nothing is
  removed automatically.
- No change to recognition thresholds, to cache writing, or to what users can enroll by hand: a user-confirmed
  prototype is never silently removed.

## Capabilities

### New Capabilities
- `voiceprint-enrollment-quality`: coherence guard applied when cache rows are promoted to prototypes.

### Modified Capabilities
- `voiceprint-review`: suspect-prototype indicator and filter in the voiceprint browser.

## Impact

- `frontend/src-tauri/src/database/repositories/speaker.rs`: `enroll_cluster`, `enroll_block_window`,
  `enroll_embeddings_from_buffer`, `list_voiceprints` (new `suspect` data on `VoiceprintRow`).
- `frontend/src/components/VoiceprintBrowser.tsx` and its typed model: badge + filter.
- No migration, no new table, no model change. Existing prototypes are only flagged, never altered.
- Not in scope: stale-cache cleanup on re-diarization (3 of 18 meetings) and orphan caches without a
  `meeting_speakers` row (6 meetings) - separate, small effect.
