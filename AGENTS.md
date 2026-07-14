# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Codebase Map

**Full map**: See [docs/CODEBASE_MAP.md](docs/CODEBASE_MAP.md) for the comprehensive codebase index linking to:
- Architecture — System overview, architecture diagrams, directory structure ([CODEBASE_MAP_ARCHITECTURE.md](docs/CODEBASE_MAP_ARCHITECTURE.md))
- Modules Index — Module dependency graph and cross-module patterns ([CODEBASE_MAP_MODULES.md](docs/CODEBASE_MAP_MODULES.md))
- Audio Engine — Deep dive: capture, mixing, VAD, device management ([CODEBASE_MAP_MODULE_AUDIO.md](docs/CODEBASE_MAP_MODULE_AUDIO.md))
- Whisper Engine — Whisper.cpp integration and model management ([CODEBASE_MAP_MODULE_WHISPER.md](docs/CODEBASE_MAP_MODULE_WHISPER.md))
- Parakeet Engine — ONNX streaming transcription ([CODEBASE_MAP_MODULE_PARAKEET.md](docs/CODEBASE_MAP_MODULE_PARAKEET.md))
- Summary Service — AI summarization with multi-provider support ([CODEBASE_MAP_MODULE_SUMMARY.md](docs/CODEBASE_MAP_MODULE_SUMMARY.md))
- AI Providers — Ollama, OpenAI, Anthropic, Groq, OpenRouter adapters ([CODEBASE_MAP_MODULE_AI_PROVIDERS.md](docs/CODEBASE_MAP_MODULE_AI_PROVIDERS.md))
- Database — SQLite data layer and repositories ([CODEBASE_MAP_MODULE_DATABASE.md](docs/CODEBASE_MAP_MODULE_DATABASE.md))
- Notifications — Desktop notification system ([CODEBASE_MAP_MODULE_NOTIFICATIONS.md](docs/CODEBASE_MAP_MODULE_NOTIFICATIONS.md))
- Analytics — PostHog integration ([CODEBASE_MAP_MODULE_ANALYTICS.md](docs/CODEBASE_MAP_MODULE_ANALYTICS.md))
- Data Flow — Data pipelines and sequence diagrams ([CODEBASE_MAP_DATA_FLOW.md](docs/CODEBASE_MAP_DATA_FLOW.md))
- Conventions — Patterns, naming standards, architectural principles ([CODEBASE_MAP_CONVENTIONS.md](docs/CODEBASE_MAP_CONVENTIONS.md))
- Operations — Build, deploy, run commands, gotchas ([CODEBASE_MAP_OPERATIONS.md](docs/CODEBASE_MAP_OPERATIONS.md))
- Navigation — Getting started, common tasks, file quick reference ([CODEBASE_MAP_NAVIGATION.md](docs/CODEBASE_MAP_NAVIGATION.md))

**Recent additions** since last map update: `audio/device_detection.rs`, `audio/hardware_detector.rs`, `audio/incremental_saver.rs`, `audio/retranscription.rs`, `audio/import.rs`, `audio/post_processor.rs`, `audio/buffer_pool.rs`, `audio/batch_processor.rs`, `audio/async_logger.rs`, `audio/device_monitor.rs`, `audio/playback_monitor.rs`, `audio/transcription/` (provider abstraction), `summary/language_detection.rs`, `summary/metadata.rs`, `summary/template_commands.rs`.

## Project Overview

See [docs/PROJECT_OVERVIEW_FULL.md](docs/PROJECT_OVERVIEW_FULL.md) for the complete project overview including:
- Technology stack, development commands, architecture diagrams
- Audio pipeline details, Tauri IPC patterns, Whisper model management
- Critical development patterns, common tasks, platform-specific notes
- Performance guidelines, constraints/conventions, key file references

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

When the user types `/graphify`, use the installed graphify skill or instructions before doing anything else.

To run graphify command use python command `uv tool run --from graphifyy python`.

Rules:
- For codebase questions, first run `graphify query "<question>"` when graphify-out/graph.json exists. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- Dirty graphify-out/ files are expected after hooks or incremental updates; dirty graph files are not a reason to skip graphify. Only skip graphify if the task is about stale or incorrect graph output, or the user explicitly says not to use it.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
