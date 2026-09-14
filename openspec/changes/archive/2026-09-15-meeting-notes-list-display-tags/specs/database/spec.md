## ADDED Requirements

### Requirement: Meeting tag tables with cascade cleanup

The system SHALL persist meeting tags in `meeting_tags` (`id`, `name`, `color`, timestamps) and links in `meeting_tag_links` (`meeting_id`, `tag_id`), with a case-insensitive unique index on tag name, a composite primary key on the link, foreign keys to `meetings` and `meeting_tags`, and deletion of a meeting SHALL delete its link rows while leaving dictionary entries intact.

#### Scenario: Migration creates tables

- **WHEN** the app launches on a database without tag tables
- **THEN** migration creates `meeting_tags` and `meeting_tag_links` with the unique name index and link primary key, and existing meetings remain readable with zero tags.

#### Scenario: Meeting delete cascades links only

- **WHEN** a meeting with two tag links is deleted via the meeting deletion path
- **THEN** its rows in `meeting_tag_links` are removed, the `meeting_tags` rows survive, and no orphan link rows remain.

#### Scenario: Duplicate link impossible

- **WHEN** the same (`meeting_id`, `tag_id`) pair is inserted twice
- **THEN** the database rejects the second insert via the composite primary key.

### Requirement: Meetings list query returns timestamps and tags

The meetings list read path SHALL return each meeting's `created_at` timestamp and its assigned tags (id, name, color) alongside `id` and `title`, ordered by `created_at` descending.

#### Scenario: List includes new fields

- **WHEN** the frontend requests the meetings list
- **THEN** each entry includes `created_at` and a `tags` array (possibly empty) without dropping `id` or `title`.

#### Scenario: Legacy rows without tags

- **WHEN** a meeting created before the tags feature is listed
- **THEN** it returns an empty `tags` array and a valid `created_at` instead of an error.
