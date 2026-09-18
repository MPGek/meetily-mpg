## Why

In online (live) recording a single speaker's speech appears as several separate transcript blocks because blocks are cut by VAD pauses (gap >= 500ms) and the 25-second transcription chunk limit before diarization ever runs. Readers then see one person's utterance fragmented into many records labeled with the same speaker, in both the live view and the saved meeting-details view. Human reading conventions (Zoom, Teams, Otter) present speech as speaker turns, not as audio chunks between pauses.

## What Changes

- Add a display-level turn grouping layer that merges **consecutive transcript segments with the same speaker on the same source channel** into a single visual turn block, in both the live recording view and the meeting-details (historical) view.
- The grouping is render-only: stored transcript data, API responses, player timestamps, block-time editing, and speaker assignment logic remain unchanged.
- Within a merged turn, a merged block's start time is the earliest segment's start and its end is the latest segment's end, so the play button seeks the beginning of the utterance.
- A speaker change, a `source_device` change, or a large time gap (>= 60s between consecutive same-speaker segments) starts a new turn; segments without a resolved speaker are never merged.
- Live behavior: turns re-form as new segments and speaker-turn events arrive; pinned/user-assigned labels from `live-speaker-labels` continue to control displayed names under the merged rendering.
- Word-level sub-rows (live Fast mode) rendered inside a merged turn stay on their member's source side: a System member's sub-rows keep the System side (right-aligned content and labels), a Microphone member's sub-rows keep the Microphone side, so a split member never appears on the opposite side of its turn.
- The existing collapsible grouping mode remains available; turn merging applies within the flat chat-style rendering that is the default.

## Capabilities

### New Capabilities
- `speaker-turn-grouping`: Display-level merging of consecutive same-speaker transcript segments into readable speaker turns in the live view and the meeting-details view, without changing stored data or the recording pipeline.

### Modified Capabilities
<!-- none -->

## Impact

- Frontend only: transcript rendering components (`TranscriptView.tsx`, `VirtualizedTranscriptView.tsx`, `MeetingDetails/TranscriptPanel.tsx`) plus a pure grouping helper in `frontend/src/lib/` (unit-testable, mirroring the approach of `live-speaker-labels.ts`).
- `VirtualizedTranscriptView.tsx`'s merged-turn branch also renders members' live word-level sub-rows; their side/label placement inside a turn is part of the render integration (no data or pipeline change).
- `TranscriptContext` consumes grouped output; no backend/Rust/SQLite changes, no schema or API changes.
- No changes to `live-segment-merging`, `live-speaker-labels`, or `split-transcript-ui` pipeline behavior; interaction with the audio player seek and active-block highlight needs care (highlight currently keys on segment id).
