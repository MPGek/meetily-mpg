-- Migration: Add voiceprint provenance to speaker_embeddings
-- Change: voiceprint-provenance-and-review
-- Adds audio_start_time / audio_end_time, relaxes ownership CHECK to allow
-- a prototype (speaker_id IS NOT NULL) to carry any provenance combination,
-- preserves legacy rows (NULL provenance), recreates existing indexes and
-- adds idx_speaker_embeddings_cache_speaker.
-- SQLite cannot ALTER a CHECK, so the table is rebuilt via the 12-step
-- procedure (https://www.sqlite.org/lang_altertable.html).

PRAGMA foreign_keys=OFF;

-- New table with updated schema
CREATE TABLE speaker_embeddings_new (
    id TEXT PRIMARY KEY,
    embedding BLOB NOT NULL,
    model TEXT NOT NULL,
    channel TEXT NOT NULL CHECK (channel IN ('mic', 'system')),
    duration_secs REAL NOT NULL DEFAULT 0.0,
    speaker_id TEXT,
    meeting_id TEXT,
    cluster_label TEXT,
    audio_start_time REAL,
    audio_end_time REAL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (speaker_id) REFERENCES speakers(id) ON DELETE CASCADE,
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE,
    CHECK (
        (speaker_id IS NULL AND meeting_id IS NOT NULL AND cluster_label IS NOT NULL)
        OR
        (speaker_id IS NOT NULL)
    )
);

-- Copy existing rows: new provenance columns default to NULL (legacy rows remain valid)
INSERT INTO speaker_embeddings_new (id, embedding, model, channel, duration_secs, speaker_id, meeting_id, cluster_label, audio_start_time, audio_end_time, created_at)
    SELECT id, embedding, model, channel, duration_secs, speaker_id, meeting_id, cluster_label, NULL, NULL, created_at
    FROM speaker_embeddings
    ORDER BY id;

DROP TABLE speaker_embeddings;

ALTER TABLE speaker_embeddings_new RENAME TO speaker_embeddings;

-- Recreate the two existing indexes
CREATE INDEX IF NOT EXISTS idx_speaker_embeddings_speaker
    ON speaker_embeddings(speaker_id, model);
CREATE INDEX IF NOT EXISTS idx_speaker_embeddings_cache
    ON speaker_embeddings(meeting_id, cluster_label);
-- Additional index for per-meeting / per-speaker review queries
CREATE INDEX IF NOT EXISTS idx_speaker_embeddings_cache_speaker
    ON speaker_embeddings(meeting_id, cluster_label, speaker_id);

PRAGMA foreign_keys=ON;
