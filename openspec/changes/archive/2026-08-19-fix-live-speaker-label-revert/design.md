## Context

See `proposal.md` — Why. Current state in `contexts/TranscriptContext.tsx`:

- The live turn stream is a cumulative ref, `turnsRef`, that is only ever appended to — never rewritten when a user renames a cluster. So it keeps advertising the pre-rename `(auto)` name for a cluster the user already bound to a person.
- On every `online-speaker-turn` event, the handler re-matches **all** transcripts against `turnsRef` via `matchSpeakerToTranscript` (greatest temporal overlap, same-channel). A pinned block is only skipped when `pin.cluster === turn.speaker` — i.e. only for same-cluster incoming turns. Any other cluster's turn falls through and re-matches, picking the stale `(auto)` turn for the pinned block's time window and resetting `speaker_label` to the old auto name.
- Pins are stored in `pinnedLabelsRef: Map<transcriptId, {cluster, name}>`, mutated both outside and inside the `setTranscripts` updater (a mild React anti-pattern but functional for a ref).

The backend already performs the authoritative bind (`assign_live_speaker` → `PrototypeStore::bind`), and stop-time persistence (`recording-stopped` assignments + per-block `speaker_override_id`) is correct and out of scope. The defect is entirely frontend local-state inconsistency.

## Goals / Non-Goals

**Goals:**
- A user-assigned live label never reverts on its own, including on old blocks and when unrelated turns arrive.
- The live turn stream becomes self-consistent with the user's bindings, so re-matching is idempotent (re-matching a bound cluster always yields the user's name, never a stale auto name).
- Preserve single-block vs apply-to-all scoping: single-block edits only that transcript; apply-to-all rewrites the whole cluster's turns.

**Non-Goals:**
- No change to the backend, the stop-time persist, or the DB schema.
- No change to the `(auto)`/confidence rendering itself.
- No change to offline/meeting-details re-analysis.

## Decisions

### Decision 1: Freeze pinned transcripts unconditionally in re-match
In the `onSpeakerTurn` handler, replace the conditional `pin.cluster === turn.speaker` guard with an unconditional check: if `pinnedLabelsRef.current.has(t.id)`, return the transcript unchanged. A pinned transcript is never re-matched by any turn; only an explicit edit (which re-pins or updates it) changes it.

- **Why:** the old guard failed whenever an unrelated cluster's turn arrived, because re-matching runs against the cumulative stale `turnsRef`. The freeze must be independent of which turn triggers the re-match.
- **Alternative considered:** keeping the cluster-match guard and relying on the turn-stream rewrite (Decision 2) alone. Rejected: a race or a missing rewrite entry would still let a stale turn leak; the explicit freeze is the cheaper, provably-correct invariant.

### Decision 2: Rewrite the live turn stream on bind so it reflects user bindings
Add a helper that, given a cluster label and the bound speaker name, rewrites `turnsRef` in place: every turn whose `speaker === clusterLabel` becomes `{...turn, display_name: name, matched_by: 'user', match_score: undefined}`. Call it from `applyLiveSpeakerLabel` (after the backend bind succeeds) for **cluster-wide** binds. For **single-block** binds, rewrite only the turn(s) whose time window overlaps the edited transcript's window.

- **Why:** this makes `turnsRef` the single source of truth consistent with the backend's `PrototypeStore::bind()`. Re-matching a bound cluster then always yields the user's name (idempotent), so even unpinned blocks of the cluster stop showing stale `(auto)` names, and the freeze in Decision 1 is belt-and-suspenders rather than load-bearing.
- **Alternative considered:** dropping the freeze and relying on rewrite alone. Rejected for the race noted in Decision 1.
- **Alternative considered:** storing user overrides in a separate map (`cluster → name`) and consulting it inside `matchSpeakerToTranscript` instead of mutating `turnsRef`. Valid and arguably cleaner, but mutating `turnsRef` is the smaller diff and leaves the rest of the re-match logic untouched. (Noted as a possible future refactor in Open Questions.)

### Decision 3: Refactor the live-override state into one explicit map
Replace `pinnedLabelsRef` with a single explicit `userAssignmentsRef: Map<transcriptId, {cluster, name}>` (renamed, same shape) plus, if Decision 2's rewrite covers the whole cluster, a `clusterBindingsRef: Map<clusterLabel, name>` for apply-to-all. Move all mutation out of the `setTranscripts` updater into a plain function invoked before/after the state update, so the ref and the derived state stay in sync and the logic is unit-testable.

- **Why:** the previous code mutated a ref inside a state updater (fragile under batching and re-renders) and conflated "pinned block" with "cluster binding". Splitting them makes single-block vs apply-to-all explicit and testable.
- **Alternative considered:** moving the override state into React state and deriving transcripts. Heavier re-render churn on every turn; the ref approach is the established pattern here.

### Decision 4: Scope rewrite by the same flag the editor uses
The editor already distinguishes scope: `applyLiveSpeakerLabel(speaker, name, transcriptId?)` with `transcriptId` → single-block, `undefined` → apply-to-all. Reuse exactly that. Single-block rewrites only the turns overlapping the transcript's `[audio_start_time, audio_end_time]`; apply-to-all rewrites every turn of the cluster.

- **Why:** keeps the product model stable and avoids a behavior surprise from the earlier design decision where apply-to-all may have been softenened. The spec for "single-turn override" explicitly says other turns of the cluster stay labeled as before.

## Risks / Trade-offs

- [Stale `turnsRef` entries for a cluster that later gets re-bound to a different name] → The rewrite is applied on every bind, so a second rename replaces the first for all matching turns. Freeze only affects transcripts pinned to the current binding, and a new edit re-pins. Acceptable.
- [Freezing a block prevents it from ever getting a newer (better) auto label] → Intentional: a user assignment is authoritative for that block. Consistency with the DB persist (which honors the user binding) reinforces this.
- [Rewriting `turnsRef` every bind is O(turns)] → Tunable memory/time; live turns are bounded per recording and binds are infrequent (a few per meeting), so this is negligible.
- [React ref mutation ordering between `turnsRef` rewrite and `setTranscripts`] → Sequence the operations: rewrite `turnsRef`/assignment maps first, then `setTranscripts` reads the refs; the handler already reads `turnsRef.current` inside the updater, so ordering is preserved.

## Migration Plan

- No data migration, no backend change, no config. Pure frontend local-state fix.
- Rollback: revert the handler guard to the same-cluster condition and leave `turnsRef` append-only; behavior returns to the previous (buggy) state.

## Open Questions

- Should single-block edits also be represented in the cluster stream so future `matchSpeakerToTranscript` calls for the *same* time window never see the stale turn? Currently the freeze covers the edited transcript; whether other transcripts sharing that time window exist is unlikely given non-overlapping segments — safe to defer, the freeze handles the per-block guarantee.
- Long term, would it be cleaner to make `matchSpeakerToTranscript` consult a `cluster → name` override map instead of mutating `turnsRef`? Deferrable without changing behavior; noted as a future refactor.