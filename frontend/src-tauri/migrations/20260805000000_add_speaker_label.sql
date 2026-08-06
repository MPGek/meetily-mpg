-- Migration: Add speaker_label column for user-assigned speaker names
-- Complements the existing `speaker` column (from migration 20251110000001_add_speaker_field.sql)
-- which stores diarization-assigned IDs (e.g., "SPEAKER_00")

ALTER TABLE transcripts ADD COLUMN speaker_label TEXT;
