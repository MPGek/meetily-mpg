# Tasks

## 1. Compact pending-tag picker

- [x] 1.1 Add `splitVisibleTags(tags, max = 3) -> { visible, hiddenCount }` to `frontend/src/lib/meeting-tags.ts`, with cases in `frontend/tests/lib/meeting-tags.test.ts` for 0, 3 and 5 tags (`{Work, Q3, Client, Urgent, Demo}` gives 3 visible and `hiddenCount` 2). Verify: `bun test tests/lib/meeting-tags.test.ts` passes.
- [x] 1.2 Rework `PendingTagsPicker` (design decision 4) and verify `bun run lint` and `npx tsc --noEmit` are clean:
  - remove the `label` prop and the `flex-wrap` container;
  - render the tag-icon trigger, then either the muted "Tag this recording" placeholder (when the set is empty, also opens the popover) or up to three removable truncated pills plus a `+N` chip that opens the popover;
  - root is `flex-nowrap min-w-0 overflow-hidden`, with no pill background or shadow.

## 2. Recording controls inside a flat row

- [x] 2.1 In `RecordingControls`, drop the control pill's `shadow-lg`/rounded white surface while keeping its flex layout. Verify: the diarization status block still renders to the right of the bars; `bun test tests/lib/diarization-status-lines.test.ts` passes.
- [x] 2.2 Replace the `absolute -top-8` "Pausing…"/"Resuming…"/"Stopping…" labels with an in-button spinner while the action is in flight, and put the text into the tooltip and `aria-label` (design decision 5). Verify: the button width is unchanged, and no element in `RecordingControls` uses negative `top` positioning.

## 3. Docked footer on the home screen

- [x] 3.1 In `frontend/src/app/page.tsx`, implement design decisions 1 and 6, then verify `bun run lint` is clean:
  - replace the `fixed bottom-12` wrapper (and its `marginLeft` style plus the white `rounded-full shadow-lg` wrapper) with a `<footer className="shrink-0 border-t bg-white">` sibling after the transcript row;
  - give the transcript row `min-h-0`;
  - add an inner `mx-auto w-full max-w-[750px] flex items-start gap-3 min-h-14 px-4 py-1` row;
  - put `RecordingControls` in a `shrink-0` column, and the picker in a `min-w-0 h-12 flex items-center` box.
- [x] 3.2 Extend the footer visibility to `hasMicrophone || isRecording || PROCESSING_TRANSCRIPTS || SAVING`. In the two stop states, render a spinner plus "Finalizing transcription..." / "Saving transcript..." in the row instead of the controls and picker. Then delete `frontend/src/app/_components/StatusOverlays.tsx` and its import. Verify: `grep -r StatusOverlays frontend/src` returns nothing and `npx tsc --noEmit` is clean.
- [x] 3.3 Centre the footer row: add `justify-center` to the inner row and drop `flex-1` from the picker box (keep `min-w-0`). Verify in the running app that the group is centred under the transcript column, and that narrowing the window still shrinks only the picker.

## 4. Transcript panel

- [x] 4.1 In `TranscriptPanel`, change the content `pb-20` to `pb-4` and the scroll-to-bottom button `bottom-12` to `bottom-4`. Verify in the running app (`bun run tauri:dev`) that, scrolled up, the button sits at the panel's bottom-right just above the footer and does not overlap it.

## 5. Integration check in the running app

- [x] 5.1 With `bun run tauri:dev`, record a short session with diarization on and verify each `recording-controls-layout` scenario by eye:
  - the newest block is fully visible above the footer while pinned;
  - controls, diarization block and picker share one row;
  - five pending tags show three pills plus `+2`;
  - narrowing the window or expanding the sidebar shrinks only the picker;
  - pause/stop show in-button spinners with nothing over the transcript;
  - on stop, the footer shows "Finalizing transcription..." then "Saving transcript..." with no height jump, and returns to the start control and empty "Tag this recording" placeholder after save.
- [x] 5.2 Trigger a device error (unplug the selected mic before start, or pick a missing device) and verify:
  - the alert appears under the controls inside the footer;
  - the transcript shrinks rather than being covered;
  - the picker stays on the controls line;
  - the alert's close button dismisses it.
