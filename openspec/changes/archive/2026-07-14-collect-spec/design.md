## Context

Meetily is a Tauri-based desktop application with a Rust backend (frontend/src-tauri/src/) and Next.js frontend. The backend consists of 7 core modules that handle the primary user-facing capabilities: audio recording/transcription, Whisper transcription engine, Parakeet transcription engine, AI summary generation, SQLite database, notifications, and analytics. Each module has evolved independently with documentation scattered across `docs/CODEBASE_MAP_*.md` files, but no formal behavioral specifications exist in the OpenSpec format. This change collects specs for all 7 core modules to establish what each capability provides as a single source of truth.

## Goals / Non-Goals

**Goals:**
- Create one spec per module under `openspec/specs/<module>/spec.md` (7 total)
- Each spec documents: purpose, key types/traits, public Tauri commands, requirements with scenarios
- Specs use SHALL/MUST for normative requirements; each requirement has WHEN/THEN scenarios
- Specs serve as reference for future refactoring and onboarding

**Non-Goals:**
- No behavioral changes to any module — this is documentation-only
- No new features or API changes
- No frontend spec collection (Next.js components)
- No LLM provider specs (ollama, openai, anthropic, groq, openrouter are support modules, not user-facing capabilities)

## Decisions

1. **One spec per module** — Each of the 7 core backend modules gets its own `specs/<name>/spec.md` file. This maps one-to-one with physical module boundaries in `src-tauri/src/`.

2. **ADDED Requirements only** — Since no existing OpenSpec specs exist, all requirements are new (ADDED). No delta spec workflow needed.

3. **Scope: Rust backend modules only** — Specs cover the Tauri backend modules that implement user-facing capabilities. Support modules (LLM providers, API layer) are excluded to keep scope focused.

4. **Requirements from code + docs** — Requirements are extracted from existing module code (`mod.rs` exports, `commands.rs` functions) and documentation (`docs/CODEBASE_MAP_*.md`, `AGENTS.md`).

## Risks / Trade-offs

- [Risk] Specs may drift from implementation over time → Mitigation: specs document current behavior; mark as living docs to be updated with code changes
- [Risk] Large spec files per module → Mitigation: keep requirements focused on external interfaces and Tauri commands, not internal implementation details
- [Risk] Missing edge cases in scenario coverage → Mitigation: cover primary user flows; scenarios are examples, not exhaustive test suites
