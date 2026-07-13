---
parent: CODEBASE_MAP_MODULES.md
last_mapped: 2026-07-13T14:36:00Z
module: frontend_hooks
---

> Part of [Module Guide](CODEBASE_MAP_MODULES.md) | [Codebase Map](CODEBASE_MAP.md)

# Module: Frontend Hooks (Custom React Hooks)

## Overview

**Purpose**: The frontend hooks module provides custom React hooks for shared logic across components — audio device management, recording state, transcription status, and Tauri command invocation.

**Entry point**: `frontend/src/hooks/` — hooks directory
**Framework**: React + TypeScript

## File Reference

| File | Purpose | Key Exports | Tokens |
|------|---------|-------------|--------|
| `use-audio-devices.ts` | Audio device management | useAudioDevices() | ~4k |
| `use-recording.ts` | Recording state management | useRecording() | ~5k |
| `use-transcription.ts` | Transcription status tracking | useTranscription() | ~3k |
| `use-tauri-invoke.ts` | Tauri command wrapper | useTauriInvoke() | ~2k |
| `use-theme.ts` | Theme management | useTheme() | ~1k |

## Public API (Hook Interfaces)

### useAudioDevices

```typescript
interface UseAudioDevicesReturn {
  devices: AudioDevice[];
  selectedDeviceId: string | null;
  isLoading: boolean;
  error: Error | null;
  selectDevice: (deviceId: string) => Promise<void>;
  refreshDevices: () => Promise<void>;
}

function useAudioDevices(): UseAudioDevicesReturn;
```

### useRecording

```typescript
interface UseRecordingReturn {
  isRecording: boolean;
  duration: number;
  meetingName: string | null;
  startRecording: (meetingName?: string) => Promise<void>;
  stopRecording: () => Promise<{ recordingPath: string }>;
  resetRecording: () => void;
}

function useRecording(): UseRecordingReturn;
```

### useTranscription

```typescript
interface UseTranscriptionReturn {
  transcript: string;
  isTranscribing: boolean;
  wordCount: number;
  onUpdate: (callback: (text: string) => void) => void;
}

function useTranscription(): UseTranscriptionReturn;
```

## Internal Architecture

### Hook Composition Pattern

Hooks are composed within page components:
```tsx
function RecordingPage() {
  const { devices, selectedDeviceId, selectDevice } = useAudioDevices();
  const { isRecording, startRecording, stopRecording } = useRecording();
  const { transcript, onUpdate } = useTranscription();
  
  // ... component logic
}
```

### State Management Integration

- Hooks use Zustand stores internally for cross-component state
- Local `useState` for hook-specific transient state (loading, error)
- `useEffect` for Tauri event listeners (recording updates, transcription progress)

## Dependencies (imports FROM)

| Module/Package | What is imported | Why |
|---------------|-----------------|-----|
| `@tauri-apps/api` | invoke, listen | Tauri IPC calls and events |
| `zustand` | useStore | Zustand store access |

## Dependents (imported BY)

| Consumer Module | What it uses | Context |
|----------------|-------------|---------|
| Recording page | useRecording, useAudioDevices | Recording controls |
| Meeting detail page | useTranscription | Transcript display |
| Settings page | useTheme, useAudioDevices | Device and theme config |

## Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `refreshInterval` | 5000ms | Audio device refresh interval |
| `durationUpdateRate` | 1000ms | Recording duration tick rate |

## Error Handling

- **Device enumeration failure**: Show error toast, allow manual refresh
- **Recording start failure**: Return error in Promise, show UI notification
- **Transcription disconnect**: Reset transcription state, prompt retry

## Gotchas and Tech Debt

- **Hook cleanup**: useEffect cleanup functions must properly remove event listeners
- **Stale closures**: Hook state accessed in callbacks may be stale — use functional setState