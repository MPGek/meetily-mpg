## 1. Layout changes

- [x] 1.1 In `frontend/src/app/_components/TranscriptPanel.tsx`, change the transcript content wrapper from `w-2/3 max-w-[750px]` to `w-full max-w-[750px]` and verify the live view content column spans the full panel width in a narrow window (~960px) while staying capped at 750px when wide

- [x] 1.2 In `frontend/src/components/VirtualizedTranscriptView.tsx`, change `max-w-[80%]` to `max-w-[90%]` on the microphone segment wrapper (~line 571) and the system segment wrapper (~line 608), and verify mic bubbles remain left-aligned and system bubbles right-aligned with a small opposite-side gutter

## 2. Live auto-follow control

- [x] 2.1 In `frontend/src/contexts/TranscriptContext.tsx`, add an `isFollowingBottom` state mirror of `isUserAtBottomRef` (set in the existing scroll handler) and expose it from the context, and verify the panel re-renders when the user scrolls away from / back to the bottom

- [x] 2.2 In the same file, move the at-bottom re-check into the 150ms timeout callback of the auto-scroll effect (D6) and verify scrolling up within the delay window after a new segment no longer yanks the view to the bottom

- [x] 2.3 In `frontend/src/app/_components/TranscriptPanel.tsx`, wrap the panel root in a `relative` container and render a circular scroll-to-bottom button (`absolute bottom-4 right-4`, down arrow, e.g. lucide `ArrowDown`) shown when `!isFollowingBottom`; onClick scrolls to bottom and re-enables auto-follow (D4/D5), and verify the button appears on scroll-up, disappears on click and on manual scroll-to-bottom

- [x] 2.4 Suppress the button during programmatic smooth scroll via a flag around `scrollTo` (D7) and verify no flicker while auto-follow or the button click is animating to the bottom

- [x] 2.5 In `frontend/src/contexts/TranscriptContext.tsx`, make the programmatic-scroll flag input-driven (D8): do not arm it when the container is already at the bottom; release it and re-sync `isFollowingBottom`/`isUserAtBottomRef` from the current position on user input (`wheel`/`touchstart`/scroll `keydown`), cancelling any in-flight smooth scroll; clear it on `scrollend` with the 500ms timeout kept only as a last-resort fallback; and verify scrolling up during or right after an auto-follow animation shows the button immediately and the view stays put

- [x] 2.6 In `frontend/src/app/_components/TranscriptPanel.tsx`, change the scroll-to-bottom button offset from `bottom-4` to `bottom-12` to align it with the `RecordingControls` pill baseline (D9), and verify both sit on the same bottom line

## 3. Verification

- [x] 3.1 Run frontend lint and typecheck (`npm run lint`, `npx tsc --noEmit` in `frontend/`) and verify they pass with no new errors

- [ ] 3.2 Manual check in the running app: record a short meeting in a narrow window (~960px and ~700px wide) and confirm transcript text uses nearly the full panel width with no dead 1/6 margins; open a saved meeting's details view and confirm the same bubble rules apply in the side panel; confirm auto-scroll during recording still behaves correctly (row heights re-measure after the width change)

- [ ] 3.3 Manual auto-follow check while recording: scroll up as new segments arrive and confirm the view stays put and the button shows; click the button and confirm jump to bottom + following resumes; manually scroll to the bottom and confirm the button hides and following resumes; confirm the meeting-details view shows no button and is unchanged
