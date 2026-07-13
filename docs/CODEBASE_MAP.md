# Meetily Codebase Map

> Generated: 2026-07-13  
> Project: [Meetily](https://github.com/Zackriyaa-Solutions/meetily) — AI-powered meeting recorder & summarizer  
> Architecture Tier: **Large** (897k tokens, 500+ files across Rust/TS/Python)

---

## Quick Navigation

| Section | File | Description |
|---------|------|-------------|
| 🏗️ Architecture | [`CODEBASE_MAP_ARCHITECTURE.md`](CODEBASE_MAP_ARCHITECTURE.md) | System architecture, tech stack, deployment |
| 📦 Modules Index | [`CODEBASE_MAP_MODULES.md`](CODEBASE_MAP_MODULES.md) | All modules with file references and APIs |
| 🔄 Data Flow | [`CODEBASE_MAP_DATA_FLOW.md`](CODEBASE_MAP_DATA_FLOW.md) | Diagrams of data pipelines |
| 📐 Conventions | [`CODEBASE_MAP_CONVENTIONS.md`](CODEBASE_MAP_CONVENTIONS.md) | Naming, style, code organization |
| ⚙️ Operations | [`CODEBASE_MAP_OPERATIONS.md`](CODEBASE_MAP_OPERATIONS.md) | Build, deploy, run commands |
| 🧭 Navigation | [`CODEBASE_MAP_NAVIGATION.md`](CODEBASE_MAP_NAVIGATION.md) | Quick links by role and task |

---

## Module Index

### Rust Backend (`src/`)

| Module | File | Key Entry Point | Description |
|--------|------|-----------------|-------------|
| **Audio** | [`CODEBASE_MAP_MODULE_AUDIO.md`](CODEBASE_MAP_MODULE_AUDIO.md) | `src/audio/mod.rs` | Audio capture, device management, PortAudio |
| **Whisper Engine** | [`CODEBASE_MAP_MODULE_WHISPER.md`](CODEBASE_MAP_MODULE_WHISPER.md) | `src/whisper_engine/engine.rs` | Whisper.cpp wrapper, local transcription |
| **Parakeet** | [`CODEBASE_MAP_MODULE_PARAKEET.md`](CODEBASE_MAP_MODULE_PARAKEET.md) | `src/parakeet/client.rs` | Parakeet streaming API client |
| **Summary Service** | [`CODEBASE_MAP_MODULE_SUMMARY.md`](CODEBASE_MAP_MODULE_SUMMARY.md) | `src/summary/service.rs` | AI summarization orchestration |
| **AI Providers** | [`CODEBASE_MAP_MODULE_AI_PROVIDERS.md`](CODEBASE_MAP_MODULE_AI_PROVIDERS.md) | `src/summary/llm_client.rs` | Ollama, OpenAI, Anthropic, Groq, OpenRouter |
| **Database** | [`CODEBASE_MAP_MODULE_DATABASE.md`](CODEBASE_MAP_MODULE_DATABASE.md) | `src/database/models.rs` | SQLite data layer, meeting storage |
| **Notifications** | [`CODEBASE_MAP_MODULE_NOTIFICATIONS.md`](CODEBASE_MAP_MODULE_NOTIFICATIONS.md) | `src/notifications/mod.rs` | Desktop notification system |
| **Analytics** | [`CODEBASE_MAP_MODULE_ANALYTICS.md`](CODEBASE_MAP_MODULE_ANALYTICS.md) | `src/analytics/mod.rs` | Usage tracking & telemetry |

### Frontend (`frontend/src/`)

| Module | File | Key Entry Point | Description |
|--------|------|-----------------|-------------|
| **App Shell** | [`CODEBASE_MAP_MODULE_FRONTEND_APP.md`](CODEBASE_MAP_MODULE_FRONTEND_APP.md) | `frontend/src/app/layout.tsx` | Next.js app, routing, state management |
| **Components** | [`CODEBASE_MAP_MODULE_FRONTEND_COMPONENTS.md`](CODEBASE_MAP_MODULE_FRONTEND_COMPONENTS.md) | `frontend/src/components/ui/` | Shadcn/ui + custom UI components |
| **Hooks** | [`CODEBASE_MAP_MODULE_FRONTEND_HOOKS.md`](CODEBASE_MAP_MODULE_FRONTEND_HOOKS.md) | `frontend/src/hooks/` | Custom React hooks |

### Python Backend Server (`backend/app/`)

| Module | File | Key Entry Point | Description |
|--------|------|-----------------|-------------|
| **Server** | (see `backend/README.md`) | `backend/app/main.py` | FastAPI server for remote inference |

---

## Project Structure Overview

```
meetily/
├── README.md                    ← Project overview & getting started
├── CONTRIBUTING.md              ← Contribution guidelines
├── LICENSE.md                   ← Apache 2.0 license
├── PRIVACY_POLICY.md            ← Privacy policy
│
├── docs/                        ← Documentation (this map)
│   ├── CODEBASE_MAP.md          ← You are here
│   ├── CODEBASE_MAP_*.md        ← Map sub-files
│   └── ...                      ← Architecture diagrams, screenshots
│
├── src/                         ← Rust backend (Tauri app)
│   ├── main.rs                  ← Entry point
│   ├── lib.rs                   ← Command registration
│   ├── audio/                   ← Audio capture module
│   ├── whisper_engine/          ← Whisper.cpp wrapper
│   ├── parakeet/                ← Parakeet streaming client
│   ├── summary/                 ← AI summarization service
│   │   ├── ollama/              ← Ollama provider
│   │   ├── openai/              ← OpenAI provider
│   │   ├── anthropic/           ← Anthropic provider
│   │   ├── groq/                ← Groq provider
│   │   └── openrouter/          ← OpenRouter provider
│   ├── database/                ← SQLite data layer
│   ├── notifications/           ← Desktop notifications
│   └── analytics/               ← Usage tracking
│
├── frontend/                    ← Next.js + Tauri desktop app
│   ├── package.json             ← Node dependencies
│   ├── tauri.conf.json          ← Tauri config
│   ├── src-tauri/               ← Tauri Rust crate (backend)
│   │   ├── Cargo.toml           ← Rust deps for Tauri bundle
│   │   └── ...                  ← Duplicate of root src/ for Tauri
│   └── src/                     ← Next.js app source
│       ├── app/                 ← Pages (App Router)
│       ├── components/          ← UI components
│       ├── hooks/               ← Custom React hooks
│       ├── lib/                 ← Utilities & config
│       └── stores/              ← Zustand stores
│
├── backend/                     ← Python server (optional)
│   ├── requirements.txt         ← Python deps
│   ├── app/                     ← FastAPI application
│   ├── docker/                  ← Docker configs
│   └── *.sh / *.cmd             ← Build & run scripts
│
├── Cargo.toml                   ← Rust workspace config
└── .gitignore                   ← Git ignore rules
```

---

## Architecture Summary

Meetily is a **desktop application** built with:
- **Tauri 2.x** — Cross-platform desktop wrapper (Rust backend + web frontend)
- **Next.js 14+** — React UI framework (App Router, Server Components)
- **Rust** — Native audio capture, Whisper.cpp inference, SQLite storage
- **Python** — Optional server for remote AI processing

### Key Capabilities

| Capability | Technology | Module |
|------------|------------|--------|
| Audio recording | PortAudio → WAV | `src/audio/` |
| Live transcription | whisper.cpp / Parakeet API | `src/whisper_engine/`, `src/parakeet/` |
| AI summarization | Ollama / OpenAI / Anthropic / Groq / OpenRouter | `src/summary/` + providers |
| Meeting storage | SQLite (sqlx) | `src/database/` |
| Desktop notifications | Tauri notification API | `src/notifications/` |
| Analytics | Anonymous telemetry | `src/analytics/` |

---

## How to Use This Map

1. **Start with Architecture** → Read [`CODEBASE_MAP_ARCHITECTURE.md`](CODEBASE_MAP_ARCHITECTURE.md) for system overview
2. **Find a module** → Browse [`CODEBASE_MAP_MODULES.md`](CODEBASE_MAP_MODULES.md) for module index
3. **Dive deep** → Each module has its own detailed file with file references, APIs, and gotchas
4. **Follow data flow** → Use [`CODEBASE_MAP_DATA_FLOW.md`](CODEBASE_MAP_DATA_FLOW.md) to trace data through the system
5. **Check conventions** → [`CODEBASE_MAP_CONVENTIONS.md`](CODEBASE_MAP_CONVENTIONS.md) for naming and style rules
6. **Build & run** → [`CODEBASE_MAP_OPERATIONS.md`](CODEBASE_MAP_OPERATIONS.md) for commands and deployment
7. **Navigate by task** → [`CODEBASE_MAP_NAVIGATION.md`](CODEBASE_MAP_NAVIGATION.md) for role-based quick links

---

## Generation Details

- **Scanner**: `docs/scanner.mjs` (agent-driven exploration)
- **Tier**: Large (897k tokens, 500+ files analyzed)
- **Files generated**: 14 markdown files in `docs/`
- **Format**: Consistent per-module structure with frontmatter, file tables, API docs, gotchas