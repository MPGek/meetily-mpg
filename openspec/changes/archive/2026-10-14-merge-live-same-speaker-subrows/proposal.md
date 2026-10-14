# Proposal

## Why

Live word-level diarization splits a finalized transcript block into one sub-row per raw cluster label. The online clusterer often gives one person several cluster labels inside a block, and each of them then resolves to the same name. The live view shows these as separate sub-rows with the same label and different coloured dots. In the user's screenshot, one 25 s block is shown as 8 one-line sub-rows, each labelled "Mikhail Shashalevich (auto)". Granola shows the same speech as one bubble. The split adds no information and makes the live transcript hard to read.

The dispatch-latency change (`live-asr-dispatch-latency`, archived 2026-10-09) listed this as out of scope.

## What Changes

- **Merge adjacent sub-rows with the same visible speaker.** Within one block, neighbouring sub-rows merge into one when they show the same speaker name with the same ownership. Ownership means user-assigned versus automatic. Without a name, the cluster label is compared instead. A merged sub-row has:
  - the joined text;
  - the start of its first part and the end of its last part;
  - the list of its parts (cluster label and time window each).
- **Merge both before and after user assignments are applied.** Two cases:
  - Before: an assignment made on a merged sub-row covers the whole merged span.
  - After: a rename that gives neighbouring sub-rows the same name merges them too.
- **One run left means one row.** When merging leaves a single run, the block renders as an ordinary row with that run's label, instead of the parent's segment-level label.
- **Edits on a merged sub-row apply to all of its parts.**
  - A single-row rename, a confirm, or a rename applied to all blocks is sent to the backend once per constituent cluster, with each part's time window where the scope is single-row.
  - The stop-time persistence can then apply it to every stored row the merged sub-row covered. That persistence matches overrides by cluster label and time window.
- Non-adjacent sub-rows of the same cluster stay separate. Editing one of them still leaves the others alone, as today.

Out of scope:
- Merging across parent blocks: consecutive transcript rows of one speaker stay separate rows.
- Backend splitting, the stop-time split and the persisted rows.
- Clustering quality, i.e. why one person gets several labels in the first place.
- Interim (partial) text and the GigaAM live engine. Those are a separate change.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `live-word-diarization`: a split block's sub-rows are one per run of the same visible speaker, not one per raw cluster label. A block whose runs all show one speaker renders as a single row with that speaker's label.
- `live-speaker-labels`: an assignment or confirmation on a merged sub-row applies to all of its parts. The "siblings stay alone" rule now applies to non-adjacent sub-rows, because adjacent same-name sub-rows are one row.

## Impact

- `frontend/src/lib/live-speaker-labels.ts`: merge helper, used around `resolveLiveBlocks`.
- `frontend/src/types/index.ts`: `LiveTranscriptBlock` gains an optional `parts` list.
- `frontend/src/contexts/TranscriptContext.tsx`: `getLiveBlocksFor` returns the merged view, including a single run.
- `frontend/src/app/_components/TranscriptPanel.tsx`: uses a single-run result as the row's label.
- `frontend/src/components/VirtualizedTranscriptView.tsx`: the label editor sends one backend assignment per part.
- `frontend/tests/lib/live-speaker-labels.test.ts`: tests.
- No backend, database, IPC payload, or persistence changes. Offline (saved meeting) transcripts are unaffected.
