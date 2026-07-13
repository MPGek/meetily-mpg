# AGENTS.md — Meetily Codebase Map Summary

> Generated: 2026-07-13  
> A concise summary for AI agents navigating the Meetily codebase.

## Project Overview

**Meetily** is an AI-powered desktop meeting recorder and summarizer built with:
- **Tauri 2.x** — Cross-platform desktop framework (Rust backend + web frontend)
- **Next.js 14+** — React UI with App Router
- **Rust** — Audio capture, Whisper.cpp inference, SQLite storage
- **Python** — Optional FastAPI server for remote AI processing

## Codebase Map Location

Full documentation is in `docs/CODEBASE_MAP.md` — a comprehensive map of the entire codebase.

### Quick Links

| Purpose | File |
|---------|------|
| Main index | [`docs/CODEBASE_MAP.md`](docs/CODEBASE_MAP.md) |
| Architecture | [`docs/CODEBASE_MAP_ARCHITECTURE.md`](docs/CODEBASE_MAP_ARCHITECTURE.md) |
| All modules | [`docs/CODEBASE_MAP_MODULES.md`](docs/CODEBASE_MAP_MODULES.md) |
| Data flow | [`docs/CODEBASE_MAP_DATA_FLOW.md`](docs/CODEBASE_MAP_DATA_FLOW.md) |
| Conventions | [`docs/CODEBASE_MAP_CONVENTIONS.md`](docs/CODEBASE_MAP_CONVENTIONS.md) |
| Operations | [`docs/CODEBASE_MAP_OPERATIONS.md`](docs/CODEBASE_MAP_OPERATIONS.md) |
| Navigation | [`docs/CODEBASE_MAP_NAVIGATION.md`](docs/CODEBASE_MAP_NAVIGATION.md) |

### Key Directories

| Directory | Purpose |
|-----------|---------|
| `src/` | Rust backend — audio, whisper, parakeet, summary, database |
| `frontend/src/` | Next.js app — pages, components, hooks, stores |
| `frontend/src-tauri/` | Tauri bundle config and Rust crate |
| `backend/app/` | Python FastAPI server (optional) |

### Core Modules

| Module | Location | Description |
|--------|----------|-------------|
| Audio | `src/audio/` | PortAudio capture, device management |
| Whisper | `src/whisper_engine/` | whisper.cpp Rust bindings for local transcription |
| Parakeet | `src/parakeet/` | Streaming API client for remote transcription |
| Summary | `src/summary/` | AI summarization orchestration + providers |
| Database | `src/database/` | SQLite with sqlx async driver |

## Development Commands

```bash
# Frontend dev
cd frontend && pnpm install && pnpm tauri dev

# Rust build
cargo build --release

# Python backend (optional)
cd backend && pip install -r requirements.txt && python main.py
```

## Key Patterns

- **Tauri commands**: `#[tauri::command]` in Rust → `invoke()` from React
- **State management**: Zustand stores in frontend
- **Error handling**: Result<T, String> for Tauri commands
- **Async**: tokio for Rust, native fetch/axios for frontend