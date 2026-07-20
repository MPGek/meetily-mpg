## Context

The `split-mic-system-tracks` change already implemented backend infrastructure: independent VAD per source, stereo recording, and `source_device` labeling on `TranscriptUpdate` events. The `source_device` field ("Microphone" or "System") is emitted by the backend and persisted in `transcripts.json`. However, the frontend drops this field at the TypeScript type boundary, and the SQLite database does not store it. All transcript segments render as an undifferentiated single-column timeline.

The data flow gap:

```
Backend (worker.rs)           → has source_device ✓
  ↓ Tauri event
Frontend TranscriptUpdate     → MISSING source_device ✗
  ↓
Transcript (state)            → MISSING source_device ✗
  ↓
TranscriptSegmentData (view)  → MISSING source_device ✗
  ↓
VirtualizedTranscriptView     → flat single-column render ✗

SQLite transcripts table      → MISSING source_device column ✗
api.rs MeetingTranscript      → MISSING source_device ✗
```

## Goals / Non-Goals

**Goals:**
- Plumb `source_device` through the entire stack: DB → API → frontend types → UI rendering
- Render mic segments on the left side with one background color, system segments on the right side with a different background color (chat-style layout)
- Timestamps appear on opposite sides: left-side for mic, right-side for system
- Apply to both live transcription view (home page) and meeting details view (old meetings)
- Persist `source_device` in SQLite so it survives across app restarts
- Gracefully handle old meetings that have no `source_device` data (render as neutral/default side)

**Non-Goals:**
- Speaker diarization within a single source (e.g., distinguishing multiple people on system audio)
- Changing the backend audio pipeline or VAD logic
- Modifying the `transcripts.json` format (it already has `source_device`)
- Echo cancellation or audio quality improvements
- Separate transcript export per source

## Decisions

### 1. DB migration: add `source_device` column to `transcripts` table

Add a nullable `TEXT` column `source_device` to the existing `transcripts` table. Nullable because old meetings won't have this data.

**Alternative considered**: Storing in a separate table. Rejected — adds join complexity for a single field.

### 2. Chat-style layout in `VirtualizedTranscriptView`

The `TranscriptSegment` component renders differently based on `source_device`:

```
┌─────────────────────────────────────────────────────┐
│ [00:12]  Hello, how are you?                        │  ← mic (left-aligned)
│          bg: blue-50                                │
│                                                     │
│        I'm doing well thanks  [00:13]               │  ← system (right-aligned)
│          bg: green-50                               │
│                                                     │
│ [00:15]  Great, let's start                        │  ← mic
│          bg: blue-50                                │
│                                                     │
│        Sure, go ahead           [00:16]             │  ← system
│          bg: green-50                               │
└─────────────────────────────────────────────────────┘
```

- Mic segments: left-aligned, timestamp on left, blue-tinted background
- System segments: right-aligned, timestamp on right, green-tinted background
- Unknown/missing source: center-aligned or neutral gray (backward compatibility)

**Alternative considered**: Two-column side-by-side layout. Rejected — breaks chronological reading order and complicates virtualization. Chat-style preserves single-column chronological flow while providing visual distinction.

### 3. `source_device` flows through `TranscriptSegmentData`

Add `source_device?: string` to `TranscriptSegmentData` so the virtualized view receives it without needing the full `Transcript` type. Both `TranscriptPanel` (live) and `usePaginatedTranscripts` (historical) populate it.

### 4. Color scheme

- Microphone: `bg-blue-50` bubble with `border-blue-100` (light blue)
- System: `bg-emerald-50` bubble with `border-emerald-100` (light green)
- Unknown/legacy: no background bubble, plain text (current behavior)

These are subtle, accessible, and distinct. Both are pastel enough to not clash with dark text.

### 5. Backward compatibility for old meetings

Old meetings in SQLite will have `source_device = NULL`. The UI renders these with the legacy neutral style (no bubble, left-aligned). No migration of old data needed — the `transcripts.json` files already have `source_device` if re-import is ever needed.

## Risks / Trade-offs

- **[Risk] Virtualization height variance** → Mic and system segments have different layouts (timestamp on different sides). The virtualizer's `estimateSize` (60px) should still work since both layouts are roughly the same height. If not, `measureElement` handles dynamic sizing.
- **[Risk] Wide text on right-aligned segments** → Long system transcripts right-aligned might look odd. Mitigate with `max-w-[80%]` on the bubble so it doesn't stretch full width.
- **[Trade-off] No re-import of old meetings** → Old meetings won't show the chat-style layout unless the user re-transcribes. This is acceptable — the data is in `transcripts.json` but not worth a migration tool for now.
