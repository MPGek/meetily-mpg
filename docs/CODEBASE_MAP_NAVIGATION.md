---
parent: CODEBASE_MAP.md
last_mapped: 2026-08-25T10:40:44Z
section: navigation
---

> Part of [Codebase Map](CODEBASE_MAP.md)

# Navigation Guide

## Getting Started

1. Read [`CODEBASE_MAP_ARCHITECTURE.md`](CODEBASE_MAP_ARCHITECTURE.md) for the system overview.
2. Read [`CODEBASE_MAP_MODULES.md`](CODEBASE_MAP_MODULES.md) for the module index.
3. For build/dev commands, see [`CODEBASE_MAP_OPERATIONS.md`](CODEBASE_MAP_OPERATIONS.md).
4. To develop: `cd frontend && pnpm install && pnpm tauri:dev` (auto GPU detection) — or `frontend/dev-gpu.bat` on Windows for the full GPU sidecar flow.

## Module Quick Reference (Rust — `frontend/src-tauri/src/`)

```
src-tauri/src/
├── lib.rs                     ← Tauri builder + command registration (START HERE)
├── audio/                     ← Audio engine (mic + system capture, VAD, stereo mix, recording)
│   ├── mod.rs                 ← Module root / re-exports
│   ├── pipeline.rs            ← Per-channel VAD + stereo mixing
│   ├── vad.rs                 ← Silero VAD v6 + rolling buffer
│   ├── recording_*.rs         ← State, commands, preferences, saver, manager
│   ├── retranscription.rs     ← "Enhance" re-transcribe
│   ├── import.rs              ← Audio import
│   ├── diarization.rs         ← Offline speaker diarization (polyvoice)
│   ├── online_diarization.rs  ← Online diarization (Efficient/Fast)
│   ├── audio_file.rs          ← Audio file discovery + playback transcode
│   ├── transcription/         ← STT provider abstraction + provider-aware model gate
│   └── audio_v2/              ← ORPHANED (dead, not declared)
├── whisper_engine/            ← Whisper.cpp wrapper (engine.rs, commands.rs, parallel_processor.rs)
├── parakeet_engine/           ← Parakeet ONNX streaming (engine, model.rs)
├── summary/                   ← AI summarization (service, processor, llm_client, debug_log, summary_engine/)
├── api/                       ← IPC commands + shared DTOs + legacy HTTP client (api.rs)
├── database/                  ← SQLite (manager, models, repositories/)
├── ollama|openai|anthropic|groq|openrouter/ ← LLM provider modules
├── notifications/             ← Desktop notifications
├── analytics/                 ← PostHog
├── main.rs                    ← App entry point
└── tray.rs, onboarding.rs     ← Tray + onboarding
```

### Frontend (`frontend/src/`)

```
src/
├── app/                       ← Next.js App Router (layout.tsx, page.tsx, settings, meeting-details, notes)
├── components/                ← UI (Sidebar, VirtualizedTranscriptView, RecordingControls, ...)
│   └── ui/                    ← Shadcn/ui primitives
├── hooks/                     ← Custom hooks (usePaginatedTranscripts, useRecordingStart, ...)
├── contexts/                  ← React contexts (RecordingState, Transcript, Config, SidebarProvider, ...)
├── services/                  ← IPC service wrappers (transcriptService, recordingService, ...)
├── lib/                       ← Utilities (analytics, etc.)
├── types/                     ← TypeScript contracts
└── constants/, config/        ← Constants + config
```

## Common Tasks — "I want to..."

| I want to... | Module | Key Files | Notes |
|--------------|--------|-----------|-------|
| Change how recording starts/stops | Audio | `audio/recording_commands.rs`, `recording_manager.rs`, `recording_state.rs` | Tauri commands `start_recording_with_devices_and_meeting`, `stop_recording` |
| Modify VAD behavior | Audio | `audio/vad.rs` | `VadConfig::live()`/`batch()`; Silero v6 model is embedded at build time |
| Add a channel to the recording (beyond mic/system) | Audio | `audio/pipeline.rs`, `recording_state.rs` | Stereo layout is left=mic/right=sys — changes ripple to retranscription + saver |
| Change the transcription engine selection | Audio + engines | `audio/transcription/`, `whisper_engine/`, `parakeet_engine/` | Provider abstraction in `transcription/engine.rs`; config via `transcript_settings` |
| Add a new Whisper model to the catalog | Whisper | `whisper_engine/whisper_engine.rs` + `config.rs` (`WHISPER_MODEL_CATALOG`) | Download URL in `download_model` |
| Change Parakeet quantization / download | Parakeet | `parakeet_engine/parakeet_engine.rs` | Catalog + URLs; Int8 only |
| Modify summarization prompts/logic | Summary | `summary/processor.rs`, `service.rs`, `templates/` | `generate_meeting_summary`; templates custom→bundled→built-in |
| Add a new LLM provider | Summary | `summary/llm_client.rs` (`LLMProvider`) | URL/header/body per provider; also `summary_engine/` for built-in |
| Toggle LLM debug logging | Summary | `summary/debug_log.rs` | `DEBUG = true` compile-time flag; writes per-call files to meeting folder |
| Change the DB schema | Database | `database/migrations/*`, `database/models.rs`, `repositories/` | sqlx runtime queries; `source_device`/`speaker` drift is a known gotcha |
| Add a transcript pagination tweak | Frontend | `hooks/usePaginatedTranscripts.ts`, `components/VirtualizedTranscriptView.tsx`, `api/api.rs` | Page size 100, `api_get_meeting_transcripts` |
| Add/change speaker diarization | Audio | `audio/diarization.rs`, `audio/online_diarization.rs` | polyvoice engine; label scheme `MIC_SPEAKER_NN`/`SPEAKER_NN` |
| Add diarization model to catalog | Audio | `audio/diarization.rs` (`check_diarization_models`/`download_diarization_models`) | polyvoice manifest + ONNX paths |
| Change audio playback behavior | Audio + Frontend | `audio/audio_file.rs`, `hooks/useAudioPlayer.ts`, `components/AudioPlayer.tsx` | `convertFileSrc` + FFmpeg WAV fallback |
| Change the recording-start model gate | Audio + tray | `audio/transcription/commands.rs`, `engine.rs` | `check_active_transcription_model_ready` (provider-aware) |
| Change mic/system transcript colors | Frontend | `components/VirtualizedTranscriptView.tsx` | Mic=blue left, System=green right |
| Add a UI component | Frontend | `components/`, `components/ui/` | Shadcn/ui primitives + `cn()` |
| Change GPU build features | Ops | `frontend/scripts/tauri-auto.js`, `auto-detect-gpu.js`, `scripts/env-cuda.*` | `TAURI_GPU_FEATURE` override |
| Bump the app version | Ops/UI | `frontend/src-tauri/Cargo.toml`, `tauri.conf.json`, `components/Sidebar/index.tsx` | Update all three (per AGENTS.md) |

## Entry Points by Role

| Role | Start Here | Key Files |
|------|-----------|-----------|
| Full-stack developer | `docs/CODEBASE_MAP_ARCHITECTURE.md` | All module docs |
| Frontend developer | `CODEBASE_MAP_MODULE_FRONTEND_APP.md` + `FRONTEND_COMPONENTS.md` + `FRONTEND_HOOKS.md` | `frontend/src/app/`, `components/`, `hooks/`, `contexts/` |
| Backend/Rust developer | `CODEBASE_MAP_ARCHITECTURE.md` → Rust section | `audio/`, `whisper_engine/`, `parakeet_engine/`, `summary/`, `database/` |
| Build/DevOps engineer | `CODEBASE_MAP_OPERATIONS.md` | `frontend/build-gpu.*`, `dev-gpu.*`, `scripts/`, `llama-helper/` |

## Cross-Reference Map

### "I changed X, what else might be affected?"

| Change | Also Check | Why |
|--------|------------|-----|
| Audio capture / device selection | `audio/stream.rs`, `devices/`, `recording_commands.rs`, frontend `DeviceSelection` | Device flows through all layers |
| VAD or pipeline | `audio/vad.rs`, `pipeline.rs`, `retranscription.rs`, `import.rs` | Shared VAD + segment helpers in `common.rs` |
| Transcription provider | `audio/transcription/`, `whisper_engine/`, `parakeet_engine/`, `transcript_settings` config | Provider switch affects Rust + frontend + retranscription |
| Database schema | `database/migrations`, `models.rs`, `repositories/`, `api/api.rs` DTOs | Schema → repo → API → UI chain |
| Diarization / speaker labels | `audio/diarization.rs`, `online_diarization.rs`, `database/repositories/meeting.rs`, `VirtualizedTranscriptView.tsx` | Labels flow Rust → DB → UI; `speaker_label` is dropped by `save_transcript` |
| Summary config/LLM | `summary/service.rs`, `llm_client.rs`, AI provider modules | Config → client → API call chain |
| Recording UI | `contexts/TranscriptContext.tsx`, `RecordingControls`, `hooks/useRecording*` | UI ↔ backend IPC must stay in sync |

### "Where is feature X implemented?"

| Feature | Primary File(s) | Secondary Files |
|---------|-----------------|-----------------|
| Audio capture (mic/system) | `audio/stream.rs`, `audio/recording_state.rs` | `audio/capture/`, `audio/devices/` |
| Voice Activity Detection | `audio/vad.rs` | `audio/pipeline.rs` (dual VAD) |
| Live transcription | `audio/transcription/worker.rs` + `engine.rs` | `whisper_engine/`, `parakeet_engine/` |
| Stereo recording file | `audio/pipeline.rs` (interleave), `audio/recording_saver.rs` | `audio/incremental_saver.rs` |
| Re-transcription ("Enhance") | `audio/retranscription.rs` | `audio/common.rs` |
| Audio import | `audio/import.rs` | frontend `ImportAudio/` (beta) |
| Speaker diarization (offline) | `audio/diarization.rs` | `audio/online_diarization.rs`, `database/repositories/meeting.rs` |
| Speaker diarization (online) | `audio/online_diarization.rs` | `audio/recording_commands.rs`, `audio/pipeline.rs` |
| Meeting audio playback | `audio/audio_file.rs` | `hooks/useAudioPlayer.ts`, `components/AudioPlayer.tsx`, `api/api.rs` |
| AI summarization | `summary/service.rs`, `processor.rs` | `summary/llm_client.rs`, `summary_engine/` |
| Transcript pagination | `hooks/usePaginatedTranscripts.ts` | `api/api.rs`, `database/repositories/meeting.rs` |
| Meeting storage | `database/manager.rs`, `models.rs`, `repositories/` | `api/api.rs` |
| Desktop notifications | `notifications/` | Settings page |
| Analytics | `analytics/` | `lib/analytics` (frontend) |

## File Search Patterns

- **All Tauri commands**: `rg "#\[tauri::command\]" frontend/src-tauri/src`
- **All database queries**: `rg "sqlx::" frontend/src-tauri/src/database`
- **All `transcript-update` events**: `rg "transcript-update" frontend/src-tauri/src`
- **Provider dispatch**: `rg "LLMProvider" frontend/src-tauri/src/summary`

## Documentation Hierarchy

```
docs/
├── CODEBASE_MAP.md                       ← Main index
├── CODEBASE_MAP_ARCHITECTURE.md          ← System architecture overview
├── CODEBASE_MAP_MODULES.md               ← Module index with cross-refs
│   ├── CODEBASE_MAP_MODULE_AUDIO.md
│   ├── CODEBASE_MAP_MODULE_WHISPER.md
│   ├── CODEBASE_MAP_MODULE_PARAKEET.md
│   ├── CODEBASE_MAP_MODULE_SUMMARY.md
│   ├── CODEBASE_MAP_MODULE_AI_PROVIDERS.md
│   ├── CODEBASE_MAP_MODULE_DATABASE.md
│   ├── CODEBASE_MAP_MODULE_NOTIFICATIONS.md
│   ├── CODEBASE_MAP_MODULE_ANALYTICS.md
│   ├── CODEBASE_MAP_MODULE_FRONTEND_APP.md
│   ├── CODEBASE_MAP_MODULE_FRONTEND_COMPONENTS.md
│   └── CODEBASE_MAP_MODULE_FRONTEND_HOOKS.md
├── CODEBASE_MAP_DATA_FLOW.md            ← Data flow diagrams
├── CODEBASE_MAP_CONVENTIONS.md          ← Naming & style conventions
├── CODEBASE_MAP_OPERATIONS.md           ← Build, deploy, run
└── CODEBASE_MAP_NAVIGATION.md           ← This file (quick navigation)
```

## Navigation Tips

1. **Start with architecture** → Understand the system before diving into modules.
2. **Follow the data flow** → Trace a recording from mic/system capture to storage and summary.
3. **Use conventions** → Naming patterns help locate files quickly.
4. **Check dependencies** → `rg` for imports to find related code.
5. **Read AGENTS.md** → It lists recent additions and the version-bump locations.
