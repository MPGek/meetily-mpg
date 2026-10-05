# Proposal

## Why

On the home screen the tag picker ("Tags for this recording") and the recording controls float in a `position: fixed` stack over the live transcript. The stack spans roughly 48–144 px above the window bottom while the transcript reserves only 80 px of bottom padding, so the newest transcript block is always partly hidden behind the controls during recording, the moment the user most needs to read it. Its height also varies (tags wrap, device error alert, "Pausing…"/"Stopping…" labels), so no fixed padding can stay correct.

## What Changes

- Replace the fixed overlay with a footer docked in the normal layout flow below the transcript. The transcript scroll area ends above the footer, so no transcript content can sit underneath the controls.
- Put the controls and the tag picker on one row (about 56 px high instead of the current ~96 px stack plus the 48 px gap): a centred group: recording controls at their natural width, then the tag picker, which shrinks first when space runs out.
- Compact the tag picker so it never wraps:
  - drop the "Tags for this recording" / "Tag this recording" text label;
  - when the pending set is empty, show only the tag icon and a "Tag this recording" placeholder;
  - show at most three pending pills on a single line with long names truncated, and collapse the rest into a `+N` chip (the popover still lists every tag).
- Make the footer a flat panel (top border, white background); the controls and tag picker no longer have their own floating pill shadows.
- Keep the footer in place during PROCESSING_TRANSCRIPTS and SAVING. It shows the status ("Finalizing transcription…", "Saving transcript…") where the controls were, replacing the separate fixed `StatusOverlays` pill.
- Keep the "Pausing…"/"Stopping…" feedback and the device-error alert from overflowing onto the transcript or breaking the single row.
- Move the scroll-to-bottom button to the bottom-right corner of the transcript panel, just above the footer. It is no longer aligned with the controls overlay, because that overlay no longer exists.
- Reduce the transcript's bottom padding from the overlay reserve (`pb-20`) to normal spacing, and remove the duplicated sidebar-width margin the fixed overlay needed.
- Leave unchanged:
  - the diarization status block (two channel lines plus the model-indicator row);
  - pending-tag semantics;
  - recording start/pause/stop behaviour.

## Capabilities

### New Capabilities
- `recording-controls-layout`: placement and shape of the home-screen recording footer. It is docked below the transcript and never overlaps it, lays out the controls and the pending-tag picker on one row with a compact non-wrapping picker, shows the stop-processing status in place, and keeps transient feedback inside the footer.

### Modified Capabilities
- `split-transcript-ui`: the scroll-to-bottom button scenario of "Live auto-follow control". It no longer says "vertically aligned with the recording controls". The button is anchored to the bottom-right of the transcript panel, above the docked footer.

## Impact

- `frontend/src/app/page.tsx`: layout restructure. Remove the fixed overlay wrapper and the `marginLeft` sidebar offset, add the footer row, and render status in the footer instead of `StatusOverlays`.
- `frontend/src/app/_components/StatusOverlays.tsx`: deleted; `page.tsx` was its only user, and the status moves into the footer.
- `frontend/src/app/_components/TranscriptPanel.tsx`: bottom padding, scroll-to-bottom button position.
- `frontend/src/components/RecordingControls.tsx`:
  - drop the outer pill shadow;
  - contain the "Pausing…"/"Stopping…" labels;
  - make sure the device-error alert does not break the row.
- `frontend/src/components/MeetingTags/PendingTagsPicker.tsx`: compact, non-wrapping, `+N` overflow; `label` prop replaced by an empty-state placeholder.
- No backend, IPC, or data changes. No new dependencies.
