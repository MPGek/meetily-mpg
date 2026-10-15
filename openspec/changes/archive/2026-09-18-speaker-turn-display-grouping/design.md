## Context

Transcript records are cut by the recording pipeline (VAD pauses, live 500ms merge threshold, 25s Whisper chunk cap) before speakers exist, so one speaker's utterance becomes several records. Speaker values are resolved later (`matchSpeakerToTranscript` live, persisted assignments for historical meetings). The renders (`TranscriptView.tsx`, `VirtualizedTranscriptView.tsx` for both home and meeting-details via `TranscriptPanel.tsx`) show one record per segment. `VirtualizedTranscriptView` keys active playback on `activeSegmentId === segment.id` (VirtualizedTranscriptView.tsx:938,1007). An existing collapsible "group by speaker" mode exists but is a section view, not consecutive-turn merging.

## Goals / Non-Goals

**Goals:**
- A pure, unit-testable grouping function: `(transcripts, per-transcript pinned/cluster resolution) -> TranscriptTurn[]`, placed in `frontend/src/lib/` (pattern of `live-speaker-labels.ts`).
- Single grouping path shared by the live view and the meeting-details view.
- Render-only: stored records, API, editing and assignment flows unchanged.

**Non-Goals:**
- No pipeline changes (VAD thresholds, merge gaps, 25s cap stay as specced in `live-segment-merging`).
- No persisted "turn" entity in SQLite; turns are derived at render time.
- No changes to word-level diarization sub-row logic (`upsertLiveBlocks`/`resolveLiveBlocks`) beyond hosting them inside merged turns and laying the container out on the member's source side (Decision 5).
- No changes to the collapsible grouping mode itself.

## Decisions

1. **Derive turns at render time; never persist them.**
   Alternatives: persisting a `turn_id` column (backend + migration + sync burden for a display concern), or mutating the transcript list (breaks seek targets and dedup). Rendering from derived turns keeps data granular for the player, editing, and word-alignment, matching how `assemble_channel_turns` already derives (not stores) diarization turns. Chosen: derive.
2. **Grouping key = resolved speaker identity + `source_device`, with 60s gap break.**
   The turn boundary condition `(speaker, source_device)` + `next.start - prev.end < 60s` is identical code for both views, satisfying the "same layout in live and details" precedent from `split-transcript-ui`.
   - 60s chosen as conservative: under 500ms-lipeline pauses always merge; a 60s silence almost certainly means a topic/activity break, and keeps a pathological "same speaker, tiny fragments far apart" list reasonably chunked.
   - Alternative considered: use the diarization turns' own boundaries as authoritative break points. Rejected for now: turns already propagate via `matchSpeakerToTranscript`, so speaker-change boundaries already appear as the `speaker` field changing; a second event stream to subscribe to in the renderer adds coupling for little gain.
3. **Live view groups from context state after matching, not from raw events.**
   The live pipeline emits `transcript-update`; grouping runs as a `useMemo` over the same inputs the matcher already consumes (resolved transcripts via `rematchTranscripts` outputs). This automatically recomputes the merge when late speaker events re-flip a segment's speaker (spec: "re-forms the merge" scenario), with no extra listeners.
   - Alternative: incremental turn updating keyed on `parent_sequence_id`. Rejected: ordering/pinning interplay makes incremental state a correctness liability; the memo version updates the whole list the way `rematchTranscripts` already does.
4. **Turn block UI: extend the existing chat-style row.**
   A merged turn renders as one row carrying the first member's header (speaker dot/label, play button, turn timestamp) plus the concatenated member texts, all laid out on the turn's own source side (`source_device` decides the side, the timestamp side, and the label order — the cues `split-transcript-ui` defines), with the active-highlight applied when `isAudioPlaying` is active for *any* member. The current integration renders the merged row without the per-record bubble; the side cue comes from alignment, timestamp side, and label order, which are identical for a merged row and for the members inside it. Keying: turn keeps a derived id `turn:<first-segment-id>`; active check becomes "playing time falls within turn [start, end]" instead of `activeSegmentId === segment.id` — this is the one behavioral change inside the renderer.
   - Editing an inline text: opens on the member whose window covers the clicked position; keeping per-member editing preserves `split-transcript-ui` inline-edit behavior.
   - Splitting a turn remains possible through the existing split UI on member records.
5. **Word-level sub-rows stay per parent record and per parent side.**
   `upsertLiveBlocks` is keyed by `parent_sequence_id`; inside a merged turn each member keeps its own word-row container, and that container inherits the member's source-side layout (a System member's sub-rows align right with their labels and dots on the System side, a Microphone member's sub-rows stay on the Microphone side). No key changes, and no change to sub-row content or data flow: only the container's side and label placement follow `source_device`. Expanded-state member rows render through the side-aware standard row, so the rule only has to be enforced in the merged container.
   - Why this must be explicit: without it the sub-row container is laid out independently of the turn, so a System record split by live word-level diarization into several short runs renders in the Microphone-side layout while still carrying System cluster labels (`SPEAKER_NN` → "Speaker 1"), which reads as a different speaker written on the microphone side. In a stereo session microphone clusters are `MIC_SPEAKER_NN` ("Mic Speaker N"), so a bare "Speaker N" left-aligned row is always a System record on the wrong side — the observed defect this decision prevents.
6. **Grouping helper is pure and channel/order agnostic.**
   Input is the chronologically-sorted transcript list (the view already sorts); the helper must not sort (sort stays upstream where dedup/sync owns it) so reuse cannot introduce ordering regressions.

## Risks / Trade-offs

- [Late speaker events flip an early segment's speaker in live mode] → merge is recomputed per update; worst case a block visually splits mid-utterance, which is truthful. No data risk.
- [Wide 90%-width bubbles grow very tall for long monologues] → merged turn text capped in practice by the 25s chunk rule plus 60s gap; if still too tall, max-height with internal scroll is the fallback UI lever.
- [Active-highlight behavioral change in `VirtualizedTranscriptView`] → highlight logic changed from id-equality to window containment only for merged turns' active spans; regression risk is renderer-local and covered by existing playback-indicator behavior.
- [Interaction with the existing collapsible grouping mode] → collapsible sections absorb merged turns unchanged (grouping is hierarchical: section by speaker, rows by turn); simpler than disabling merging in that mode.
- [Performance in long meetings] → grouping is O(n) scan on a memo'd sorted list equal to what `rematchTranscripts` already walks; virtualization receives turns, not raw records, reducing row count.
- [Split sub-rows rendered on the wrong side] → the sub-row container is laid out from the turn's/member's side (Decision 5), covered by a spec scenario and by task-level verification; renderer-local and reversible, no data change.
- [The same defect outside turns] → a split record that belongs to no merged turn is rendered by the standalone `blocks.length > 1` branch in `VirtualizedTranscriptView`, which also ignores `source_device`; that path belongs to `live-word-level-diarization`/`split-transcript-ui` and is out of scope here (follow-up update to that change).

## Open Questions

- Exact 60s gap break value is tunable without behavior-contract impact (the 60s threshold is in the spec scenario; adjusting it within the same test structure is a spec edit, decided at implementation QA if real recordings suggest otherwise).
- Whether merged rows should regain the per-record bubble used by single-record rows (`split-transcript-ui`) is a visual choice that changes no spec scenario and no task; the current integration omits it and relies on the side cues above.

## Migration Plan

Pure frontend additive change: new helper + render integration behind no flag (default on for both views). Rollback = revert the render integration; helper is inert without callers. No data migration.
