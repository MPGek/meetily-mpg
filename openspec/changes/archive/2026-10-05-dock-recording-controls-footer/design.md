# Design

## Context

The motivation is in proposal.md, under Why. The current structure is:

- `app/layout.tsx` already offsets the page beside the sidebar: `MainContent` applies `ml-16`/`ml-64` and `pl-8`.
- `app/page.tsx` renders `flex flex-col h-screen`, then a `flex flex-1 overflow-hidden` row that holds `TranscriptPanel`.
- The controls stack (`PendingTagsPicker` above a `bg-white rounded-full shadow-lg` wrapper around `RecordingControls`) is a sibling inside that row, but it is `fixed bottom-12`. Because `fixed` escapes the layout, the stack re-applies the sidebar offset itself through an inline `marginLeft`.
- `StatusOverlays` uses the same fixed pattern (`bottom-4`) and is only used by `page.tsx`.
- `TranscriptPanel` reserves `pb-20` for the overlay. Its scroll-to-bottom button is `absolute bottom-12 right-4` inside the panel's `relative` root, so it is aligned with the overlay rather than with the panel edge.
- `RecordingControls` renders `flex flex-col` with three parts:
  - the control pill;
  - an optional "Validating speech recognition..." line;
  - an optional `deviceError` `Alert`.

  "Pausing…"/"Stopping…" labels are `absolute -top-8` over the buttons.

Constraint from `online-diarization-telemetry`: the status block stays two channel lines plus one model-indicator row, to the right of the recording indicator. This makes the block about 40 px tall, which sets the minimum row height (the buttons are `h-10` as well).

## Goals / Non-Goals

**Goals:**
- The footer is a normal flex child, so the browser's layout guarantees there is no overlap. Nothing has to be measured.
- The footer row height is constant across idle, recording, processing, and saving. It only grows while a device error is shown.

**Non-Goals:**
- Reworking the diarization status block or making it denser.
- Changing tag semantics, the popover's contents, or the meeting-details tag bar (`MeetingTagsBar`).
- Restyling the controls beyond removing the floating shadow and containing the transient labels.

## Decisions

### 1. Footer as a flow sibling of the transcript row

The page structure becomes:

```
<div flex flex-col h-screen>
  <div flex flex-1 min-h-0 overflow-hidden>   <- TranscriptPanel
  <footer shrink-0 border-t bg-white>          <- only when footer is visible
    <div mx-auto w-full max-w-[750px] flex items-start justify-center gap-3 min-h-14 px-4 py-1>
      [controls | status]   [tag picker: min-w-0]
```

The transcript scroll container then ends at the footer's top edge. The `fixed` wrapper and its `marginLeft` go away, because `MainContent` already provides the sidebar offset. Keeping the inner row at `max-w-[750px]` aligns it with the transcript column, so it reads as part of the transcript. The picker is `min-w-0` but not `flex-1`, so it shrinks under pressure without stretching, letting `justify-center` centre the group; the stop status is centred the same way.

- *Alternative: keep the overlay and measure its height* (a ResizeObserver feeding a dynamic `padding-bottom`). This keeps the floating look but needs a ref, an observer, and wiring between page and panel to get the same guarantee the flow layout gives for free. Rejected.
- *Alternative: keep the overlay with a larger fixed padding.* Rejected: it breaks again whenever the footer grows.

Visibility: the footer is shown when `hasMicrophone || isRecording || PROCESSING_TRANSCRIPTS || SAVING`. This is today's condition for the controls plus the two stop states, which previously hid the controls and showed `StatusOverlays` instead.

### 2. Status replaces controls inside the footer; `StatusOverlays` is deleted

During PROCESSING_TRANSCRIPTS or SAVING, the footer row renders a spinner and the message ("Finalizing transcription..." / "Saving transcript...") in place of both `RecordingControls` and the tag picker. Hiding the picker matches today's behaviour: the pending set is consumed at save, so editing it mid-save is not offered. The row's `min-h-14` keeps the height stable. `StatusOverlays.tsx` has no other users, so it is removed rather than left as dead code.

### 3. Flat footer; the controls keep their internal layout

The `bg-white rounded-full shadow-lg` wrapper in `page.tsx` is removed. The pill element inside `RecordingControls` drops `shadow-lg`, the rounded white background and its own `px-4 py-2` padding (the footer row pads instead), keeps its flex layout, and gets `min-h-12` so the controls line is 48 px in every state (the idle start button is `h-12`). The footer's `border-t bg-white` provides the surface. The diarization block is not touched.

### 4. Compact picker: icon, placeholder, at most three pills, `+N`

`PendingTagsPicker` loses the `label` prop and the `flex-wrap` container. Its layout:

```
[#-trigger] (empty)  "Tag this recording"   (muted text, also opens the popover)
[#-trigger] [Work x] [Q3 x] [Client x] [+2]
```

- The row is `flex-nowrap min-w-0 overflow-hidden`. Pills keep `max-w-[80px] truncate`, and the outer element drops its own pill background and shadow.
- `+N` is a non-removable chip that opens the same popover.
- The split into visible and hidden tags is a small pure helper in `lib/meeting-tags.ts`, for example `splitVisibleTags(tags, max = 3) -> { visible, hiddenCount }`. It is unit-tested in `tests/lib/meeting-tags.test.ts`.
- *Alternative: measure the pills and fit as many as the width allows.* This needs a ResizeObserver and per-pill measurement. It was rejected for simplicity: the cap plus truncation keeps the picker one line high at any width, which is the requirement.

### 5. Transient pause/stop feedback inside the buttons

The `absolute -top-8` "Pausing…"/"Resuming…"/"Stopping…" labels are removed. While one of these actions is in flight, the affected button shows a small spinner instead of its icon. The text goes into the button's tooltip and `aria-label`, so the button width stays fixed and the row does not shift.

- *Alternative: inline text next to the buttons.* Rejected: the row width would change mid-action and push the tag picker around.

### 6. Device-error alert stays in `RecordingControls`, under the control line

`deviceError` state and its listeners stay where they are. The footer row uses `items-start`. The tag picker is wrapped in an `h-12 flex items-center` box, the height of the controls line, so that it stays centred on the controls line, whether or not the alert is shown. When the alert appears it sits under the control pill, within the controls column. The footer grows downward, and the transcript area shrinks accordingly, as decision 1 provides. The alert keeps its close button.

- *Alternative: a sonner toast.* Rejected: the app's `Toaster` is `bottom-center`, which would float over the transcript and break the no-cover requirement.
- *Alternative: lift `deviceError` into `page.tsx` and render a full-width strip.* This is more plumbing for a rare state. It can be done later if the column-width alert looks cramped.

### 7. Transcript panel adjustments

- `pb-20` becomes `pb-4`.
- The scroll-to-bottom button goes from `bottom-12` to `bottom-4`. Because the panel's box now ends at the footer, this puts the button just above the footer, as the modified `split-transcript-ui` scenario requires.
- The `flex-1` transcript row also needs `min-h-0`, so that it actually shrinks when the footer grows instead of overflowing `h-screen`.

## Risks / Trade-offs

- **[Visual change]** A permanent white strip replaces the floating pills, so the screen looks heavier. → The strip is flat with a thin border and only about 56 px tall; this was accepted in the explore session.
- **[Narrow windows]** The controls alone (about 370 px with the diarization block) plus the tag trigger may not fit very narrow content areas. → The picker shrinks to just its icon trigger (`min-w-0`). The controls are `shrink-0`. Below that width the row clips the picker, never the controls.
- **[Alert in the controls column]** A long multi-line device error message wraps within about 370 px. → It is acceptable for a rare state. Decision 6 names the follow-up if needed.
- **[Height jump on stop]** If the status row rendered shorter than the controls row, the transcript would shift on stop. → `min-h-14` on the row is the same for every footer state.

## Migration Plan

Frontend-only layout change with no data or IPC impact. Roll back by reverting the commit.
