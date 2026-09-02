## Why

The live transcript view wastes a third of the panel width: the content column is capped at `w-2/3` of the panel and chat bubbles are further limited to `max-w-[80%]`, so in a narrow window (e.g. ~960px) actual text renders in a ~450px column with huge dead margins on both sides, making the transcript hard to use while recording.

On top of the width problem, live auto-follow is unreliable and invisible: a new transcript schedules a delayed scroll-to-bottom that still fires if the user scrolls up within that window (yanking the view back down), and once auto-follow is paused there is no indicator and no one-click way to return to the live bottom — the user must manually scroll all the way down.

## What Changes

- Live transcript view (home page): transcript content column expands to use the full panel width, keeping the 750px readability cap for wide windows (`w-2/3 max-w-[750px]` → `w-full max-w-[750px]`).
- Transcript bubbles (microphone and system variants, both live and meeting-details views): the chat-style width cap is relaxed from 80% to 90% of the row width, preserving the left/right source cue while reclaiming horizontal space.
- Explicitly unchanged: `px-4` container padding, timestamp/play-button columns, centering wrappers of `RecordingControls` and `StatusOverlays` (they only center a small pill, not text width), legacy (no-bubble) segment style.
- Live view auto-follow: a pending scroll-to-bottom is cancelled if the user has scrolled up before it fires (fixes the 150ms yank-back race in `TranscriptContext`).
- Live view: a circular overlay button with a down-arrow appears at the bottom-right of the transcript panel whenever the view is not pinned to the bottom; clicking it scrolls to the bottom and re-enables auto-follow. Manually scrolling back to the bottom also re-enables auto-follow and hides the button. Auto-follow behavior in the meeting-details view is unchanged (no auto-scroll there, no button).

## Capabilities

### New Capabilities

- (none)

### Modified Capabilities

- `split-transcript-ui`: two new requirements — (1) transcript content width adapts to the available panel width (full width up to the 750px readability cap) instead of a fixed 2/3 column, and chat bubbles span at least 90% of the row width; (2) live auto-follow control: paused auto-follow survives new segments, a bottom-right overlay button returns to the live bottom and re-enables auto-follow, and manual scroll-to-bottom re-enables it and hides the button.

## Impact

- `frontend/src/app/_components/TranscriptPanel.tsx` — content wrapper class (line ~112); new scroll-to-bottom overlay button anchored to the panel.
- `frontend/src/components/VirtualizedTranscriptView.tsx` — `max-w-[80%]` on mic and system segment wrappers (lines ~571, ~608).
- `frontend/src/contexts/TranscriptContext.tsx` — live auto-follow logic (lines ~70-106): expose at-bottom state for the button, cancel pending scroll when user scrolled up.
- Layout + frontend state changes only; no backend, DB, or IPC impact. Virtualizer measures rows dynamically, so width changes are handled automatically.
