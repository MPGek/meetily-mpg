-- Migration: Add speaker identity registry (change: speaker-identity-registry)
-- Adds four tables per design D1:
--   speakers                    - global person registry (cross-meeting identity)
--   speaker_embeddings          - voiceprint storage (one table, two owners)
--   meeting_speakers            - per-meeting cluster -> person mapping + centroid
--   meeting_expected_speakers   - per-meeting recognition allowlist
-- Legacy meetings.speaker_names JSON and transcripts.speaker_label are retained
-- as display fallback only; new flows do not write them.

-- Global speaker registry. Names are unique case-insensitively (index below).
CREATE TABLE IF NOT EXISTS speakers (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    is_me INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Case-insensitive uniqueness on speaker names.
CREATE UNIQUE INDEX IF NOT EXISTS idx_speakers_name_nocase
    ON speakers(name COLLATE NOCASE);

-- Voiceprint storage. Each row is owned either by a registry speaker
-- (speaker_id set: enrolled prototype) or by a meeting cluster
-- (meeting_id + cluster_label set: unassigned cache). The CHECK constraint
-- guarantees exactly one owner kind. Caches are retained indefinitely.
CREATE TABLE IF NOT EXISTS speaker_embeddings (
    id TEXT PRIMARY KEY,
    embedding BLOB NOT NULL,
    model TEXT NOT NULL,
    channel TEXT NOT NULL CHECK (channel IN ('mic', 'system')),
    duration_secs REAL NOT NULL DEFAULT 0.0,
    speaker_id TEXT,
    meeting_id TEXT,
    cluster_label TEXT,
    created_at TEXT NOT NULL,
    FOREIGN KEY (speaker_id) REFERENCES speakers(id) ON DELETE CASCADE,
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE,
    CHECK (
        (speaker_id IS NOT NULL AND meeting_id IS NULL AND cluster_label IS NULL)
        OR
        (speaker_id IS NULL AND meeting_id IS NOT NULL AND cluster_label IS NOT NULL)
    )
);

-- Recognition reads prototypes by speaker + model tag.
CREATE INDEX IF NOT EXISTS idx_speaker_embeddings_speaker
    ON speaker_embeddings(speaker_id, model);
-- Enrollment reparents cache rows by meeting + cluster label.
CREATE INDEX IF NOT EXISTS idx_speaker_embeddings_cache
    ON speaker_embeddings(meeting_id, cluster_label);

-- Per-meeting cluster -> person mapping. Centroid is the recognition target.
-- `channel` records which audio channel the cluster came from ('mic'/'system')
-- so re-match can apply the same-channel prototype preference; NULL for legacy
-- meetings with no diarization cache.
-- matched_by: 'auto' (recognized above threshold), 'user' (manual edit), or NULL.
CREATE TABLE IF NOT EXISTS meeting_speakers (
    meeting_id TEXT NOT NULL,
    cluster_label TEXT NOT NULL,
    speaker_id TEXT,
    centroid BLOB,
    channel TEXT CHECK (channel IS NULL OR channel IN ('mic', 'system')),
    matched_by TEXT CHECK (matched_by IN ('auto', 'user') OR matched_by IS NULL),
    match_score REAL,
    PRIMARY KEY (meeting_id, cluster_label),
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE,
    FOREIGN KEY (speaker_id) REFERENCES speakers(id) ON DELETE SET NULL
);

-- Expected-speaker allowlist per meeting. Empty set = match against ALL speakers.
CREATE TABLE IF NOT EXISTS meeting_expected_speakers (
    meeting_id TEXT NOT NULL,
    speaker_id TEXT NOT NULL,
    PRIMARY KEY (meeting_id, speaker_id),
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE,
    FOREIGN KEY (speaker_id) REFERENCES speakers(id) ON DELETE CASCADE
);
