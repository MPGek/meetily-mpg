# Proposal

## Why

Diarization is implemented twice. `frontend/src-tauri/src/audio/diarization.rs` (4077 lines) runs the offline batch pipeline and `frontend/src-tauri/src/audio/online_diarization.rs` (1951 lines) runs the live one; they share only the embedder family and the token-split helper. The two paths carry a structurally identical `find_best_speaker` (diarization.rs:2547 vs online_diarization.rs:1535), two cluster→embedding groupers (online_diarization.rs:1476/1496 vs diarization.rs:2345), two embedder factories (online_diarization.rs:679 vs `create_speaker_embedder` used at diarization.rs:1243), and two clusterer constructions with different parameter sources. The consequence a user sees: the `diarization-param-tuning` settings never reach live recording — `OnlineDiarizationProcessor::finalize` hardcodes `TITANET_CLUSTER_THRESHOLD` (online_diarization.rs:1283) while the batch path reads `DiarizationConfig::resolved()` (diarization.rs:706), and live Efficient clustering runs with `max_speakers` passed through unclamped (recording_commands.rs:672-676 resolves an unset value to `0`), so the always-enforced speaker-count ceiling does not apply to it.

This change unifies the two into one `audio/diarization/` module tree with one core, one parameter surface, and one engine facade, so a later accuracy change (05b) and the `recording_commands.rs` split (07) have a single thing to call.

## What Changes

- Relocate `audio/diarization.rs`, `audio/online_diarization.rs`, `audio/live_diarization_reconcile.rs`, and `audio/speaker_recognition.rs` into an `audio/diarization/` module tree (`config`, `core`, `batch`, `streaming`, `identity`, `persist`, `telemetry`, `commands`, `engine`), with `mod.rs` re-exports so the 30 `audio::diarization::*` / `audio::online_diarization::*` references across 12 files keep compiling.
- Delete the duplicated implementations: the second `find_best_speaker`, `cluster_embeddings_by_labels` / `cluster_embeddings_by_overlap`, `create_enhanced_embedder`, and the inline offline token-split block in `start_diarization` (diarization.rs:330-470) which is folded into the shared timeline + persistence modules.
- Introduce a `Clustering` trait so batch global AHC and the live Efficient buffer construct their clusterer through one factory that resolves kind, merge threshold, and ceiling from the same settings.
- **Behavior change:** the live diarization path resolves its clustering parameters from the persisted `diarization-param-tuning` settings instead of the built-in family constant, and the speaker-count ceiling is enforced (and clamped) for live clustering exactly as it already is for batch.
- **Behavior change:** live and stop-time speaker attribution run one algorithm, so the two paths cannot drift apart as either is edited.
- Introduce a `DiarizationEngine` facade (start/finalize/persist a live session, assign a live speaker, telemetry snapshot, start a batch run, clustering settings) as the single entry point for `recording_commands.rs`; change 07 moves the orchestration behind it.
- No new IPC command, no event payload change, no schema change, no change to offline accuracy or to the offline result for the same input.

## Capabilities

### New Capabilities
<!-- None: this change unifies existing diarization behavior; it introduces no new capability. -->

### Modified Capabilities
- `diarization-param-tuning`: the runtime clustering parameters (kind, merge threshold, ceiling, gap-merge window) resolve for the live diarization path as well as the offline one, and the speaker-count ceiling is always enforced for every clustering pass regardless of path.
- `online-speaker-diarization`: live stop-time clustering uses the resolved runtime parameters rather than a fixed family constant (the tuned family value stays the default), and live and stop-time speaker attribution are required to produce the same result for the same input.

## Impact

- Code: `frontend/src-tauri/src/audio/diarization.rs` → `audio/diarization/` tree; `audio/online_diarization.rs`, `audio/live_diarization_reconcile.rs`, `audio/speaker_recognition.rs` absorbed into it; `audio/mod.rs:67-79,154-168` re-export block; `audio/recording_commands.rs` (statics at 53, 59, 78, 99 move behind the facade); `database/speaker_commands.rs:562-566` (reaches into `ONLINE_DIARIZATION_STORE` directly today); `src/bin/diarize_eval.rs:9,116` (imports `diarize_wav_samples`, `DiarizationConfig`, `ClustererKindSetting`); `lib.rs:846-847` command registration.
- Prerequisites: applies after `04-recording-lock-hardening` and after the in-flight `live-word-level-diarization` change has finished (its tasks 4.1/6.x still edit `online_diarization.rs` and `live_diarization_reconcile.rs`; this change must not rebase in-flight work).
- Dependents: `07-split-recording-commands` calls the `DiarizationEngine` facade defined here; `05b-live-diarization-accuracy` builds its refinement pass on the `Clustering` trait and the unified parameter surface.
- Dependencies: none new. `polyvoice` clusterer/streaming APIs are used exactly as today.
- Out of scope: any accuracy change (deferred re-clustering at stop, the cache-reusing offline pass, `AudioSource` unification, new eval metrics and gates) — all of that is `05b-live-diarization-accuracy`. Removing the Efficient engine, changing the live event contract, and touching offline defaults are also out of scope.
