-- Migration: Add source_device column for transcript source tracking
-- This enables distinguishing between microphone and system audio transcripts
-- Nullable to support legacy meetings recorded before this feature

ALTER TABLE transcripts ADD COLUMN source_device TEXT;
