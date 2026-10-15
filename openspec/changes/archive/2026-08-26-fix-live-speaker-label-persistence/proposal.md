## Why

Live speaker renames during Fast-mode online diarization are unreliable: on system-sound transcripts a rename sticks only sometimes (the UI shows it for a moment, then reverts to the original value), and on microphone transcripts it almost never survives. The root causes are structural: (1) the mic turn prefix flips from `SPEAKER_NN` to `MIC_SPEAKER_NN` mid-session as soon as the first system chunk is seen, so the label a user clicked never matches the label persisted at stop; (2) the stop-time assignment pass overwrites every transcript's `speaker` with the raw cluster label, dropping user bindings; and (3) the frontend lets turn events carrying a stale `display_name` clobber a label the user just set. Separately, a user-selected speaker is ground truth that should improve the global registry — the segment's embeddings should join that person's voiceprint set — and auto-identified names should be visibly marked so users can trust them at a glance.

## What Changes

- **Stable live label prefixes**: the mic channel's label prefix (`MIC_SPEAKER` vs `SPEAKER`) is fixed for the whole session and used consistently by live turn emission, stop-time `finalize()`, and persistence, so displayed and persisted labels always match.
- **Stop-time assignment reconciliation**: `finalize()` applies session cluster-to-person bindings when emitting `SpeakerAssignment`s, so transcripts persisted at stop carry the user's chosen identity instead of being clobbered back to the raw cluster label.
- **Ground-truth enrollment**: when a user assigns a speaker to a transcript block (live or offline, single-block or cluster-wide), the embeddings whose time windows cover that block are enrolled into that registry speaker's global prototype set, so the person is recognized across future meetings. Live single-turn overrides no longer skip enrollment when the block's embeddings are available.
- **Auto-label marking**: transcripts whose speaker identity comes from automatic recognition display the name with an `(auto)` suffix together with the match confidence/similarity; user-assigned identities display the plain name with no suffix.
- **Live UI label pinning**: labels the user has explicitly assigned during recording are pinned and are never overwritten by later speaker-turn events carrying stale `display_name` values.
- **Binding identity by pipeline id + channel**: live bindings and enrollments are keyed by the underlying pipeline speaker identity and channel rather than the display label, so enrollment seeding no longer mixes mic and system voices of the same numeric id.

## Capabilities

### New Capabilities

- `speaker-label-confidence`: Display of automatic-vs-user provenance for speaker labels across the app — auto-matched names render with an `(auto)` suffix and a similarity/confidence value, user-assigned names render plain; the underlying `matched_by` and `match_score` are already stored in `meeting_speakers` and SHALL be surfaced through transcript queries and the live speaker-turn event.

### Modified Capabilities

- `speaker-identity-registry`: The enrollment rule changes — user-assigned transcript blocks SHALL enroll the block's covering session embeddings into the person's global prototypes (ground truth), in addition to the existing cluster-cache enrollment; enrollment seeding SHALL be keyed by pipeline id + channel, never mixing channels.
- `live-speaker-labels`: Live rename persistence changes — renames SHALL survive to the persisted meeting (reconciled into the stop-time assignment pass), mic prefixes SHALL be stable for the session, and user-pinned labels SHALL NOT be reverted by subsequent speaker-turn events.
- `online-speaker-diarization`: The stop-time finalize behavior changes — final assignments SHALL be reconciled with session cluster-to-person bindings and ground-truth enrollments before they are written, and the label prefix scheme SHALL be session-stable rather than chunk-dependent.

## Impact

- **Backend**: `audio/online_diarization.rs` (prefix stability, pipeline-id/channel keyed bindings, reconciled assignments, ground-truth enrollment of user-assigned blocks), `audio/recording_commands.rs` (`assign_live_speaker`, `finalize_online_session`, session data shape), `audio/diarization.rs` (`persist_and_recognize_session`, enrollment helpers), `audio/speaker_recognition.rs` (confidence surfacing), `database/repositories/speaker.rs` (enrollment by time window, auto-marking queries), `database/repositories/meeting.rs` (display-name select gains provenance + score).
- **Frontend**: `contexts/TranscriptContext.tsx` (turn handler no longer clobbers pinned labels), `components/VirtualizedTranscriptView.tsx` + `app/_components/TranscriptPanel.tsx` (display `(auto)` + confidence, no suffix for user picks), `services/recordingService.ts` (turn payload carries confidence/vs-user provenance).
- **Specs**: delta specs for `speaker-label-confidence` (new), `speaker-identity-registry`, `live-speaker-labels`, and `online-speaker-diarization` (modified).
- **No DB migration required**: `meeting_speakers.matched_by` and `match_score` already exist; enrollment reuses `speaker_embeddings`; confidence comes from existing `match_score`.