# Tasks

## 1. Match new rows on arrival

- [x] 1.1 Add a test in `frontend/tests/lib/live-speaker-labels.test.ts`: a turn list that already overlaps a newly added row labels that row through `rematchTranscripts`, and a row in the frozen set keeps its label; verify with `bun test tests/lib/live-speaker-labels.test.ts`
- [x] 1.2 In `frontend/src/contexts/TranscriptContext.tsx`, pass the combined, sorted list in the transcript-append `setTranscripts` through `rematchTranscripts(…, turnsRef.current, frozenTranscriptsRef.current)`, reusing the existing helper; verify `bun test tests/` and `npx tsc --noEmit` pass

## 2. Live check

- [ ] 2.1 Record a Fast-mode session, say one phrase and stay silent for ~10 s; verify the last row gets its speaker label within a second of appearing instead of waiting for the next speech
