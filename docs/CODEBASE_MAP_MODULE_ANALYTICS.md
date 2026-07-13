---
parent: CODEBASE_MAP_MODULES.md
last_mapped: 2026-07-13T14:34:00Z
module: analytics
---

> Part of [Module Guide](CODEBASE_MAP_MODULES.md) | [Codebase Map](CODEBASE_MAP.md)

# Module: Analytics

## Overview

**Purpose**: The analytics module provides product analytics collection using PostHog, with user consent management. Tracks recording events, transcription usage, summary generation, and feature adoption for product improvement.

**Entry point**: `analytics/mod.rs` — module root
**Sub-packages**: None (single directory)

## File Reference

| File | Purpose | Key Exports | Tokens |
|------|---------|-------------|--------|
| `mod.rs` | Module root, re-exports all sub-modules | analytics types | ~1k |
| `analytics.rs` | Core analytics tracker | AnalyticsTracker struct, track events | ~6k |
| `commands.rs` | Tauri command handlers | get_consent, set_consent, track_event | ~4k |

## Public API

### Key Functions (Tauri Commands)

| Function | Signature | Description |
|----------|-----------|-------------|
| `get_analytics_consent` | `() -> bool` | Check if user has consented to analytics |
| `set_analytics_consent` | `(consent: bool) -> Result<(), String>` | Set analytics consent status |
| `track_recording_started` | `(meeting_name?) -> Result<(), String>` | Track recording start event |
| `track_recording_stopped` | `(duration?, meeting_name?) -> Result<(), String>` | Track recording stop event |
| `track_transcription_completed` | `(provider?, duration_ms?) -> Result<(), String>` | Track transcription completion |
| `track_summary_generated` | `(provider?, model?, token_count?) -> Result<(), String>` | Track summary generation |

### Key Types

```rust
struct AnalyticsTracker {
    enabled: bool,
    client: Option<posthog_rs::Client>,
}
```

## Internal Architecture

### Event Tracking Flow

1. **Consent Check**: `AnalyticsTracker` checks if user has consented
2. **Event Queue**: Events queued for batch sending (if supported)
3. **PostHog API Call**: Async HTTP POST to PostHog ingestion endpoint
4. **Error Handling**: Failed sends logged but not retried

### Concurrency Model

- Analytics tracking is non-blocking — events sent in background tasks
- `Arc<RwLock<AnalyticsTracker>>` for shared state across tasks

## Dependencies (imports FROM)

| Module/Package | What is imported | Why |
|---------------|-----------------|-----|
| `posthog-rs` (or similar) | PostHog client SDK | Analytics event sending |
| `tauri` | `Manager`, `AppHandle` | Tauri app context |

## Dependents (imported BY)

| Consumer Module | What it uses | Context |
|----------------|-------------|---------|
| `audio/` | Recording start/stop tracking | When recording begins/ends |
| `summary/` | Summary generation tracking | After AI summarization completes |
| `lib.rs` (main) | All Tauri commands | Entry point for frontend analytics control |

## Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `enabled` | false | Analytics disabled by default (privacy-first) |
| `consent_required` | true | Require user consent before tracking |
| `posthog_project_id` | From env | PostHog project identifier |
| `posthog_host` | https://app.posthog.com | PostHog ingestion endpoint |

## Error Handling

- **API failure**: Logged but not retried (fire-and-forget analytics)
- **Consent revoked**: All future events suppressed immediately
- **Network unavailable**: Events dropped silently

## Gotchas and Tech Debt

- **Privacy-first design**: Analytics opt-in by default — most users may never have tracking enabled
- **No event batching**: Each event sent individually (could be optimized)