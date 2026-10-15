# Proposal

## Why

During a Fast-mode recording the most recent transcript row often stays without a speaker label for a long time, sometimes until someone speaks again. The live diarizer and the transcription engine receive the same audio chunk at the same moment, and the diarizer often finishes first. The frontend labels rows only when a new speaker turn arrives. A row whose turn arrived before the row itself is never matched until the next turn, and during a pause no turn comes.

## What Changes

- When new live transcript rows are added, the frontend matches them right away against the speaker turns it has already received, using the same overlap rule it applies when a turn arrives.
- User-pinned rows stay exempt from automatic matching, as today.
- No backend change. Turn emission, the live word-level split and stop-time assignment are untouched.

Out of scope (noted, not addressed): the last ~0–0.75 s of each channel's audio is not covered by any turn until more speech arrives (streaming window geometry). A very short row that falls entirely in that tail still waits for the next turn.

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `live-speaker-labels`: the "Frontend matches speaker turns to live transcripts" requirement gains the case where the covering turn was emitted before the transcript row arrived.

## Impact

- `frontend/src/contexts/TranscriptContext.tsx`: the transcript-append path.
- `frontend/tests/lib/live-speaker-labels.test.ts`: new test for the turn-before-row order.
- No API, event, persistence or dependency changes.
