## 1. Database Migration

- [x] 1.1 Create migration `20260720000000_add_source_device.sql` that adds `source_device TEXT` column to `transcripts` table
- [x] 1.2 Verify migration runs cleanly against existing database

## 2. Backend — Propagate source_device Through Data Layer

- [x] 2.1 Add `source_device: Option<String>` field to `Transcript` struct in `database/models.rs`
- [x] 2.2 Add `source_device: Option<String>` field to `TranscriptSegment` struct in `api/api.rs` (line 180)
- [x] 2.3 Add `source_device: Option<String>` field to `MeetingTranscript` struct in `api/api.rs` (line 129)
- [x] 2.4 Update `TranscriptsRepository::save_transcript` in `database/repositories/transcript.rs` to INSERT `source_device` column
- [x] 2.5 Update `api_get_meeting_transcripts` in `api/api.rs` to map `source_device` from DB model to `MeetingTranscript` response
- [x] 2.6 Update retranscription INSERT in `audio/retranscription.rs` to include `source_device` column
- [x] 2.7 Update import INSERT in `audio/import.rs` to include `source_device` column
- [x] 2.8 Update `recording_commands.rs` save logic to pass `source_device` from `TranscriptSegment` (recording_saver) to the API `TranscriptSegment` when persisting to SQLite

## 3. Frontend — TypeScript Types

- [x] 3.1 Add `source_device?: string` to `Transcript` interface in `types/index.ts`
- [x] 3.2 Add `source_device: string` to `TranscriptUpdate` interface in `types/index.ts`
- [x] 3.3 Add `source_device?: string` to `TranscriptSegmentData` interface in `types/index.ts`

## 4. Frontend — State Management

- [x] 4.1 Update `TranscriptContext.tsx` main listener to copy `source_device` from `TranscriptUpdate` into `Transcript` object (line 306-318)
- [x] 4.2 Update `TranscriptContext.tsx` `addTranscript` callback to copy `source_device` (line 416-427)
- [x] 4.3 Update `TranscriptContext.tsx` reload sync (`syncFromBackend`) to map `source_device` from backend history segments (line 375-386)

## 5. Frontend — Data Conversion

- [x] 5.1 Update `TranscriptPanel.tsx` segment conversion to include `source_device` from `Transcript` (line 41-49)
- [x] 5.2 Update `usePaginatedTranscripts.ts` `convertTranscriptsToSegments` to include `source_device` (line 33-41)

## 6. Frontend — Chat-Style UI Rendering

- [x] 6.1 Update `TranscriptSegment` component in `VirtualizedTranscriptView.tsx` to accept `source_device` prop
- [x] 6.2 Implement conditional layout: mic = left-aligned with timestamp left + blue bubble; system = right-aligned with timestamp right + green bubble; unknown = neutral legacy style
- [x] 6.3 Update virtualized rendering path to pass `source_device` to `TranscriptSegment`
- [x] 6.4 Update simple rendering path to pass `source_device` to `TranscriptSegment`

## 7. Verification

- [x] 7.1 Run `cargo check` in `frontend/src-tauri` to verify backend compiles
- [x] 7.2 Run frontend typecheck/lint to verify TypeScript compiles
- [x] 7.3 Manual test: start recording with both mic and system audio, verify chat-style layout on home page
- [x] 7.4 Manual test: stop recording, save meeting, view in meeting details, verify chat-style layout persists
- [x] 7.5 Manual test: view an old meeting (pre-migration), verify legacy neutral rendering
