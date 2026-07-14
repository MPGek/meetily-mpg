## Why

Meetily has grown into a complex desktop application with multiple independently-developed modules: Audio Engine, Whisper Engine, Parakeet Engine, Summary Service, Database Layer, Notifications, and Analytics. Each module has its own documentation in `docs/CODEBASE_MAP_*.md`, but there is no centralized specification of what each module *does* — the behavioral contracts, interfaces, and capabilities are scattered across markdown docs and source code. This change collects formal specs for all core modules to establish a single source of truth for what each capability provides, enabling safer refactoring, clearer onboarding, and better cross-module reasoning.

## What Changes

- Create spec files (`openspec/specs/<module>/spec.md`) for each core module in the codebase
- Each spec captures: purpose, key types/traits, public interfaces, requirements, and cross-module interactions
- Specs are stored under `openspec/specs/` following the OpenSpec schema-driven format

## Capabilities

### New Capabilities
- `audio-engine`: Audio capture, mixing, VAD, device detection, stream management, level monitoring
- `whisper-engine`: Whisper.cpp integration, model management, GPU acceleration, transcription provider interface
- `parakeet-engine`: ONNX streaming transcription, model downloads, parallel processing
- `summary-service`: AI summarization with multi-provider support (Ollama, OpenAI, Anthropic, Groq, OpenRouter), caching, language detection
- `database`: SQLite data layer, meeting/transcript models, repositories, migrations
- `notifications`: Desktop notification system, consent management, settings, DND awareness
- `analytics`: PostHog integration, event tracking, consent gating, analytics client configuration

### Modified Capabilities
<!-- None yet — all specs are new -->

## Impact

All core Rust backend modules (`src-tauri/src/audio/`, `src-tauri/src/whisper/`, `src-tauri/src/parakeet/`, `src-tauri/src/summary/`, `src-tauri/src/database/`, `src-tauri/src/notifications/`, `src-tauri/src/analytics/`). Frontend Tauri command handlers that surface these modules. No breaking changes to runtime behavior — this is a documentation/spec collection effort only.
