---
parent: CODEBASE_MAP.md
last_mapped: 2026-07-13T14:40:00Z
section: navigation
---

> Part of [Codebase Map](CODEBASE_MAP.md)

# Navigation Guide

## Quick Start Paths

### "I want to understand the project structure"
→ Read [`CODEBASE_MAP_ARCHITECTURE.md`](CODEBASE_MAP_ARCHITECTURE.md) for high-level overview

### "I need to modify recording functionality"
→ See [`CODEBASE_MAP_MODULE_AUDIO.md`](CODEBASE_MAP_MODULE_AUDIO.md) and [`CODEBASE_MAP_MODULES.md`](CODEBASE_MAP_MODULES.md#frontend-recording-page)

### "I want to add a new transcription provider"
→ See [`CODEBASE_MAP_MODULE_WHISPER.md`](CODEBASE_MAP_MODULE_WHISPER.md) and [`CODEBASE_MAP_MODULE_PARAKEET.md`](CODEBASE_MAP_MODULE_PARAKEET.md)

### "I need to change the AI summarization logic"
→ See [`CODEBASE_MAP_MODULE_SUMMARY.md`](CODEBASE_MAP_MODULE_SUMMARY.md) and [`CODEBASE_MAP_MODULE_AI_PROVIDERS.md`](CODEBASE_MAP_MODULE_AI_PROVIDERS.md)

### "I'm building a new UI component"
→ See [`CODEBASE_MAP_MODULE_FRONTEND_COMPONENTS.md`](CODEBASE_MAP_MODULE_FRONTEND_COMPONENTS.md)

## Entry Points by Role

| Role | Start Here | Key Files |
|------|-----------|-----------|
| **Full-stack developer** | `README.md` → `CODEBASE_MAP_ARCHITECTURE.md` | All module files |
| **Frontend developer** | `CODEBASE_MAP_MODULE_FRONTEND_APP.md` + `CODEBASE_MAP_MODULE_FRONTEND_COMPONENTS.md` | `frontend/src/app/`, `frontend/src/components/` |
| **Backend/Rust developer** | `CODEBASE_MAP_ARCHITECTURE.md` → Rust section | `src/audio/`, `src/whisper_engine/`, `src/summary/` |
| **Python backend dev** | `backend/README.md` | `backend/app/` |
| **DevOps/Build engineer** | `CODEBASE_MAP_OPERATIONS.md` | Dockerfiles, build scripts |

## Module Quick Reference

### Rust Backend (`src/`)

```
src/
├── lib.rs                    ← Command registration (START HERE)
├── audio/                    ← Audio capture module
│   ├── mod.rs                ← Re-exports
│   └── device_manager.rs     ← Device enumeration
├── whisper_engine/           ← Whisper.cpp wrapper
│   ├── mod.rs
│   └── engine.rs             ← Core inference
├── parakeet/                 ← Parakeet integration
│   ├── mod.rs
│   └── client.rs             ← API client
├── summary/                  ← AI summarization
│   ├── mod.rs
│   ├── llm_client.rs         ← Trait definition
│   ├── service.rs            ← Orchestration
│   ├── ollama/               ← Ollama provider
│   ├── openai/               ← OpenAI provider
│   ├── anthropic/            ← Anthropic provider
│   ├── groq/                 ← Groq provider
│   └── openrouter/           ← OpenRouter provider
├── database/                 ← SQLite layer
│   ├── mod.rs
│   ├── models.rs             ← Schema structs
│   └── setup.rs              ← Migration logic
├── notifications/            ← Desktop notifications
│   └── mod.rs
├── analytics/                ← Usage tracking
│   └── mod.rs
└── main.rs                   ← Tauri app entry point
```

### Frontend (`frontend/src/`)

```
frontend/src/
├── app/                      ← Next.js App Router
│   ├── layout.tsx            ← Root layout (START HERE)
│   ├── page.tsx              ← Home page
│   └── ...                   ← Other pages
├── components/               ← UI Components
│   ├── ui/                   ← Shadcn/ui primitives
│   ├── features/             ← Feature components
│   └── icons/                ← SVG icons
├── hooks/                    ← Custom React hooks
├── lib/                      ← Utilities & config
├── stores/                   ← Zustand stores
└── types/                    ← TypeScript types
```

### Backend Server (`backend/app/`)

```
backend/app/
├── main.py                   ← FastAPI entry point (START HERE)
├── models.py                 ← Pydantic models
├── config.py                 ← Configuration
├── whisper_server.py         ← Whisper server wrapper
└── ...                       ← Other endpoints
```

## Cross-Reference Map

### "I changed X, what else might be affected?"

| Change | Also Check | Why |
|--------|------------|-----|
| Audio device selection | `audio/device_manager.rs`, `useAudioDevices` hook | Device flows through all layers |
| Transcription provider | `whisper_engine/`, `parakeet/`, config | Provider switch affects both Rust and frontend |
| Database schema | `database/models.rs`, migrations, API responses | Schema changes propagate to UI |
| LLM provider config | `summary/service.rs`, AI providers module | Config → client → API call chain |
| Recording page UI | `useRecording` hook, Tauri commands | UI ↔ Backend IPC must stay in sync |

### "Where is feature X implemented?"

| Feature | Primary File(s) | Secondary Files |
|---------|-----------------|-----------------|
| Audio capture | `src/audio/device_manager.rs` | `frontend/src/hooks/use-audio-devices.ts` |
| Live transcription | `src/whisper_engine/engine.rs` | `frontend/src/components/features/transcript-display.tsx` |
| Meeting storage | `src/database/models.rs` | `frontend/src/app/page.tsx` (meeting list) |
| AI summarization | `src/summary/service.rs` | `frontend/src/components/features/summary-viewer.tsx` |
| Desktop notifications | `src/notifications/mod.rs` | Settings page |
| Analytics tracking | `src/analytics/mod.rs` | Background, no UI |
| GPU acceleration | `Cargo.toml` (features) | `whisper_engine/engine.rs` |

## File Search Patterns

### "Find all Tauri commands"
```bash
grep -r "#\[tauri::command\]" src/
```

### "Find all database queries"
```bash
grep -r "sqlx\|query!" src/database/
```

### "Find all API client implementations"
```bash
find frontend/src/components/features -name "*selector*"
find src/summary -name "*.rs" | xargs grep -l "send_request"
```

### "Find all Zustand stores"
```bash
grep -r "create<" frontend/src/stores/
```

## Architecture Decision Records (Key Locations)

| Decision | Documented In | Implementation |
|----------|---------------|----------------|
| Tauri over Electron | `README.md` → Architecture section | `frontend/src-tauri/` |
| Whisper.cpp via Rust bindings | `src/whisper_engine/Cargo.toml` | `src/whisper_engine/engine.rs` |
| Parakeet for streaming | `backend/parakeet/` + docs | `src/parakeet/client.rs` |
| SQLite over Postgres | `src/database/setup.rs` | `sqlx` dependency |
| Zustand over Redux | `frontend/src/stores/` | `zustand` package |
| Shadcn/ui for components | `frontend/src/components/ui/` | Copied primitives |

## Common Developer Tasks Quick Links

| Task | Files to Edit | Files to Check |
|------|---------------|----------------|
| Add new setting | Settings page + config struct | `lib/config.ts`, `src/summary/service.rs` |
| Add new API provider | `src/summary/{provider}/` | `llm_client.rs` trait |
| Modify recording UI | `frontend/src/components/features/recording-controls.tsx` | `useRecording` hook |
| Change database schema | `src/database/models.rs`, `setup.rs` | Migration logic |
| Add new page route | `frontend/src/app/{new-page}/page.tsx` | `layout.tsx` |
| Update dependencies | `Cargo.toml`, `package.json`, `requirements.txt` | Lock files |
| Change build config | `tauri.conf.json`, `Dockerfile.*` | Build scripts |

## Documentation Hierarchy

```
docs/
├── CODEBASE_MAP.md                    ← Main index (create first)
├── CODEBASE_MAP_ARCHITECTURE.md       ← System architecture overview
├── CODEBASE_MAP_MODULES.md            ← Module index with cross-refs
│   ├── CODEBASE_MAP_MODULE_AUDIO.md   ← Audio capture module
│   ├── CODEBASE_MAP_MODULE_WHISPER.md ← Whisper.cpp wrapper
│   ├── CODEBASE_MAP_MODULE_PARAKEET.md← Parakeet streaming
│   ├── CODEBASE_MAP_MODULE_SUMMARY.md ← AI summarization service
│   ├── CODEBASE_MAP_MODULE_DATABASE.md← SQLite data layer
│   ├── CODEBASE_MAP_MODULE_NOTIFICATIONS.md
│   ├── CODEBASE_MAP_MODULE_ANALYTICS.md
│   ├── CODEBASE_MAP_MODULE_AI_PROVIDERS.md
│   ├── CODEBASE_MAP_MODULE_FRONTEND_APP.md
│   ├── CODEBASE_MAP_MODULE_FRONTEND_COMPONENTS.md
│   └── CODEBASE_MAP_MODULE_FRONTEND_HOOKS.md
├── CODEBASE_MAP_DATA_FLOW.md          ← Data flow diagrams
├── CODEBASE_MAP_CONVENTIONS.md        ← Naming & style conventions
├── CODEBASE_MAP_OPERATIONS.md         ← Build, deploy, run
└── CODEBASE_MAP_NAVIGATION.md         ← This file (quick navigation)
```

## Navigation Tips

1. **Start with architecture** → Understand the system before diving into modules
2. **Follow the data flow** → Trace a recording from capture to storage
3. **Use conventions** → Naming patterns help locate files quickly
4. **Check dependencies** → `grep` for imports to find related code
5. **Read tests** → Test files show expected behavior and edge cases