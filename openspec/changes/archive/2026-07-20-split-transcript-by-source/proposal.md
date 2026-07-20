## Why

The backend already labels every transcript segment with `source_device` ("Microphone" or "System") and persists it in `transcripts.json`, but the frontend drops this field at the TypeScript boundary. All transcript segments render as an undifferentiated single-column timeline, making it impossible to tell who is speaking when both the local participant (mic) and remote participants (system audio) talk simultaneously. This makes the transcription output confusing and hard to follow.

## What Changes

- **Frontend TypeScript types** gain `source_device` field on `TranscriptUpdate` and `Transcript` interfaces
- **Live transcript view** (home page) renders segments in a chat-style layout: mic segments aligned left with one background color, system segments aligned right with a different background color, timestamps on opposite sides
- **Meeting detail view** (note list / old meetings) uses the same chat-style layout, restoring `source_device` from persisted transcript data
- **Transcript persistence** in SQLite and API response types gains `source_device` column/field so it survives database round-trips (currently only in `transcripts.json`)
- **TranscriptContext** preserves `source_device` through state updates, sorting, and deduplication

## Capabilities

### New Capabilities
- `split-transcript-ui`: Chat-style dual-column transcript rendering with source-based visual differentiation (background colors, alignment, timestamp sides) for both live and historical views

### Modified Capabilities
<!-- No existing spec capabilities are modified — the backend source-labeled-transcription spec is already satisfied; this change consumes its output in the frontend -->

## Impact

- **Frontend types**: `frontend/src/types/index.ts` — add `source_device` to `TranscriptUpdate` and `Transcript`
- **Components**: `VirtualizedTranscriptView.tsx`, `TranscriptView.tsx`, `TranscriptPanel.tsx` — new chat-style rendering logic
- **Context**: `TranscriptContext.tsx` — preserve `source_device` through state management
- **Database**: `database/models.rs`, migration — add `source_device` column to transcript table
- **API**: `api/api.rs` — include `source_device` in `MeetingTranscript` response struct
- **Services**: `transcriptService.ts` — pass `source_device` through to frontend state
- **No backend audio pipeline changes** — all backend labeling already exists from `split-mic-system-tracks`
