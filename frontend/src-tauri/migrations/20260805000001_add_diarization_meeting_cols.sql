-- Migration: Add diarization tracking columns to meetings table
-- diarization_status: NULL (not run), "processing", "complete", "failed"
-- speaker_names: JSON map of speaker_id -> user_label (e.g., {"SPEAKER_00": "Alice"})

ALTER TABLE meetings ADD COLUMN diarization_status TEXT;
ALTER TABLE meetings ADD COLUMN speaker_names TEXT;
