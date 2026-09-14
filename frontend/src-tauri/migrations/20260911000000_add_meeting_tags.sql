-- Migration: Meeting tags dictionary + many-to-many links
-- Change: meeting-notes-list-display-tags
-- New global `meeting_tags` dictionary (name unique NOCASE, fixed-palette
-- color key) and `meeting_tag_links` join table. Existing meetings get zero
-- tags implicitly (no backfill). Forward-only (SQLx migrate); rollback =
-- previous build ignores the new tables; meetings data untouched.

CREATE TABLE IF NOT EXISTS meeting_tags (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    color TEXT NOT NULL DEFAULT 'blue',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_meeting_tags_name_nocase
    ON meeting_tags(name COLLATE NOCASE);

CREATE TABLE IF NOT EXISTS meeting_tag_links (
    meeting_id TEXT NOT NULL,
    tag_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (meeting_id, tag_id),
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE,
    FOREIGN KEY (tag_id) REFERENCES meeting_tags(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_meeting_tag_links_meeting
    ON meeting_tag_links(meeting_id);

CREATE INDEX IF NOT EXISTS idx_meeting_tag_links_tag
    ON meeting_tag_links(tag_id);
