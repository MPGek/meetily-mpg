-- Token-level timestamps for refined speaker assignment (diarization-accuracy-upgrade)
-- When Whisper provides token timestamps, offline and online diarization split cross-speaker segments into N rows.
-- Fallback remains segment-level overlap when NULL.
ALTER TABLE transcripts ADD COLUMN tokens TEXT;
