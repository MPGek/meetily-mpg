# live-word-diarization Delta Spec

## Purpose

During Fast-mode recording, attribute transcribed words to speakers live and split a finalized transcript block spanning multiple speakers into one display row per contiguous speaker block, using the same token-to-turn assignment rules as stop-time finalize. All live splitting is display-only; stop-time finalize remains authoritative for persisted data.

## ADDED Requirements

### Requirement: Live token-level speaker attribution during recording

In Fast mode with online diarization active, when a transcript block finalizes and later acquires refined word tokens (live CTC alignment enabled and available), the system SHALL attribute each token to the speaker of the covering live speaker turn (max overlap, nearest-turn fallback ≤30s, same rules as stop-time assignment) using only live turns from the transcript block's own channel, and SHALL notify the frontend of the per-block speaker grouping without writing any transcript speaker assignment to the database.

#### Scenario: Fully covered block attributed live
- **WHEN** a finalized block's token span is fully covered by stable live turns of its channel and all tokens attribute to one speaker
- **THEN** the frontend SHALL display that block with that speaker's live label (cluster label, display name, matched_by, match_score as emitted by the turn stream)

#### Scenario: Attribution uses refined tokens only
- **WHEN** live CTC alignment is enabled and available, the block's tokens SHALL have been refined against the block's own audio before token-to-turn attribution runs
- **WHEN** live CTC alignment is disabled or unavailable
- **THEN** attribution SHALL run on the transcription engine's own word timestamps and attribution SHALL NOT fail or error

### Requirement: Live N-way split of multi-speaker blocks

When token attribution yields more than one validated contiguous speaker block (boundaries validated by ≥2 contiguous tokens of the new speaker), the system SHALL emit the block's live display as split sub-rows: one sub-row per speaker block, each carrying the block's own token text-slice, contiguous gap-free sub-row timestamps stitched at the boundary token edge, and the sub-block's speaker identity (cluster label plus display name / matched_by / match_score provenance from the turn stream).

#### Scenario: Block spanning a speaker change splits live
- **WHEN** a finalized mic-channel block's tokens attribute to speaker A for the first tokens and speaker B for the last ≥2 tokens
- **THEN** the live view SHALL render the block as two sub-rows: the first carrying A's tokens and A's speaker label, the second carrying B's tokens and B's speaker label

#### Scenario: Single-speaker block never splits
- **WHEN** a finalized block's tokens all attribute to one speaker (or token attribution is inconclusive)
- **THEN** the block SHALL render as one row exactly as today

#### Scenario: Tokens follow sub-row boundaries on stop
- **WHEN** a live-split block reaches stop-time finalize
- **THEN** persistence SHALL be computed by the existing stop-time N-way split logic, which SHALL produce equivalent per-speaker rows; the live split itself SHALL NOT have added, removed, or split any persisted transcript row

### Requirement: Watermark-based reconciliation of late speaker turns

Because stable turns can arrive after a block finalizes, a block whose token span is not yet covered by the live turn stream SHALL be held in a bounded provisional set and re-attributed (and re-emitted as a new display revision of the same block) when new stable turns arrive. A held block SHALL become decidable once a stable turn of its channel exists with start time at or after the block's end (no future turn can overlap it). Blocks whose covering turns never arrive SHALL remain unsplit.

#### Scenario: Block finalizes before covering turn is stable
- **WHEN** a block finalizes and its tail tokens have no covering stable turn yet, and later a stable turn arrives covering the tail and attributing it to a different speaker
- **THEN** the held block SHALL be re-attributed and re-emitted as a new revision that splits the block, and the live view SHALL replace the earlier single-row rendering

#### Scenario: Provisional set is bounded
- **WHEN** the number of held provisional blocks exceeds the bounded capacity
- **WHEN** new blocks are admitted beyond that capacity
- **THEN** the oldest held blocks SHALL be released, keeping their last emitted rendering, and recording SHALL continue

#### Scenario: Turn stream never produces coverage
- **WHEN** a block finishes with no stable turns overlapping its tokens for the rest of the session (no overlapping turn, no waterfall past it)
- **THEN** the block SHALL remain unsplit and SHALL fall back to today's segment-level labeling; no error is surfaced

### Requirement: Live splits are display-only

Live token attribution, watermark holding, and N-way splitting SHALL update only in-memory live view state. They SHALL NOT write to the shared in-memory transcript buffer used for incremental persistence, shall not modify incremental transcript files during recording, and shall not write to the database; stop-time finalize SHALL remain the authoritative splitter for saved transcripts.

#### Scenario: Live split does not touch persistence
- **WHEN** recording stops after live splits occurred
- **THEN** the persisted transcript rows SHALL come from the existing stop-time split path only, and incremental transcripts written during the session SHALL show the original block exactly once with unrefined-or-refined tokens as before

### Requirement: Silent degradation ladder

The system SHALL degrade silently at each step, without errors or user-visible warnings: no live turns (Efficient/Off/processing failure) → segment-level labeling as today; alignment disabled or missing → attribution on ASR-provided tokens; provisional overflow → oldest blocks released as segment-level. No live-split failure SHALL interrupt transcription, recording, or the alignment queue.

#### Scenario: Efficient mode unaffected
- **WHEN** recording runs in Efficient mode with diarization enabled
- **THEN** no live token attribution, reconciliation, or sub-row splitting SHALL occur, and the existing segment-level label flow and stop-time finalize behavior SHALL be unchanged

#### Scenario: Alignment queue overflow during reconciliation
- **WHEN** the live reconciliation queue overflows and drops the oldest held block
- **THEN** that block SHALL keep its most recent rendering and recording SHALL continue unaffected

### Requirement: Live split revision identity

Each emitted live split of a block SHALL carry the block's identifying parent reference and a monotonically increasing revision, so repeated re-attribution (from newly arrived turns) update the same block's rendering rather than duplicating rows, and long-standing user assignments remain addressable across revisions.

#### Scenario: Turn stream revision does not duplicate rows
- **WHEN** a held block is re-attributed more than once as successive stable turns arrive
- **THEN** each emission carries the same parent reference with an increased revision, and the frontend SHALL render exactly one current revision of each block, not multiple stacked versions
