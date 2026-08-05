---
parent: CODEBASE_MAP_MODULES.md
last_mapped: 2026-08-05T15:00:00Z
module: frontend_hooks
---

> Part of [Module Guide](CODEBASE_MAP_MODULES.md) | [Codebase Map](CODEBASE_MAP.md)

# Module: Frontend Hooks (Custom React Hooks)

## Overview

**Purpose**: Custom React hooks encapsulating shared logic — recording lifecycle, live transcript buffering, paginated transcript loading, streaming/typewriter effects, auto-scroll, permissions, model selection, and meeting-details orchestration. Recent changes added **`usePaginatedTranscripts`** (offset/limit infinite scroll) and live-mode recording wiring.

**Entry point**: `frontend/src/hooks/` — hooks directory.
**Framework**: React 18 + TypeScript + Tauri IPC (`invoke`/`listen`).

## File Reference

| File | Purpose | Key Exports | Tokens |
|------|---------|-------------|--------|
| `usePaginatedTranscripts.ts` | **NEW/CHANGED** — offset/limit pagination for a meeting's persisted transcripts | `usePaginatedTranscripts` | ~1.4k |
| `useTranscriptStreaming.ts` | Typewriter reveal effect | `useTranscriptStreaming` | — |
| `useAutoScroll.ts` | Smart auto-scroll to bottom | `useAutoScroll` | — |
| `useRecordingStart.ts` | Start recording (validates model, picks devices) | `useRecordingStart` | — |
| `useRecordingStop.ts` | Stop flow: wait transcription → flush → save → navigate | `useRecordingStop` | — |
| `useRecordingStateSync.ts` | Poll backend recording state | `useRecordingStateSync` | — |
| `usePermissionCheck.ts` | Check mic/system permissions | `usePermissionCheck` | — |
| `useAudioPlayer.ts` | Audio playback | `useAudioPlayer` | — |
| `useImportAudio.ts` | Audio import flow | `useImportAudio` | — |
| `useModalState.ts` | Modal open/close | `useModalState` | — |
| `useNavigation.ts` | Navigation helpers | `useNavigation` | — |
| `usePlatform.ts` | Platform detection (`useIsLinux`) | `usePlatform` | — |
| `useProcessingProgress.ts` | Processing progress | `useProcessingProgress` | — |
| `useRecentLanguages.ts` | Recent summary languages | `useRecentLanguages` | — |
| `useTranscriptionModels.ts` | Transcription model selection | `useTranscriptionModels` | — |
| `useTranscriptRecovery.ts` | Recover un-saved transcripts | `useTranscriptRecovery` | — |
| `useUpdateCheck.ts` | App update checks | `useUpdateCheck` | — |
| `meeting-details/` | `useMeetingData`, `useSummaryGeneration`, `useTemplates`, `useCopyOperations`, `useMeetingOperations` | Meeting-details page hooks | — |

## Public API (key hooks)

### usePaginatedTranscripts

```typescript
function usePaginatedTranscripts({ meetingId: string | null, initialTimestamp?: number }): {
  metadata: MeetingMetadata | null;
  segments: TranscriptSegmentData[];      // memoized derived display segments
  transcripts: Transcript[];             // raw, sorted by audio_start_time
  isLoading: boolean; isLoadingMore: boolean; hasMore: boolean;
  totalCount: number; loadedCount: number;
  error: string | null;
  loadMore: () => Promise<void>;
  reset: () => void;
  refetch: () => Promise<void>;          // retranscription
}
```

- **Offset/limit, page size 100**: calls `api_get_meeting_transcripts({ meetingId, limit: 100, offset })`; backend returns `{ transcripts, total_count, has_more }`.
- **Guards**: `isLoadingRef` (no concurrent), `lastLoadTimeRef` (100ms debounce), `loadedMeetingIdRef` (no re-load).
- **`initialTimestamp` is declared but unused** — pagination always starts from offset 0 (deep-linking not yet implemented).
- Used **only** on the meeting-details page; separate from the live `TranscriptContext` buffer.

## Internal Architecture

- **Recording state**: `RecordingStateContext` is the single source of truth, backend-polled (500ms while recording) + event listeners (`recording-started/stopped/paused/resumed`). `RecordingStatus` lifecycle enum: `IDLE, STARTING, RECORDING, STOPPING, PROCESSING_TRANSCRIPTS, SAVING, COMPLETED, ERROR`.
- **Live transcripts**: `TranscriptContext` buffers `transcript-update` events with sequence ordering + de-dup + IndexedDB persistence (crash recovery), reload-syncs history, and manages the meeting id.
- **Two transcript pipelines**: live (context buffer, home page) vs persisted (pagination hook, meeting-details) — they never share state.
- **Data fetching**: Tauri `invoke` for commands; `listen`/`emit` for events; services wrap IPC (`transcriptService`, `recordingService`, `configService`, `indexedDBService`, `storageService`, `updateService`).

## Dependencies (imports FROM)

| Module/Package | What is imported | Why |
|---------------|-----------------|-----|
| `@tauri-apps/api/core` | `invoke` | IPC command calls |
| `@tauri-apps/api/event` | `listen`, `emit` | Event subscription |
| React | hooks, context | State + effects |

## Dependents (imported BY)

| Consumer Module | What it uses | Context |
|----------------|-------------|---------|
| `app/page.tsx` | `useRecordingStart`, `useRecordingStop`, `usePermissionCheck`, `useRecordingStateSync`, `useTranscriptRecovery` | Live recording |
| `app/meeting-details/page.tsx` | `usePaginatedTranscripts` | Persisted transcript view |
| `VirtualizedTranscriptView.tsx` | `useTranscriptStreaming`, `useAutoScroll` | Rendering |

## Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `DEFAULT_PAGE_SIZE` | 100 | Pagination page size |
| Recording state poll | 500ms | Backend polling while recording |
| Summary generation poll | 5s (max 200 polls) | `SidebarProvider.startSummaryPolling` |
| Transcription completion poll | 500ms (60s cap) | `useRecordingStop` |

## Error Handling

- Hooks expose a coarse single `error` string (e.g. metadata or transcript load failure).
- Recording start/stop failures returned via promises and surfaced as toasts.
- `useTranscriptRecovery` restores un-saved transcripts on reload.

## Gotchas and Tech Debt

- **`initialTimestamp` unused** — deep-linking to a specific transcript page is not implemented (reserved).
- **Two sources of truth for live insertion**: `addTranscript` (RecordingControls parallel path) bypasses the sequence buffer with a weaker de-dup (`text`+`timestamp` equality).
- The live-buffer "recent/stale" dual-path logic is largely dead weight (serial workers = sequential order) but retained as a safety net.
- Effect re-subscription depends on `[currentMeetingId]`, so it re-subscribes on every new recording start (cleanup handled).
- `loadMore` early-returns if `isLoading` (initial) — callers must wait for the initial load.
