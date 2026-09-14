-- Migration: recording start time (change: recording-start-time)
-- `started_at` = when the recording began; `created_at` keeps meaning when
-- the DB row was created (stop/import time). Backfill is an honest
-- approximation: pre-feature rows only know their stop time.

ALTER TABLE meetings ADD COLUMN started_at TEXT;

UPDATE meetings SET started_at = created_at WHERE started_at IS NULL;
