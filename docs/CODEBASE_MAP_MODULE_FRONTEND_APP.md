---
parent: CODEBASE_MAP_MODULES.md
last_mapped: 2026-08-05T15:00:00Z
module: frontend_app
---

> Part of [Module Guide](CODEBASE_MAP_MODULES.md) | [Codebase Map](CODEBASE_MAP.md)

# Module: Frontend App (Next.js + Tauri shell)

## Overview

**Purpose**: The Next.js App Router application shell running inside the Tauri v2 desktop window. Sets up the global provider tree, onboarding gating, tray/drag-drop event handling, and page routing. This is a **client-heavy** app — nearly every component uses `'use client'` (it is a desktop app, not SSG/SSR).

**Entry point**: `frontend/src/app/layout.tsx` (RootLayout).
**Framework**: Next.js App Router + React 18 + TypeScript, served as a static export (`frontendDist: "../out"`).

## File Reference

| File | Purpose | Key Exports | Tokens |
|------|---------|-------------|--------|
| `app/layout.tsx` | Root layout: fonts, provider tree, onboarding gating, tray/drag events, Sidebar + MainContent | `RootLayout` | — |
| `app/page.tsx` | Live recording home page | `Home` | — |
| `app/settings/page.tsx` | Settings page (General/Recordings/Transcription/Summary/Beta tabs) | `SettingsPage` | — |
| `app/meeting-details/page.tsx` | Persisted meeting view (paginated transcripts + summary) | `MeetingDetailsPage` (wrapped in `Suspense`) | — |
| `app/notes/[id]/` | Per-note route (BlockNote editor) | — | — |
| `app/_components/` | Page-local components | `TranscriptPanel`, `SettingsModal`, `StatusOverlays` | — |
| `app/globals.css` | Global styles + Tailwind | — | — |
| `contexts/` | React contexts | `RecordingStateContext`, `TranscriptContext`, `ConfigContext`, `SidebarProvider`, `OnboardingContext`, `ImportDialogContext`, `OllamaDownloadContext`, `RecordingPostProcessingProvider` | — |
| `services/` | IPC service wrappers | `transcriptService`, `recordingService`, `storageService`, `indexedDBService`, `configService`, `updateService` | — |
| `lib/` | Utility modules | `analytics`, `summary-language-preferences`, `recordingNotification` | — |
| `constants/` / `config/` / `types/` | Constants, config, data contracts | `audioFormats`, `Transcript`, `TranscriptUpdate` | — |

## Public API (Routes)

| Route | Component | Description |
|-------|-----------|-------------|
| `/` | `Home` | Live recording + transcript panel |
| `/settings` | `SettingsPage` | Tabs: General, Recordings, Transcription, Summary, Beta |
| `/meeting-details?id=` | `MeetingDetailsPage` | Persisted meeting (paginated transcripts, summary, retranscription) |
| `/notes/[id]` | Notes page | BlockNote editor |

## Internal Architecture

### Provider Tree (`app/layout.tsx`, outer → inner)

`AnalyticsProvider → RecordingStateProvider → TranscriptProvider → ConfigProvider → OllamaDownloadProvider → OnboardingProvider → UpdateCheckProvider → SidebarProvider → TooltipProvider → RecordingPostProcessingProvider → ImportDialogProvider` — plus `DownloadProgressToastProvider`, `ImportDropOverlay`, `ConditionalImportDialog` (beta-gated), `Toaster`. Handles `request-recording-toggle` tray events, `tauri://drag-enter/leave/drop` for audio import, and onboarding completion (window reload).

### State Management

- **React Context** (not Zustand) layered over Tauri IPC service wrappers.
- `RecordingStateContext` — single source of truth for recording state (backend-polled 500ms + events); `RecordingStatus` lifecycle enum.
- `TranscriptContext` — live transcript buffer (sequence ordering + IndexedDB persistence).
- `SidebarProvider` — meeting list, search, current meeting, summary polling registry, server addresses.

### Data Fetching

- **Tauri `invoke`** for commands (`api_*`, `start_recording_with_devices_and_meeting`, `get_transcript_history`, `get_audio_devices`, `parakeet_*`).
- **Tauri `listen`/`emit`** for events (`transcript-update`, `recording-*`, `model-config-updated`, `speech-detected`, `transcription-error`, `request-recording-toggle`).
- **Polling**: recording state 500ms; recording sync 1s; summary generation 5s (max 200 polls); transcription completion 500ms (60s cap).
- Next.js App Router but effectively all client components.

### Two Transcript Pipelines

- **Live** (`TranscriptContext`, home page) — buffered `transcript-update` events, no pagination.
- **Persisted** (`usePaginatedTranscripts`, meeting-details page) — offset/limit infinite scroll from `api_get_meeting_transcripts`.

## Dependencies (imports FROM)

| Module/Package | What is imported | Why |
|---------------|-----------------|-----|
| `@tauri-apps/api/core` + `/event` | `invoke`, `listen`, `emit` | Tauri IPC |
| `next/navigation` | `useRouter`, `usePathname`, `useSearchParams` | Routing |
| `@tanstack/react-virtual` | `useVirtualizer` | Transcript virtualization |
| `framer-motion` | `motion`, `AnimatePresence` | Animations |
| `sonner` | `toast` | Notifications |

## Dependents (imported BY)

| Consumer Module | What it uses | Context |
|----------------|-------------|---------|
| All feature components | Contexts, services | State + IPC access |

## Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `serverAddress` / `transcriptServerAddress` | `http://localhost:5167` / `http://127.0.0.1:8178/stream` | Hardcoded in SidebarProvider (legacy) |
| Dev server port | 3118 | `pnpm dev` / `devUrl` |

## Error Handling

- Tauri command errors (`Result<_, String>`) surfaced via `sonner` toasts.
- `Suspense` + `useSearchParams` for dynamic pages.
- Onboarding completion reloads the window; update checks handled by `UpdateCheckProvider`.

## Gotchas and Tech Debt

- **App is fully client-side** — "server components" are minimal; hydration concerns are mostly moot in a Tauri window.
- **Hardcoded server addresses** (`localhost:5167`, `127.0.0.1:8178/stream`) in SidebarProvider are legacy leftovers.
- Version string `v0.5.0` hardcoded in sidebar footer (one of 3 version-bump locations).
- Startup cleanup deletes old meetings (`deleteOldMeetings(7)`, `deleteSavedMeetings(24)`).
