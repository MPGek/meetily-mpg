## Why

Voiceprints are invisible and unverifiable. Enrolled prototypes lose every trace of where they came from — `enroll_cluster` nulls `meeting_id`/`cluster_label` on reparenting — and even unassigned caches store no timecodes at all. There is no way to audit what a voiceprint represents, hear the audio it was extracted from, see the unconfirmed caches, or fix a mis-enrolled voiceprint without deleting it blindly. This change records full provenance for every embedding, exposes all of them in Settings, and adds reject/reconfirm with global speaker replacement.

## What Changes

- **Provenance on every embedding**: `speaker_embeddings` gains `audio_start_time`/`audio_end_time`; a prototype no longer has its `meeting_id`/`cluster_label` nulled on enrollment, so every row (confirmed prototype or unconfirmed cache) remembers the meeting, cluster, channel, and source time range it came from.
- **Timecode threading**: both diarization paths populate start/end at write time — offline already has `RawSegment`/`DiarizationSegment` start/end but drops them when building `ClusteredEmbedding`; online already holds `(start, end, embedding)` triples. These are carried through to the stored rows.
- **Retrofit-safe schema change**: existing rows have no timecodes, so provenance columns are nullable and legacy rows are preserved without fabricated data.
- **Voiceprint review page in Settings**: a tree/table of every confirmed speaker and their prototypes plus every unconfirmed cache (grouped by meeting), with **collapsible speaker and meeting groups and expand-all / collapse-all controls**, with channel, duration, meeting, time range, and ownership shown, and a **play button that streams the original audio clip** (reusing the existing asset-protocol player) for rows with provenance.
- **Reject / reconfirm with global replacement**: reject a confirmed voiceprint (demotes it back to an unconfirmed cache, or removes it), reconfirm a cache as a speaker via a person-picker, and — when an identity should change — replace the speaker across the whole corpus: auto-matched clusters and transcripts re-map to a chosen replacement (or to anonymous) chosen through **the same person-picker dialog as speaker name change / assignment** (searchable list of existing persons, quick filter, and create-new), while user bindings and per-block overrides are preserved. Works for both confirmed prototypes and cached embeddings.
- **Storage stats corrected + expanded** so prototypes keeping provenance no longer inflate the cache count.

## Capabilities

### New Capabilities
- `voiceprint-provenance`: storing meeting + channel + time-range provenance on every embedding row (confirmed and cached), threading timecodes through both diarization paths, and preserving provenance through enrollment.
- `voiceprint-review`: Settings voiceprint browser — tree/table of speakers and unconfirmed caches with per-embedding provenance and original-audio-clip playback.
- `voiceprint-rejection`: reject and reconfirm live embeddings (prototype ↔ cache) with optional whole-corpus speaker replacement that re-maps auto-assigned transcripts/clusters while preserving user bindings.

### Modified Capabilities
- `speaker-identity-registry`: voiceprint storage requirement changes — provenance columns, relaxed ownership CHECK (a prototype may also carry meeting/cluster provenance), enrollment no longer nulls provenance, meeting deletion deletes only unassigned caches; speaker enrollment requirement keeps provenance.

## Impact

- **DB**: new SQLite migration for `speaker_embeddings` (2 columns + ownership CHECK relaxation via table rebuild; `meeting_id` FK behavior stays manual since FK enforcement is off).
- **Rust core**:
  - `database/models.rs` — `SpeakerEmbedding`, `Exemplar`, `ClusteredEmbedding` gain timecode fields.
  - `database/repositories/speaker.rs` — `write_cluster_cache` / `enroll_cluster` / `enroll_embeddings_from_buffer` preserve provenance; `load_prototypes` unchanged (still `speaker_id IS NOT NULL AND model = ?`); `storage_stats` cache/prototype counts disambiguated by `speaker_id`; new queries for the review browser, rejection, and global re-map.
  - `audio/diarization.rs` — thread `DiarizationSegment` start/end into the two `ClusteredEmbedding` build sites (line ~796 and ~1143).
  - `audio/online_diarization.rs` — thread `(start, end)` into `ClusteredEmbedding` build sites (~854, ~872) and cache write.
  - `database/repositories/meeting.rs` — meeting deletion deletes only unassigned caches (`speaker_id IS NULL`), not provenance-carrying prototypes (line 376).
- **Tauri commands** (`database/speaker_commands.rs` + `lib.rs` registration): list voiceprints (per-speaker / unconfirmed), reject, reconfirm, replace-across-corpus; `get_meeting_audio_path` reused for clip playback.
- **Frontend**: new Settings section for voiceprint review (grouping tree with collapsible groups and expand/collapse-all controls, provenance columns, play-from-range using the existing audio player hook), reject/reconfirm/confirm dialogs that reuse the shared person-picker component (search/create), storage stats display.
- **Specs**: new `voiceprint-provenance`, `voiceprint-review`, `voiceprint-rejection`; delta to `speaker-identity-registry`.
