---
parent: CODEBASE_MAP_MODULES.md
last_mapped: 2026-08-25T10:40:44Z
module: frontend_components
---

> Part of [Module Guide](CODEBASE_MAP_MODULES.md) | [Codebase Map](CODEBASE_MAP.md)

# Module: Frontend Components (React / Next.js)

## Overview

**Purpose**: All React UI for the Meetily desktop app — nav (Sidebar), live recording UI (RecordingControls, TranscriptPanel), the virtualized transcript renderer, model managers, settings, and Shadcn/ui primitives. Recent changes added **mic/system visual separation**, **infinite-scroll pagination**, and — most recently — the **streaming meeting audio player** (`AudioPlayer`) and **speaker diarization UI** (`DiarizationSettings`, speaker labels in `VirtualizedTranscriptView`).

**Entry point**: `frontend/src/components/` (feature components) and `frontend/src/components/ui/` (primitives). Pages in `frontend/src/app/`.

**Framework**: React 18 + Next.js App Router + TypeScript + Tailwind + `@tanstack/react-virtual` + Framer Motion + Shadcn/ui.

## File Reference (key + recently-changed files)

| File | Purpose | Key Exports | Tokens |
|------|---------|-------------|--------|
| `Sidebar/index.tsx` | Left nav: Home/Notes/Settings, meeting list, search, record toggle, dialogs | `Sidebar`, `SidebarProvider` | ~6.8k |
| `VirtualizedTranscriptView.tsx` | **Primary transcript renderer** — virtualization + mic/sys separation + infinite scroll | `VirtualizedTranscriptView`, `VirtualizedTranscriptViewProps` | ~3.6k |
| `TranscriptPanel.tsx` (app/_components) | Home-page live transcript panel (adapts context → segments) | `TranscriptPanel` | ~0.9k |
| `RecordingControls.tsx` | Start/pause/resume/stop recording | `RecordingControls` | — |
| `RecordingStatusBar.tsx` | Recording status overlay | `RecordingStatusBar` | — |
| `ConfidenceIndicator.tsx` | Per-segment confidence display | `ConfidenceIndicator` | — |
| `PermissionWarning.tsx` | Mic/system permission warnings | `PermissionWarning` | — |
| `DeviceSelection.tsx` | Audio device picker | `DeviceSelection` | — |
| `AudioLevelMeter.tsx` / `AudioPlayer.tsx` | Level meter / **streaming playback bar** (`AudioPlayer`) | `AudioPlayer`, `AudioPlayerHandle`, `PlaybackState` | ~1.1k |
| `DiarizationSettings.tsx` | **NEW** Speaker-diarization settings card (enable/auto-run/mode/max-speakers + model check/download) | `DiarizationSettings`, `DiarizationSettingsState` | ~2.6k |
| `WhisperModelManager.tsx`, `ParakeetModelManager.tsx`, `BuiltInModelManager.tsx` | Model download/management | — | — |
| `MeetingDetails/TranscriptPanel.tsx`, `SummaryPanel.tsx` | Persisted meeting view (TranscriptPanel now composes AudioPlayer + diarization progress + speaker rename) | — | — |
| `ImportAudio/`, `DatabaseImport/`, `TranscriptRecovery/` | Import + recovery UI | — | — |
| `ui/` | Shadcn/ui primitives (button, dialog, tooltip, etc.) | `cn()` helper | — |

### Full inventory (feature components under `src/components/`)
`Sidebar`, `MainContent`, `MainNav`, `RecordingControls`, `RecordingStatusBar`, `TranscriptView`, `VirtualizedTranscriptView`, `TranscriptSettings`, `ConfidenceIndicator`, `PermissionWarning`, `AudioLevelMeter`, `AudioPlayer`, `DeviceSelection`, `EditableTitle`, `Logo`, `Info`, `MessageToast`, `ComplianceNotification`, `About`, `AnalyticsProvider`/`AnalyticsConsentSwitch`/`AnalyticsDataModal`, `SettingTabs`, `ModelSettingsModal`, `SummaryModelSettings`, `SummaryLanguageSettings`, `LanguageSelection`, `LanguagePickerPopover`, `WhisperModelManager`, `ParakeetModelManager`, `BuiltInModelManager`, `ModelDownloadProgress`, `RecordingSettings`, `PreferenceSettings`, `BetaSettings`, `ConsoleToggle`, `AudioBackendSelector`, `BluetoothPlaybackWarning`, `ChunkProgressDisplay`, `CustomDialog`, `ConfirmationModel/`, `BlockNoteEditor/`, `AISummary/`, `MeetingDetails/` (TranscriptPanel, SummaryPanel, TranscriptButtonGroup, …), `ImportAudio/`, `DatabaseImport/`, `TranscriptRecovery/`, `UpdateCheckProvider`/`UpdateDialog`/`UpdateNotification`, `molecules/`, `shared/`, `ui/`, `onboarding/`.

## Public API (key components)

### VirtualizedTranscriptView

```tsx
interface VirtualizedTranscriptViewProps {
  segments: TranscriptSegmentData[];       // { id, timestamp(=audio_start_time), endTime?, text, confidence?, source_device? }
  isRecording?: boolean;
  isPaused?: boolean;
  isProcessing?: boolean;
  isStopping?: boolean;
  enableStreaming?: boolean;              // typewriter effect
  showConfidence?: boolean;
  disableAutoScroll?: boolean;            // meeting-details page
  hasMore?: boolean;                      // pagination
  isLoadingMore?: boolean;
  totalCount?: number; loadedCount?: number;
  onLoadMore?: () => void;
}
```

- **Mic/System separation**: `source_device === 'Microphone'` → left-aligned **blue** bubble; `'System'` → right-aligned **green** bubble; `undefined` (legacy) → neutral no-bubble.
- **Speaker labels (NEW)**: inline-editable `SpeakerLabel` per segment (blur/Enter commits, Escape reverts) — calls `onUpdateSpeakerLabel` → `update_speaker_label_command`. Speaker color derived from the numeric suffix of `SPEAKER_NN`/`MIC_SPEAKER_NN` (8-color palette).
- **Virtualization threshold = 10**; below → simple map + Framer Motion entrance; at/above → `useVirtualizer` (`estimateSize: 60`, `overscan: 10`).
- **Infinite scroll**: `IntersectionObserver` on `loadMoreTriggerRef` (+ rAF scroll fallback within 200px), gated on `onLoadMore && hasMore && !isLoadingMore && !isRecording`.

### AudioPlayer (streaming meeting player)

```tsx
forwardRef<AudioPlayerHandle, AudioPlayerProps>
// props: audioPath: string|null, onPlaybackStateChange?, onTimeUpdate?
// handle: { playFrom(startTime: number): Promise<void> }  // jump-to-segment before metadata loads
```
- Wraps `useAudioPlayer`; hidden `<audio>` streams via `convertFileSrc(audioPath)`; on native decode failure falls back to `prepare_audio_for_playback` (FFmpeg WAV).
- Imperative `playFrom` queues a pending time until `duration > 0`, then seek+play.

### DiarizationSettings (speaker diarization card)

```tsx
DiarizationSettings(): JSX.Element  // rendered inside PreferenceSettings
```
- Persists `enabled`/`autoRun`/`maxSpeakers`/`mode` to localStorage (keys duplicated from `lib/diarization.ts`).
- `check_diarization_models` → readiness; `download_diarization_models` → download with progress (`diarization-model-download-progress`/`-complete`/`-error` events).

### Sidebar

```tsx
export default function Sidebar(): React.FC
// 'use client'. Nav: Home, Meeting Notes, Settings. Meeting routing to /meeting-details?id=.
// Transcript search (api_search_transcripts), record toggle (start-recording-from-sidebar window event
//   or sessionStorage['autoStartRecording']='true' + route to /), meeting CRUD dialogs,
//   model/transcript config modals, import-audio (beta-gated), version footer.
```

### TranscriptPanel (home page)

```tsx
TranscriptPanel({ isProcessingStop, isStopping, showModal })
// useMemo maps Transcript[] → TranscriptSegmentData[] (carries source_device),
// renders VirtualizedTranscriptView with recording-driven props; Copy + Language header controls;
// PermissionWarning (skipped on Linux).
```

## Internal Architecture

- **Component composition**: Shadcn/ui primitives + `cn()` + lucide-react icons + sonner toasts. Feature components consume context hooks (`useRecordingState`, `useTranscripts`, `useSidebar`, `useConfig`) and Tauri services.
- **Two transcript panels**: `app/_components/TranscriptPanel.tsx` (live, non-paginated, uses `TranscriptContext.transcripts`) vs `components/MeetingDetails/TranscriptPanel.tsx` (persisted, paginated via `usePaginatedTranscripts`).
- **Beta gating**: Import-audio UI renders only when `betaFeatures.importAndRetranscribe` is set.

## Dependencies (imports FROM)

| Module/Package | What is imported | Why |
|---------------|-----------------|-----|
| `@tanstack/react-virtual` | `useVirtualizer` | Transcript virtualization |
| `framer-motion` | `motion`, `AnimatePresence` | Entrance/status animations |
| `@tauri-apps/api` | `invoke`, `listen`, `emit` | IPC + events |
| `lucide-react` | icons | Icon set |
| `sonner` | `toast` | Notifications |
| `next/navigation` | `useRouter`, `usePathname` | Routing |

## Dependents (imported BY)

| Consumer Module | What it uses | Context |
|----------------|-------------|---------|
| `app/layout.tsx` | `Sidebar`, providers, Toaster | App shell |
| `app/page.tsx` | `TranscriptPanel`, `RecordingControls`, overlays | Live home page |
| `app/meeting-details/page.tsx` | `MeetingDetails/*`, `VirtualizedTranscriptView` | Persisted meeting view |
| `app/settings/page.tsx` | `SettingTabs`, model managers, settings | Settings page |

## Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `VIRTUALIZATION_THRESHOLD` | 10 | Switch to virtualization above this segment count |
| `estimateSize` / `overscan` | 60 / 10 | Virtualizer sizing |
| Version string | `v0.5.0` | Hardcoded in sidebar footer (one of 3 version-bump locations) |

## Error Handling

- TypeScript strict types prevent most runtime errors.
- Recording/permission failures surfaced via toasts (`sonner`) and `PermissionWarning`.
- Per-command errors returned as `Result<_, String>` from Tauri and shown via toast.

## Gotchas and Tech Debt

- **Dead conditional in `TranscriptSegment`**: the `isStreaming` branch returns the same markup as the final state for both mic and system (cosmetic).
- **Duplicated JSX**: virtualized and non-virtualized branches contain near-identical infinite-scroll and listening-indicator markup.
- **`playback`/`showPlayback` UI is vestigial** (`setShowPlayback(true)` commented out).
- **Diarization storage keys duplicated**: `DiarizationSettings.STORAGE_KEYS` vs `lib/diarization.ts` `DIARIZATION_STORAGE_KEYS` — drift risk.
- **`modelsReady` default-false flicker**: the "Speakers" button briefly shows "Setup Speakers" until `check_diarization_models` resolves.
- **Version string hardcoded** in the sidebar (drift risk with `tauri.conf.json`).
- `modelConfig` defaults deliberately **not** applied ("let DB be the source of truth").
- Clean-stop-word logic strips filler words for display only (not copy).
