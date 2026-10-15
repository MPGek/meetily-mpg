## Why

A manual single-block speaker correction enrolls the **whole diarization cluster's** longest empirical embeddings into the chosen speaker, not the audio of the block the user corrected. On a mixed cluster — the common case when offline diarization produces one cluster covering several people — this stamps other people's voices onto the speaker's global voiceprint set, poisoning their recognition in every future meeting. Observed on `Meeting 2026-09-14_15-01`: correcting one `SPEAKER_02` block to "Alex Shingel" enrolled 8 prototypes, 7 of which the user had manually labeled as Katsiaryna, Marina, and Dmitry.

The main specs also contradict each other: "Speaker enrollment on assignment" and "Enrollment is wired into the single-block correction path" require enrolling the embeddings that cover the corrected block, while "Per-block speaker override" and the editor scenario demand that block-level assignment enroll nothing. The implementation resolved the contradiction in the worst way — it enrolls the cluster in full.

## What Changes

- A single-block speaker correction SHALL enroll only the block's cluster exemplars whose time window overlaps the corrected block, on that block's capture channel, capped at K=8 — never the cluster's whole top-K by duration.
- "Apply to all blocks of this speaker" and any other cluster-wide binding keep whole-cluster enrollment, and additionally SHALL NOT leave prototypes previously enrolled from that same cluster and channel bound to a different speaker: such rows are demoted back to unassigned cache (unless pinned by a current per-block override for that speaker).
- Channel separation is enforced during enrollment: the channel SHALL be derived from the corrected block's source device and only same-channel exemplars SHALL be enrolled or cleaned up.
- The contradictory `speaker-identity-registry` and `speaker-correction` requirements SHALL be reconciled so block-level assignment neither modifies `meeting_speakers` nor enrolls the cluster, while still enrolling the block-covering embeddings.
- Non-goal: retroactive repair of voiceprints already contaminated by past releases. Existing wrong prototypes remain until the user rejects them in the Voiceprint Browser or re-corrects the affected blocks.

## Capabilities

### New Capabilities
- None.

### Modified Capabilities
- `speaker-identity-registry`: single-block enrollment is scoped to the corrected block's time window and channel instead of the whole cluster; cluster-wide re-binding cleans up stale prototypes originating from the same cluster; the per-block-override and editor requirements stop forbidding enrollment outright.
- `speaker-correction`: inline block correction enrolls only the embeddings covering that block and never contaminates the chosen speaker with sibling speakers' audio from a mixed cluster.

## Impact

- Backend: `database/speaker_commands.rs` (`assign_block_speaker`, `assign_speaker`, `apply_block_speaker_to_cluster`), `database/repositories/speaker.rs` (`enroll_cluster` call sites; a window- and channel-scoped enrollment plus stale-prototype cleanup). The online finalize path (`audio/recording_commands.rs`) already enrolls single-turn corrections by time overlap and only needs the cluster-wide cleanup for live rebinds.
- Data: `speaker_embeddings` gains correctly scoped prototypes and loses cross-cluster contamination; `meeting_speakers` semantics are unchanged. No schema change.
- Affected recordings: `Meeting 2026-09-14_15-01` (`SPEAKER_02` → Alex Shingel contaminated with Katsiaryna/Marina/Dmitry audio); other meetings where a multi-person cluster was corrected block-by-block are affected the same way.
- No new external dependencies; no API surface changes beyond existing command behavior.
