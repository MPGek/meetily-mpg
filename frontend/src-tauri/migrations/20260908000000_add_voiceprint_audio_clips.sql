-- Migration: Add self-contained audio clips + verification flag to speaker_embeddings
-- Change: voiceprint-audio-clips-and-verification
-- Adds audio_blob (Opus mono Ogg bytes), audio_codec, audio_sample_rate,
-- is_verified (0/1) and verified_at. Existing rows become legacy rows
-- (NULL blob, unverified) and keep working via the legacy playback path.
-- SQLite cannot ALTER a CHECK, but the CHECK is unchanged here, so plain
-- ADD COLUMN statements are sufficient (no table rebuild needed).
-- Forward-only (SQLx migrate!); rollback = previous build ignores new columns.

ALTER TABLE speaker_embeddings ADD COLUMN audio_blob BLOB;
ALTER TABLE speaker_embeddings ADD COLUMN audio_codec TEXT DEFAULT 'opus';
ALTER TABLE speaker_embeddings ADD COLUMN audio_sample_rate INTEGER DEFAULT 16000;
ALTER TABLE speaker_embeddings ADD COLUMN is_verified INTEGER NOT NULL DEFAULT 0;
ALTER TABLE speaker_embeddings ADD COLUMN verified_at TEXT;

CREATE INDEX IF NOT EXISTS idx_speaker_embeddings_has_audio
    ON speaker_embeddings(id) WHERE audio_blob IS NOT NULL;
