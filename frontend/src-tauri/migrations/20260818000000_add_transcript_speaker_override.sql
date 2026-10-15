-- Migration: Add per-transcript speaker override (change: speaker-identity-registry, design D10)
-- Adds a nullable direct link from a transcript row to a registry speaker,
-- used for single-block relabels. Takes precedence over the cluster mapping
-- (meeting_speakers) at display resolution. No FK enforcement (in line with
-- the rest of this codebase); NULL means "no override, resolve via cluster".
ALTER TABLE transcripts ADD COLUMN speaker_override_id TEXT;

-- Display resolution joins by this column (override speaker first), so an
-- index helps meeting-scoped transcript queries.
CREATE INDEX IF NOT EXISTS idx_transcripts_speaker_override
    ON transcripts(meeting_id, speaker_override_id);