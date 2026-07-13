---
parent: CODEBASE_MAP_MODULES.md
last_mapped: 2026-07-13T14:35:00Z
module: frontend_components
---

> Part of [Module Guide](CODEBASE_MAP_MODULES.md) | [Codebase Map](CODEBASE_MAP.md)

# Module: Frontend Components (Shadcn/ui + Custom)

## Overview

**Purpose**: The frontend components module provides reusable UI components built with Shadcn/ui, Radix primitives, and Tailwind CSS. Used across all pages for consistent design system, accessibility, and responsive layout.

**Entry point**: `frontend/src/components/ui/` — component library root
**Framework**: React + TypeScript + Tailwind CSS + Shadcn/ui

## File Reference

### Core Components (`components/ui/`)

| File | Purpose | Key Exports | Tokens |
|------|---------|-------------|--------|
| `button.tsx` | Button variant styles | Button component, variants | ~1k |
| `card.tsx` | Card container | Card, CardHeader, CardContent | ~2k |
| `dialog.tsx` | Modal dialog | Dialog, DialogContent, DialogTitle | ~3k |
| `dropdown-menu.tsx` | Dropdown menus | DropdownMenu, MenuItem | ~2k |
| `input.tsx` | Text input fields | Input component | ~1k |
| `label.tsx` | Form labels | Label component | ~0.5k |
| `select.tsx` | Select dropdowns | Select, SelectItem | ~3k |
| `slider.tsx` | Range slider | Slider component | ~2k |
| `switch.tsx` | Toggle switch | Switch component | ~1k |
| `tabs.tsx` | Tab navigation | Tabs, TabsContent, TabsList | ~2k |
| `toast.tsx` | Toast notifications | toast(), useToast() | ~3k |
| `tooltip.tsx` | Hover tooltips | Tooltip, TooltipContent | ~2k |
| `badge.tsx` | Status badges | Badge component | ~0.5k |
| `separator.tsx` | Visual separators | Separator component | ~0.5k |

### Layout Components (`components/layout/`)

| File | Purpose | Key Exports | Tokens |
|------|---------|-------------|--------|
| `app-sidebar.tsx` | App sidebar navigation | AppSidebar, nav items | ~3k |
| `header.tsx` | Page header component | PageHeader | ~2k |
| `main-content.tsx` | Main content wrapper | MainContent | ~1k |

### Feature Components (`components/features/`)

| File | Purpose | Key Exports | Tokens |
|------|---------|-------------|--------|
| `recording-controls.tsx` | Recording start/stop UI | RecordingControls | ~4k |
| `meeting-card.tsx` | Meeting list item | MeetingCard | ~3k |
| `transcript-display.tsx` | Transcript text display | TranscriptDisplay | ~5k |
| `summary-viewer.tsx` | AI summary display | SummaryViewer | ~4k |
| `device-selector.tsx` | Audio device selection | DeviceSelector | ~6k |
| `provider-selector.tsx` | LLM provider selection | ProviderSelector | ~3k |

### Icons (`components/icons/`)

| File | Purpose | Key Exports | Tokens |
|------|---------|-------------|--------|
| `mic-icon.tsx` | Microphone icon | MicIcon | ~0.5k |
| `record-icon.tsx` | Record button icon | RecordIcon | ~0.5k |
| `stop-icon.tsx` | Stop button icon | StopIcon | ~0.5k |
| `summary-icon.tsx` | Summary icon | SummaryIcon | ~0.5k |

## Public API (Component Props)

### RecordingControls

```tsx
interface RecordingControlsProps {
  isRecording: boolean;
  duration: number;
  onToggleRecording: () => void;
  meetingName?: string;
}
```

### DeviceSelector

```tsx
interface DeviceSelectorProps {
  devices: AudioDevice[];
  selectedDeviceId: string | null;
  onSelectDevice: (deviceId: string) => void;
  onRefreshDevices: () => void;
}
```

### TranscriptDisplay

```tsx
interface TranscriptDisplayProps {
  transcript: string;
  isLive?: boolean;
  onUpdate?: (newText: string) => void;
  wordCount?: number;
}
```

## Internal Architecture

### Component Composition Pattern

```tsx
// Shadcn/ui primitive composition
<Card>
  <CardHeader>
    <CardTitle>Title</CardTitle>
  </CardHeader>
  <CardContent>
    {/* Custom feature component */}
    <RecordingControls {...props} />
  </CardContent>
</Card>
```

### Theme Integration

- CSS variables in `globals.css` for theme colors
- Dark mode via class-based toggle (`dark` class on `<html>`)
- Tailwind `dark:` variant for dark-specific styling

### Responsive Design

- Mobile-first with Tailwind breakpoints (sm, md, lg, xl)
- Sidebar collapses to drawer on mobile
- Grid layouts adjust columns based on viewport

## Dependencies (imports FROM)

| Module/Package | What is imported | Why |
|---------------|-----------------|-----|
| `@radix-ui/react-*` | Dialog, Select, Switch, etc. | Accessible UI primitives |
| `lucide-react` | Icon components | SVG icons |
| `clsx` + `tailwind-merge` | cn() helper | Conditional class names |
| `sonner` | toast() | Toast notifications |

## Dependents (imported BY)

| Consumer Module | What it uses | Context |
|----------------|-------------|---------|
| All pages | UI components | Page layout and interaction |
| Shared layouts | Sidebar, header | App navigation structure |

## Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `theme` | system | Theme mode (light/dark/system) |
| `fontFamily` | Inter | Font family for text |
| `sidebar_collapsed` | false | Sidebar default state |

## Error Handling

- **Missing props**: TypeScript strict types prevent runtime errors
- **Invalid device ID**: Validation before passing to Tauri backend
- **Component mount errors**: React error boundaries catch rendering failures

## Gotchas and Tech Debt

- **Shadcn/ui customization**: Components are copied into project (not packages) — must manually update when primitives change
- **Tailwind class conflicts**: `cn()` helper handles merging but complex cases need manual review
- **Icon consistency**: Some custom icons may not match Lucide style perfectly