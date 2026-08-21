## Context

See proposal.md — Why. Verified current behavior (research complete):

- **Inline corrections don't enroll.** The offline single-block path calls `assign_block_speaker` (`speaker_commands.rs`), which only sets `transcripts.speaker_override_id` via `set_transcript_override` (`speaker.rs:1006`) — no enrollment. Only `assign_speaker`/`apply_block_speaker_to_cluster` call `enroll_cluster` (reparenting cached exemplars). The live single-block path enrolls only at stop-time `finalize_online_session` via `enroll_embeddings_from_buffer`, gated on a captured `TurnOverride` with valid times and on `assign_live_speaker` actually reaching a live store.
- **Live labels revert.** `useRecordingStop.ts:274` overwrites every fresh transcript's `speaker` with the predicted cluster label before saving; `save_transcript` writes only `transcripts.speaker` (the prediction). The user's name is restored only indirectly via `finalize_online_session` → `meeting_speakers`/`speaker_override_id`, re-resolved by the render-time `COALESCE(so.name, s.name, t.speaker_label)` join (`meeting.rs:13`). `assign_live_speaker` silently no-ops (`warn!`) when no prototype store is active (`recording_commands.rs:1824`).
- **Suffix staleness.** Offline local updater `usePaginatedTranscripts.updateSpeakerLabel` (`:185`) writes only `speaker_label` locally, leaving `speaker_matched_by='auto'` + `match_score` so `formatSpeakerDisplay` keeps rendering `(auto) xx%`. The live updater `applyLiveSpeakerLabel` (`TranscriptContext.tsx:643`) already clears those fields. Both updaters short-circuit on `speaker_label === name`, so confirming the same name is a silent no-op.
- Two enrollment mechanisms already exist: `enroll_cluster` (**reparent** cache rows; works offline where caches exist) and `enroll_embeddings_from_buffer` (**insert** from a raw time-windowed buffer; works live where `mic_raw`/`sys_raw` exist).

## Goals / Non-Goals

**Goals:**
- Make every inline correction (offline single-block, offline apply-all, live single-turn) enroll the correct person's voiceprint.
- Persist live user bindings deterministically on stop so they survive reopen regardless of the render-time join.
- Clear the `(auto) xx%` suffix immediately on edit and on confirmation, in both modes.
- Add a first-class "confirm correct" path with visible saved feedback.

**Non-Goals:**
- No changes to the offline diarization clustering or recognition scoring itself.
- No changes to `is_me`, expected-speaker allowlists, or `speaker_embeddings` schema (provenance columns already exist).
- No re-architecture of the render-time display join (it stays, but is no longer the only thing restoring user names).

## Decisions

### D1 — Offline single-block correction enrolls via the cluster cache (reparent)
Route offline single-block corrections through enrollment of the block's cluster cache. In `assign_block_speaker`, after `set_transcript_override`, resolve the block's `(meeting_id, cluster_label)` via `get_transcript_cluster` and call `enroll_cluster` for the assigned speaker. Rationale: offline meetings always have persisted cache rows from `write_cluster_cache` at diarization time, so reparenting works with no raw buffer needed. Alternative considered: extracting a raw embedding buffer for the block window offline — rejected because it requires retaining/ re-extracting audio state that offline mode doesn't keep. This is a clean fulfillment of the already-specified "Speaker enrollment on assignment" for single blocks.

### D2 — Live single-turn corrections already enroll; make it reliable
Keep `enroll_embeddings_from_buffer` at `finalize_online_session` for live single-turn corrections. Fix the gaps: (a) `assign_live_speaker` must return an actionable error (not a `warn!` + fake success) when no live prototype store is active, so a correction cannot silently vanish; (b) guarantee `finalize_online_session` runs for every live-diarized stop (remove reliance on a frontend flag that can be false after live binds).

### D3 — Persist user bindings into stored transcripts at finalize (not just the join)
In `finalize_online_session`, after writing `meeting_speakers` user bindings and `speaker_override_id` overrides, also rewrite the stored `transcripts.speaker` for user-bound blocks to a stable value so the DB row itself carries user identity. This removes the dependency on the render-time `COALESCE` as the sole restore path. Rationale: cheapest deterministic fix for C2; keeps the join for auto/none cases, but user labels no longer depend on it. Implementation may set `transcripts.speaker = speaker_override_id` columns resolution or write the registry name; exact column choice is a task-level detail.

### D4 — Offline local updater mirrors the live updater
Fix `usePaginatedTranscripts.updateSpeakerLabel` to set `speaker_matched_by:'user'` and `speaker_match_score: undefined` (matching `applyLiveSpeakerLabel`), and remove the `speaker_label === label` short-circuits so re-selecting the same name doesn't no-op. This is the minimal change that makes the suffix drop in place offline.

### D5 — New "confirm correct" backend + UI affordance
Add a command (e.g. `confirm_block_speaker` / reuse `confirm_speaker_binding`) that marks an auto binding as user-confirmed without changing the name: per-block → set `speaker_override_id`; per-cluster → flip `meeting_speakers.matched_by='user'` + clear `match_score`. Wire it from a small "confirm" action on auto-decorated labels in `SpeakerLabel`. Do NOT create new `speaker_embeddings` on a pure confirmation. Rationale: a pure confirmation is semantically distinct from an assignment; treating "user re-picked same name" as confirmation (D5b) is also adopted so the editor naturally confirms without a separate button, but a dedicated affordance avoids ambiguity.

### D5b — Re-selecting the same name = confirmation (except a true name change)
In the editor, when the user picks a speaker whose name equals the current display name, treat it as a confirmation (D5) rather than a no-op. When the name differs, the normal assignment path runs. This gives the user a natural "I agree with this" gesture.

### D6 — Human-readable voiceprint storage (reuse existing formatter)
The voiceprint browser storage summary currently prints raw `Bytes: N` (`VoiceprintBrowser.tsx:431`). The Settings general-tab voiceprint section already formats a human-readable size via `formatBytes` (`DiarizationSettings.tsx:47-52`, "X.X MB"). Reuse the same shared formatter in the voiceprint browser so the two surfaces agree and the size is shown in MB. Rationale: no new formatting logic; one source of truth for size display. Alternative considered: a new formatter — rejected as duplication.

### D7 — Voiceprint browser default state is collapsed
The voiceprint browser currently expands all groups on load (`VoiceprintBrowser.tsx:252-256`, `setExpanded(allIds)`). Change the default to an empty expanded set so groups start collapsed, keeping the existing per-group toggle and expand-all/collapse-all controls (`:273-292`) unchanged. Rationale: with a growing unconfirmed-cache corpus this is the least noisy default; the expand-all affordance is already in the header. No new state or controls required.

## Risks / Trade-offs

- **[Enrolling every single-block edit can over-enroll]** → Per-person cap (64) and best-K (8) pruning already bound prototype growth; used embeddings are high-quality clips, so over-enrollment risk is low. Confirmation (D5) explicitly does not add new rows.
- **[Offline single-block enroll requires cache rows]** → For legacy meetings without caches, `enroll_cluster` is a no-op but the label mapping still applies (spec: "Correction with no audio still labels"). Acceptable degradation.
- **[Rewriting transcripts.speaker at finalize may conflict with stop-time predicted pass]** → `useRecordingStop.ts:274` runs before `finalize_online_session`; D3 applies after, so finalize wins for user-bound blocks. Apply-then-check ordering in the task.
- **[New confirm command adds an API surface]** → Small, mirrors existing `assign_*`; keeps name-change and confirmation paths explicit.
- **[Collapsed-by-default hides rows on first load]** → The header's Expand-all control is always visible, so users can reveal everything in one click; group counts remain visible while collapsed.

## Migration Plan

Backend command signatures are additive (new `confirm_*` command; `assign_block_speaker` gains enrollment internally — no signature change). No schema migration: reuse existing `transcripts.speaker_override_id` and `meeting_speakers.matched_by`/`match_score`. No data backfill required; behavior applies to new edits immediately.

## Open Questions

None blocking. Exact choice of stored `transcripts.speaker` representation for user bindings at finalize (registry name vs. cluster label + override) is a task-level implementation detail that does not change the specs.
