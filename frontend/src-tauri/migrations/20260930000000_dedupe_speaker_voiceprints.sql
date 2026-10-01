-- Migration: collapse duplicate voiceprints of a person
-- (change: fix-live-subrow-assignment-scope, design D5)
--
-- Stop-time ground-truth enrollment ran once per live sub-row override, and
-- a ~20 s chunk embedding is shared by every short sub-row inside it, so the
-- same chunk was stored for a person once per click. Keep one row per
-- (speaker, meeting, channel, audio window, embedding): the verified copy if
-- any, then one with a clip, then the oldest. The kept row inherits a clip
-- from a removed copy when it has none. Unassigned cache rows
-- (speaker_id IS NULL) are not a person's voiceprints and are left alone.
--
-- Enrollment applies the same rule per speaker from now on
-- (SpeakerRepository::collapse_duplicate_prototypes).

WITH ranked AS (
    SELECT id, audio_blob, audio_codec, audio_sample_rate,
           ROW_NUMBER() OVER w AS rn,
           FIRST_VALUE(id) OVER w AS keep_id
    FROM speaker_embeddings
    WHERE speaker_id IS NOT NULL
    WINDOW w AS (
        PARTITION BY speaker_id, meeting_id, channel, audio_start_time, audio_end_time, embedding
        ORDER BY is_verified DESC, (audio_blob IS NOT NULL) DESC, created_at ASC, id ASC
    )
),
donor AS (
    SELECT keep_id, audio_blob, audio_codec, audio_sample_rate,
           ROW_NUMBER() OVER (PARTITION BY keep_id ORDER BY rn) AS dn
    FROM ranked WHERE rn > 1 AND audio_blob IS NOT NULL
)
UPDATE speaker_embeddings
SET audio_blob = donor.audio_blob,
    audio_codec = donor.audio_codec,
    audio_sample_rate = donor.audio_sample_rate
FROM donor
WHERE speaker_embeddings.id = donor.keep_id
  AND donor.dn = 1
  AND speaker_embeddings.audio_blob IS NULL;

WITH ranked AS (
    SELECT id,
           ROW_NUMBER() OVER (
               PARTITION BY speaker_id, meeting_id, channel, audio_start_time, audio_end_time, embedding
               ORDER BY is_verified DESC, (audio_blob IS NOT NULL) DESC, created_at ASC, id ASC
           ) AS rn
    FROM speaker_embeddings
    WHERE speaker_id IS NOT NULL
)
DELETE FROM speaker_embeddings WHERE id IN (SELECT id FROM ranked WHERE rn > 1);
