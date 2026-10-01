# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Codebase Map

**Map**: See [docs/CODEBASE_MAP.md](docs/CODEBASE_MAP.md) for the thin, stable overview (system summary and core capabilities), linking to:
- Architecture — System overview, architecture diagrams, layer descriptions ([CODEBASE_MAP_ARCHITECTURE.md](docs/CODEBASE_MAP_ARCHITECTURE.md))
- Conventions — Patterns, naming standards, architectural principles ([CODEBASE_MAP_CONVENTIONS.md](docs/CODEBASE_MAP_CONVENTIONS.md))
- Operations — Build, test, lint, CI, crash diagnostics, gotchas ([CODEBASE_MAP_OPERATIONS.md](docs/CODEBASE_MAP_OPERATIONS.md))

**File-, module- and symbol-level navigation**: use graphify (see the graphify section below) — `graphify query`/`explain`/`path`, the wiki at `graphify-out/wiki/index.md` (gitignored; generate on demand with `graphify export wiki` after `graphify update .`), or `graphify-out/GRAPH_REPORT.md`.

**Changelog**: for what has shipped, see `openspec/changes/archive/` (browse newest-first).

## Project Overview

See [docs/PROJECT_OVERVIEW_FULL.md](docs/PROJECT_OVERVIEW_FULL.md) for the complete project overview including:
- Technology stack, development commands, architecture diagrams
- Audio pipeline details, Tauri IPC patterns, Whisper model management
- Critical development patterns, common tasks, platform-specific notes
- Performance guidelines, constraints/conventions, key file references

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

When the user types `/graphify`, use the installed graphify skill or instructions before doing anything else.

Rules:
- For codebase questions, first run `graphify query "<question>"` when graphify-out/graph.json exists. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- Dirty graphify-out/ files are expected after hooks or incremental updates; dirty graph files are not a reason to skip graphify. Only skip graphify if the task is about stale or incorrect graph output, or the user explicitly says not to use it.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).

## Version Changes

When bumping the app version, update **all three** locations:
- `frontend/src-tauri/Cargo.toml` — Rust package version
- `frontend/src-tauri/tauri.conf.json` — Tauri app version (this is what the running app reads)
- Any hardcoded version strings in UI components (e.g. `Sidebar/index.tsx`)

Tauri ignores `Cargo.toml` for its runtime version; it only reads `tauri.conf.json`.

## 1. Think Before Coding

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:
- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them - don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

## 2. Simplicity First

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

## 3. Surgical Changes

**Touch only what you must. Clean up only your own mess.**

When editing existing code:
- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it - don't delete it.

When your changes create orphans:
- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

Keep Unicode emoji (e.g. 🔧 🚀 ✅) in scripts and other files exactly as they are. When editing or rewriting a file that already contains emoji, preserve every emoji that was there — don't strip, replace, or "clean up" existing emoji, and don't reword content in a way that drops them.

The test: Every changed line should trace directly to the user's request.

## 4. Goal-Driven Execution

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:
- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:
```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

---
