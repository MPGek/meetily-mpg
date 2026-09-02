## Context

Margin stack in the live transcript view (see proposal.md - Why), from outermost to innermost:

1. `frontend/src/app/_components/TranscriptPanel.tsx:112` — `w-2/3 max-w-[750px]` content wrapper.
2. `VirtualizedTranscriptView.tsx:782` — `px-4` scroll container padding (32px total).
3. `VirtualizedTranscriptView.tsx:571` (mic) and `:608` (system) — `max-w-[80%]` bubble cap.
4. Per-row fixed costs: timestamp `min-w-[50px]`, gap, play button.

The meeting-details view (`MeetingDetails/TranscriptPanel.tsx`) has no `w-2/3` wrapper; it only suffers layers 2-4. The legacy (no-bubble) segment variant already uses `flex-1` without a cap.

Live auto-follow today: the scrolling element in the live view is the `TranscriptPanel` root (`transcriptContainerRef`, `overflow-y-auto`), not the `VirtualizedTranscriptView` scroll container — `useAutoScroll` inside VTV is effectively inert there. `TranscriptContext.tsx:70-106` implements the real behavior: a scroll listener keeps `isUserAtBottomRef` (ref, 10px tolerance) and a `transcripts`-change effect schedules a 150ms-delayed smooth `scrollTo` bottom when the ref is true. The ref check happens before the timer, so scrolling up within the 150ms window still yanks the view down, and no UI exposes the paused state.

## Goals / Non-Goals

**Goals:**
- Transcript text reclaims width in narrow windows in both live and details views.
- Live auto-follow is reliable (no yank-back) and user-controllable via a visible scroll-to-bottom button.
- Preserve: chat-style side alignment as the source cue, 750px readable line cap on wide screens, virtualization behavior.

**Non-Goals:**
- No changes to `RecordingControls`/`StatusOverlays` centering (`w-2/3` there only centers a small pill).
- No redesign of the timestamp/play-button columns or `px-4` padding.
- No new settings or user-controllable width options.
- No auto-follow changes in the meeting-details view (`disableAutoScroll` stays; no button there).
- No changes to `useAutoScroll`/`VirtualizedTranscriptView` scroll logic (inert in the live view; the live fix lives in `TranscriptContext`).

## Decisions

**D1: `w-full max-w-[750px]` instead of `w-2/3 max-w-[750px]`.**
Keeps the 750px readability cap for wide windows (long lines hurt reading; the cap was already the binding constraint above ~1125px panel width). Alternative considered: `w-full` with no cap — rejected, line length would grow unbounded on wide panels.

**D2: Bubble cap `max-w-[80%]` → `max-w-[90%]`, static.**
The 80% cap is a deliberate chat cue (empty gutter on the opposite side marks mic vs system), so removing it entirely would flatten the design; 90% keeps the cue while reclaiming space at every panel size, including the narrow details side panel. Alternatives considered: container-query-based responsive cap (`@container` + `@lg`) — rejected, requires adding the `@tailwindcss/container-queries` plugin for a cosmetic gain; viewport breakpoints (`sm:`/`md:`) — rejected, the panel width is decoupled from viewport width in the details view (panel is 1/4-1/3 of viewport), so viewport-based rules would misfire there.

**D3: No changes to virtualization.**
`@tanstack/react-virtual` measures rows dynamically (`measureElement`), so width-driven height changes are handled without extra work.

**D4: Auto-follow state lives in `TranscriptContext`, mirrored as React state.**
The live view's scroll container is the panel root tracked by `TranscriptContext` (see Context), so the pause/resume logic and the button state belong there — not in `useAutoScroll` (wrong element in the live view). Keep `isUserAtBottomRef` as the source of truth for the scroll effect and add a `isFollowingBottom` state mirror (set alongside the ref in the scroll handler) so the button can render. Alternative considered: move auto-scroll into `VirtualizedTranscriptView` — rejected, it would mean re-plumbing the scroll element and duplicating logic for a marginal cleanup.

**D5: Button click = scroll to bottom + re-enable.**
The button's onClick sets the ref/state to at-bottom and calls the same smooth `scrollTo` the auto-follow uses; re-enabling follows from the ref being true on the next effect run. Reaching the bottom by manual scroll already flips the ref via the existing listener; the state mirror hides the button. No new state machine needed.

**D6: Race fix — re-check `isUserAtBottomRef` inside the 150ms timeout.**
Move the at-bottom check into the timeout callback (and keep the pre-schedule check), so a scroll-up during the delay cancels the yank. Alternative considered: remove the delay — rejected, the 150ms wait exists so `scrollHeight` includes the full rendered new segment (Framer Motion animation).

**D7: Button anchored in a `relative` wrapper around the panel, suppressed during programmatic scroll.**
The panel root is the scroll container, so an absolutely positioned child would scroll with the content; wrap `TranscriptPanel`'s root in a `relative` container and place the button `absolute bottom-12 right-4` outside the scroll flow (bottom offset per D9). Alternative considered: `fixed` positioning — rejected, it would need the sidebar-margin hack that `RecordingControls` already carries. During our own smooth scroll (auto-follow or button click) intermediate scroll events report "not at bottom" and would flicker the button; set a programmatic-scroll flag around `scrollTo` and ignore it in the state mirror (the ref logic stays as-is). Amended by D8: the time-based flag swallowed genuine user scrolls.

**D8: Input-release + `scrollend` for the programmatic-scroll flag (revised after manual verification).**
Manual check showed the D7 flag is armed on every auto-follow segment (150ms schedule + smooth animation + up to 500ms fallback), so a user scroll-up inside that window is ignored for the button state: the button appears up to ~1s late, or never — when the animation is not cancelled (scrollbar drag, trackpad inertia) it completes to the bottom, re-enables following and hides the button, re-yanking the view. Fix: (1) do not arm the flag when the container is already at the bottom (no movement, no events, 500ms of dead suppression); (2) release the flag on user input (`wheel`/`touchstart`/scroll `keydown`) and immediately re-sync `isFollowingBottom`/`isUserAtBottomRef` from the current position — user input is interrupt intent, so also abandon the in-flight smooth scroll (assign current `scrollTop` to cancel it); (3) clear the flag precisely on `scrollend` (Chromium ≥114 / WebView2); keep the 500ms timeout only as a last-resort fallback. Alternatives considered: shorter timeout — still guesswork with the same swallow window; debouncing the button show — adds latency without fixing the yank-back.

**D9: Button bottom offset aligned with the recording controls.**
The overlay button at `bottom-4` floats below the `RecordingControls` pill row (`fixed bottom-12` in `page.tsx`); use the same `bottom-12` offset so the button and the pill sit on one baseline.

## Risks / Trade-offs

- [90% bubbles look less "chat-like" on wide screens where 750px cap dominates] → the side gutter is still visible (10% ≈ 75px); acceptable trade-off, reversible one-class change.
- [Very long lines on wide single-column panels could reduce readability] → bounded by the 750px cap (D1).
- [Details view panel is narrow by design (~250-340px)] → bubbles gain ~25px each; the main win there is unchanged by this work, no regression risk.
- [10px at-bottom tolerance means one wheel tick shows the button] → desired: that tick is exactly the "I left the live bottom" signal; button is small and non-blocking.
- [Programmatic-scroll flag swallows user scroll / leaks if no final event] → per D8: released on user input (`wheel`/`touchstart`/keydown) with immediate state re-sync, cleared precisely on `scrollend`, 500ms timeout only as last-resort fallback.
- [State mirror re-renders the panel on every scroll settle] → scroll handler is already debounced by the 150ms effect; state changes only at the at-bottom boundary, not per pixel.
