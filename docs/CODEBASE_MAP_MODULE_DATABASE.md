---
parent: CODEBASE_MAP_MODULES.md
last_mapped: 2026-08-05T14:57:00Z
module: database
---

> Part of [Module Guide](CODEBASE_MAP_MODULES.md) | [Codebase Map](CODEBASE_MAP.md)

# Module: Database Layer

## Overview

**Purpose**: Complete persistence layer for meetings, transcripts, summary jobs, transcript chunks, and app config, built on **SQLite via sqlx 0.8** (tokio runtime). Owns the DB file lifecycle including legacy `.db` → `.sqlite` migration and WAL-corruption recovery. Recent changes added `source_device` (mic/system channel) storage on transcripts and paginated transcript queries.

> **Note:** Queries are **runtime strings** (`sqlx::query_as`, `query`), not compile-time `query!` macros — a notable tech-debt point.

**Entry point**: `database/mod.rs` — module root.

**Sub-packages**:
- `repositories/` — Five stateless, pool-taking repository structs doing raw SQL.

## File Reference

| File | Purpose | Key Exports | Tokens |
|------|---------|-------------|--------|
| `mod.rs` | Module root, declares 5 submodules | — | <1k |
| `manager.rs` | Connection pool + lifecycle, legacy import, WAL recovery | `DatabaseManager` | ~2k |
| `setup.rs` | Startup init (first-launch event vs immediate init) | `initialize_database_on_startup` | <1k |
| `commands.rs` | Tauri IPC for DB import/init/utility | `check_first_launch`, `initialize_fresh_database`, `import_and_initialize_database` | ~2k |
| `models.rs` | Entity structs (`FromRow`) | `MeetingModel`, `Transcript`, `SummaryProcess`, `TranscriptChunk`, `Setting`, `TranscriptSetting`, `DateTimeUtc` | ~1k |
| `repositories/mod.rs` | Repository module root | — | <1k |
| `repositories/meeting.rs` | Meeting + transcript CRUD, pagination | `MeetingsRepository` | ~2k |
| `repositories/transcript.rs` | Save meeting+segments transactionally, search | `TranscriptsRepository` | ~1k |
| `repositories/transcript_chunk.rs` | Persist full transcript + chunking params | `TranscriptChunksRepository` | <1k |
| `repositories/summary.rs` | Summary job state machine + result backup/restore | `SummaryProcessesRepository` | ~1.5k |
| `repositories/setting.rs` | Summary/transcript config + API keys | `SettingsRepository` | ~2.6k |

## Public API

### Key Functions (Tauri Commands — `database/commands.rs`)

| Function | Signature | Description |
|----------|-----------|-------------|
| `check_first_launch` | `(app) -> Result<bool, String>` | `!meeting_minutes.sqlite.exists()` |
| `select_legacy_database_path` | `(app) -> Result<Option<String>, String>` | OS file dialog for legacy `.db` |
| `detect_legacy_database` | `(selected_path: String) -> Result<Option<String>, String>` | Heuristic detection |
| `check_homebrew_database` | `(path: String) -> Result<Option<DatabaseCheckResult>, String>` | Detect old Python-backend installs |
| `import_and_initialize_database` | `(app, legacy_db_path) -> Result<(), String>` | Import + manage AppState; emits `database-initialized` |
| `initialize_fresh_database` | `(app) -> Result<(), String>` | Fresh init + seeds default model configs |
| `get_database_directory` / `open_database_folder` | `(app) -> Result<.., String>` | DB dir helpers |

> Most data access from the frontend goes through the **`api/api.rs`** commands (e.g. `api_get_meetings`, `api_get_meeting_transcripts`, `api_save_transcript`) which wrap these repositories — not through `database/commands.rs`.

### Key Types

```rust
struct DatabaseManager {
    pool: SqlitePool,
}
// new(tauri_db_path, backend_db_path), new_from_app_handle, is_first_launch,
// import_legacy_database, pool(), with_transaction, cleanup()

struct Transcript {                       // models.rs — includes audio sync + device channel
    id: String, meeting_id: String, transcript: String, timestamp: String,
    summary: Option<String>, action_items: Option<String>, key_points: Option<String>,
    audio_start_time: Option<f64>, audio_end_time: Option<f64>, duration: Option<f64>,
    source_device: Option<String>,       // 'mic' | 'system'  (mic/system channel)
}
```

### Schema (from 11 embedded migrations)

```mermaid
erDiagram
    meetings ||--o{ transcripts : has
    meetings ||--o{ summary_processes : has
    meetings ||--o{ transcript_chunks : has
    meetings ||--o{ meeting_notes : has
    meetings {
        string id PK
        string title
        datetime created_at
        datetime updated_at
        string folder_path
    }
    transcripts {
        string id PK
        string meeting_id FK
        string transcript
        string timestamp
        real audio_start_time
        real audio_end_time
        real duration
        string speaker          -- added 2025-11 (values 'mic'/'system')
        string source_device    -- added 2026-07 (mic/system)
    }
    settings { string id PK, string provider, string model, ... api keys, customOpenAIConfig }
    transcript_settings { string id PK, string provider, string model, ... api keys }
    summary_processes { string meeting_id PK, string status, string result, ... result_backup }
    transcript_chunks { string meeting_id PK, string transcript_text, ... }
    licensing { string license_key PK, ... }
    meeting_notes { string meeting_id PK, ... }
```

## Internal Architecture

- **manager.rs**: single managed `DatabaseManager` in `AppState`; `new_from_app_handle` resolves `app_data_dir()` (`meeting_minutes.sqlite` primary, `meeting_minutes.db` legacy), runs `sqlx::migrate!`, and on "malformed"/"corrupt" error deletes `-wal`/`-shm` and retries once. `cleanup()` runs `PRAGMA wal_checkpoint(TRUNCATE)` then closes pool.
- **setup.rs**: on first launch spawns a 500ms-delayed `first-launch-detected` event (AppState NOT yet managed); otherwise initializes immediately.
- **repositories/**: raw-SQL unit structs taking `&SqlitePool`. `MeetingsRepository::get_meeting_transcripts_paginated` orders by `audio_start_time` and returns `(Vec<Transcript>, total)` for infinite scroll. `TranscriptsRepository::save_transcript` inserts meeting + segments atomically (incl. `audio_start_time/end_time/duration/source_device`).
- **summary.rs**: `SummaryProcessesRepository` implements a `PENDING → completed/failed/cancelled` state machine with **result backup/restore**: `create_or_reset_process` backs up `result`→`result_backup`; `update_process_failed`/`cancelled` restores `result = COALESCE(result_backup, result)`.
- **setting.rs**: singleton config rows `id='1'` via UPSERT; per-provider API keys; custom OpenAI as JSON blob.

## Dependencies (imports FROM)

| Module/Package | What is imported | Why |
|---------------|-----------------|-----|
| `sqlx` | `SqlitePool`, `Row`, `Connection`, `Transaction` | SQLite pool/querying |
| `chrono` | `DateTime<Utc>` | Timestamps |
| `api::api` | `MeetingDetails`, `MeetingTranscript`, `TranscriptSegment`, `TranscriptSearchResult` | DTOs consumed/produced |
| `summary` | `CustomOpenAIConfig` | Custom OpenAI config parsing |
| `config` | `DEFAULT_PARAKEET_MODEL` | Default seeding |
| `summary::summary_engine::commands` | `get_recommended_summary_model_for_current_system` | Default model seeding |

## Dependents (imported BY)

| Consumer Module | What it uses | Context |
|----------------|-------------|---------|
| `api/api.rs` | All repositories | Most `api_*` commands are SQLite IPC wrappers |
| `summary/` (service, commands) | `MeetingsRepository`, `SummaryProcessesRepository`, `TranscriptChunksRepository`, `SettingsRepository` | Summary pipeline + config |
| `state.rs`, `lib.rs` | `DatabaseManager` | Managed state, exit cleanup |
| `audio/` | via api/repositories | Transcript persistence |

## Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| DB file | `app_data_dir()/meeting_minutes.sqlite` | Primary SQLite (WAL mode implied by `-wal`/`-shm`) |
| Legacy file | `app_data_dir()/meeting_minutes.db` | Auto-import source |
| Migrations | embedded `./migrations` dir | Applied at open |
| Default seed | summary provider `builtin-ai` + recommended model (fallback `qwen3.5:2b`); whisper `large-v3`; transcription `parakeet` + `DEFAULT_PARAKEET_MODEL` | Fresh init / onboarding |
| Config rows | singleton `id='1'` | `settings` + `transcript_settings` |

## Error Handling

- `sqlx::Error` throughout; repos return `SqlxError::Protocol` for invalid input (e.g. empty meeting_id), `RowNotFound` when absent.
- `SqlxError::Io` for filesystem failures; WAL-corruption recovery via error-string matching + single retry.
- `save_api_key`/`get_api_key` use **dynamic SQL column interpolation** (`format!`) — column names come from a fixed provider→column match, not user input.
- `cleanup` treats WAL-checkpoint failure as non-fatal.

## Concurrency and Thread Safety

- Single shared `SqlitePool` (tokio, `Send+Sync`), WAL journaling, explicit transactions via `pool.begin()`/`conn.begin()`, `ON CONFLICT DO UPDATE` upserts.
- No compile-time query checking (runtime SQL).
- `app_data_dir().expect(...)` panics on failure.

## Gotchas and Tech Debt

- **`speaker` vs `source_device` drift**: schema has both columns for mic/system channel; the Rust `Transcript` model only exposes `source_device` (added 2026-07). The `speaker` column is unreachable from Rust — two competing fields.
- **`update_meeting_title` vs `update_meeting_name`** are near-duplicates (one also updates `transcript_chunks.meeting_name`); naming confusing.
- **Runtime SQL** (no `query!`), and `SELECT *` in `get_meeting` vs explicit columns elsewhere.
- **Dynamic column interpolation** is SQL-injection-adjacent (mitigated by fixed match).
- **Hardcoded defaults** in `save_api_key` (`openai`/`gpt-4o-2024-11-20`/`large-v3`) override whatever provider/model the caller intended — footgun.
- **No FTS5**: `LOWER(transcript) LIKE '%q%'` → O(N) full scans on large transcripts.
- `with_transaction` on `DatabaseManager` is unused (repos open transactions directly).
- Default-seeding logic duplicated in `onboarding.rs`; `geminiApiKey` column not in `Setting` model; `licensing`/`meeting_notes` tables have no repo.
- `search_transcripts` decodes tuples rather than a `FromRow` struct.
