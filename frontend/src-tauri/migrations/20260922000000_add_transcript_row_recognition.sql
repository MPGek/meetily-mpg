-- Migration: per-row automatic speaker recognition
-- (change: per-row-speaker-recognition, design D2)
--
-- A live session names every emitted turn from that turn's own audio, but at
-- recording stop the name came from one match per cluster, so a cluster that
-- merged several voices renamed every row it covered. These two nullable
-- columns hold the match computed from the embeddings that overlap the row
-- itself, which display resolution prefers over the cluster binding (but
-- never over a user decision).
--
-- Additive and not backfilled: NULL means "no row-level match, resolve via
-- the cluster", which is exactly how every existing row behaves today. No FK
-- enforcement, in line with the rest of this schema.
ALTER TABLE transcripts ADD COLUMN speaker_auto_id TEXT;
ALTER TABLE transcripts ADD COLUMN speaker_auto_score REAL;

-- Display resolution joins speakers by this column for every row of a
-- meeting, alongside the existing override join.
CREATE INDEX IF NOT EXISTS idx_transcripts_speaker_auto
    ON transcripts(meeting_id, speaker_auto_id);
