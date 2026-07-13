---
parent: CODEBASE_MAP_MODULES.md
last_mapped: 2026-07-13T14:35:00Z
module: frontend_app
---

> Part of [Module Guide](CODEBASE_MAP_MODULES.md) | [Codebase Map](CODEBASE_MAP.md)

# Module: Frontend App (Next.js + Tauri)

## Overview

**Purpose**: The frontend app module provides the main Next.js application shell that runs inside the Tauri desktop wrapper. It handles routing, state management via Zustand, and serves as the entry point for all UI pages and global configuration.

**Entry point**: `frontend/src/app/layout.tsx` — root layout
**Framework**: Next.js 14+ (App Router) + Tauri Desktop Wrapper

## File Reference

| File | Purpose | Key Exports | Tokens |
|------|---------|-------------|--------|
| `app/layout.tsx` | Root layout component | Global providers, theme, fonts | ~3k |
| `app/page.tsx` | Home page (meeting list) | MeetingListPage | ~2k |
| `app/globals.css` | Global styles + Tailwind directives | CSS variables, base styles | ~4k |
| `lib/config.ts` | App configuration | APP_CONFIG constant | ~1k |
| `lib/constants.ts` | Shared constants | App-wide constants | ~0.5k |

## Public API (Components)

### Root Layout Structure

```tsx
// app/layout.tsx
<Html>
  <Body>
    <ThemeProvider>
      <FontProvider>
        <ZustandStore>
          <TauriContext>
            {children}
          </TauriContext>
        </ZustandStore>
      </FontProvider>
    </ThemeProvider>
  </Body>
</Html>
```

### Page Routes

| Route | Component | Description |
|-------|-----------|-------------|
| `/` | MeetingListPage | Home page with meeting list |
| `/meeting/[id]` | MeetingDetailPage | Single meeting detail view |
| `/recording` | RecordingPage | Active recording interface |
| `/settings` | SettingsPage | App settings and configuration |

## Internal Architecture

### State Management

- **Zustand stores**: Global state (audio devices, transcription status, UI preferences)
- **React Context**: Theme, font family, Tauri app handle
- **Server Components**: Data fetching for meeting list (Next.js Server Components)

### Configuration

```typescript
// lib/config.ts
const APP_CONFIG = {
  appName: 'Meetily',
  version: '1.0.0',
  maxRecordingDuration: 4 * 60 * 60, // 4 hours
  supportedFormats: ['wav', 'mp3', 'mka'],
  defaultTranscriptionProvider: 'parakeet' as const,
};
```

### Concurrency Model

- **Server Components**: Data fetching at build/request time (Next.js App Router)
- **Client Components**: Interactive UI with Tauri command calls via `@tauri-apps/api`
- **SWR**: Data revalidation and caching for meeting list

## Dependencies (imports FROM)

| Module/Package | What is imported | Why |
|---------------|-----------------|-----|
| `next/font/google` | Geist, Inter fonts | Typography |
| `@tauri-apps/api` | invoke, event | Tauri IPC communication |
| `zustand` | create, useStore | Global state management |

## Dependents (imported BY)

| Consumer Module | What it uses | Context |
|----------------|-------------|---------|
| All pages | Layout, config, constants | App shell and routing |

## Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `appName` | Meetily | Application display name |
| `maxRecordingDuration` | 4 hours | Maximum recording time limit |
| `supportedFormats` | wav, mp3, mka | Accepted audio formats |
| `defaultProvider` | parakeet | Default transcription provider |

## Error Handling

- **Tauri invoke failure**: Show toast notification with error message
- **Network unavailable**: Graceful degradation for cloud features
- **Font loading fallback**: System font if Google Fonts fails

## Gotchas and Tech Debt

- **Hydration mismatch**: Some components may hydrate differently between server/client
- **Tauri API calls**: Must use `invoke()` — not direct Rust function calls
- **Next.js App Router**: Server vs Client Components must be carefully managed