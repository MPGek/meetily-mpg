# Design

## Context

See `proposal.md` for motivation. Verified current state (2026-09-18, branch `feat/diarization`):

- **Offline path** — `audio/diarization.rs` (4077 lines): `start_diarization` (204) → ffmpeg PCM (`spawn_ffmpeg_pcm` 2147, `PcmStream` 2082, `StreamWindows` 2221) → `run_channel_diarization_stream` (2284) → `V2Core::process_chunk` (1521: segmentation-3.0 with hysteresis binarization, overlap masking, dense TitaNet units) → `V2Core::finish` (1657) → global clustering via `build_clusterer` (1191) → `assemble_channel_turns` (1723, with `subtract_span` 1943 and `gap_fill_turns` 1978) → the token N-way split written straight to SQL inside `start_diarization` (call at 357, block ≈330-470) → `persist_and_recognize_session` (2443) over `group_cluster_embeddings` (2345) / `persist_channel_clusters` (2394) and `find_best_speaker` (2547).
- **Live path** — `audio/online_diarization.rs` (1951 lines): `process_chunk` (989) resamples the VAD chunk and routes it to `Engine::Fast` (polyvoice `StreamingPipeline` built by `create_fast_channel` 1514 with `EnergyVad` and `LatencyPreset::Balanced`) or `Engine::Efficient` (`EmbeddingBuffer` 699, `cluster` 712). Fast embeds each chunk itself (1112), recognizes live through `PrototypeStore::recognize` (611), publishes stable turns to `live_diarization_reconcile::registry()` (1231) and to the `online-speaker-turn` sender. `finalize` (1267) re-does the token split (1394-1440) and matches with its own `find_best_speaker` (1535).
- **Confirmed duplication**: `find_best_speaker` at diarization.rs:2547 and online_diarization.rs:1535 are the same algorithm, differing only in the segment type and in `i32` vs `usize`; `cluster_embeddings_by_labels` (1476) / `cluster_embeddings_by_overlap` (1496) versus `group_cluster_embeddings` (2345); `create_enhanced_embedder` (679, `DiarizationEmbedder::new(path, 1)`) versus the pooled `create_speaker_embedder(models_dir, pool)` used at diarization.rs:1243; three `assign_tokens_to_speakers` call sites with three different output shapes (diarization.rs:357, online_diarization.rs:1412, live_diarization_reconcile.rs:300).
- **Confirmed parameter gap**: `DiarizationConfig::resolved()` (706) reads the persisted overrides through the atomics set by `set_clustering_overrides` (782) / the `set_diarization_clustering_settings` command (806). `finalize` instead takes `crate::audio::embedder::TITANET_CLUSTER_THRESHOLD` (online_diarization.rs:1283) and passes it with `self.max_speakers` into `EmbeddingBuffer::cluster` (712), which builds `MinClusterSizeClusterer(AhcClusterer::with_threshold(max_speakers, threshold), 2)` directly — no clusterer kind, no gap-merge window, and no 255 clamp. `max_speakers` is `max_speakers.filter(|m| *m > 0).unwrap_or(0)` (recording_commands.rs:672-673), so an unset user maximum reaches the clusterer as `0`.
- **Fast mode never clusters**: it trusts the `StreamingPipeline`'s own speaker ids end-to-end and never runs segmentation-3.0; `channel.turns` at stop are those same pipeline turns mapped through `TimelineMapper::to_abs` (782).
- **Two independent guards**: `DiarizationGuard` (diarization.rs:33, over `DIARIZATION_IN_PROGRESS`) and `OnlineDiarizationGuard` (online_diarization.rs:39, over `ONLINE_DIARIZATION_ACTIVE`) protect different resources — offline runs and live sessions can legitimately be concurrent today.
- **Call-site surface**: 30 references to `audio::diarization::*` / `audio::online_diarization::*` / `audio::live_diarization_reconcile::*` / `audio::speaker_recognition::*` across 12 files, including `lib.rs:846-847` (command registration), `bin/diarize_eval.rs:9,116`, `database/repositories/speaker.rs`, `database/speaker_commands.rs:562-566` (reaches into `recording_commands::ONLINE_DIARIZATION_STORE` directly), `audio/pipeline.rs`, `audio/retranscription.rs`, `audio/transcription/worker.rs:104,598`, `audio/word_alignment/queue.rs`.
- **In flight**: `live-word-level-diarization` (9/15 done) still edits `online_diarization.rs` and `live_diarization_reconcile.rs` in its tasks 4.1 and 6.x.

## Goals / Non-Goals

**Goals:**

- One module tree (`audio/diarization/`) holding every diarization concern, with a stable public surface re-exported from `mod.rs` so no caller outside the tree changes its import path.
- One implementation of each shared behavior: speaker attribution, cluster→embedding grouping, embedder construction, clusterer construction.
- One resolved parameter surface for both paths, with the ceiling always enforced.
- One facade (`DiarizationEngine`) that `recording_commands.rs` and change 07 call, so no other module reaches into diarization statics.
- Every step of the migration compiles, passes the existing tests unchanged, and can ship on its own.

**Non-Goals:**

- Any accuracy change. Deferred re-clustering at stop, the cache-reusing offline pass, the `AudioSource` unification, and the new eval metrics belong to `05b-live-diarization-accuracy`.
- Removing the Efficient engine or changing which engines exist.
- Changing the `online-speaker-turn` / `live-transcript-blocks` event payloads, the IPC command set, the database schema, or the offline defaults.
- Merging the two concurrency guards into one.
- Rebasing or re-planning the in-flight `live-word-level-diarization` work.

## Decisions

### D1: Module map (file:line → new module)

`audio/diarization.rs`, `audio/online_diarization.rs`, `audio/live_diarization_reconcile.rs`, and `audio/speaker_recognition.rs` become `audio/diarization/`:

| Source | New module |
| --- | --- |
| diarization.rs:30-52 (`DIARIZATION_IN_PROGRESS`, `DIARIZATION_CANCELLED`, `DiarizationGuard`) | `batch/guard.rs` |
| diarization.rs:54-75 (`DiarizationProgress`, `DiarizationResult`, `is_diarization_in_progress`, `cancel_diarization`) | `core/units.rs` (types) + `commands.rs` (queries) |
| diarization.rs:77-201 (`get_diarization_status` 78, `update_speaker_label_command` 97, `rematch_meeting_speakers` 116) | `commands.rs` |
| diarization.rs:204-517 (`start_diarization`) | `commands.rs` wrapper + `batch/orchestrator.rs` |
| diarization.rs:330-470 (inline token split + `INSERT OR IGNORE` rows) | `persist/offline_split.rs`, attributing tokens through `core/timeline.rs` |
| diarization.rs:518-567 (`refine_offline_rows`) | `persist/offline_split.rs` |
| diarization.rs:569-620 (`fixed_pool_size`) | `config.rs` |
| diarization.rs:623-826 (`ClustererKindSetting`, `DiarizationConfig`, atomics, `set_clustering_overrides` 782, `set_diarization_clustering_settings` 806) | `config.rs` (command re-exported from `commands.rs`) |
| diarization.rs:827-1148 (`StageTimings`, `run_diarization_blocking*`, `ChannelSplit`, `run_channel_diarization`, `diarize_decoded_channels`) | `batch/orchestrator.rs` |
| diarization.rs:1150-1190 (`DiarizationSegment`, `ClusteredEmbedding`, `ChannelClusters`, `PolyvoiceDiarizer`) | `core/units.rs` |
| diarization.rs:1191-1229 (`build_clusterer`) | `core/cluster.rs` |
| diarization.rs:1230-1394 (`create_polyvoice_diarizer`, `_for_app`, `standalone_model_candidates`, `resolve_models_dir_standalone`, `create_diarizer_standalone`, `diarize_wav_samples`) | `core/factory.rs` |
| diarization.rs:1396-1500 (`DenseUnit`, `ChunkRecord`, `expand_embed_units`, `embed_unit_slices`) | `core/segment.rs` |
| diarization.rs:1501-1722 (`V2Core`) | `core/segment.rs` |
| diarization.rs:1723-1999 (`assemble_channel_turns`, `subtract_span`, `gap_fill_turns`) | `core/turns.rs` |
| diarization.rs:2000-2081 (`run_chunked_polyvoice_diarization`, `channel_chunks`) | `batch/chunking.rs` |
| diarization.rs:2082-2283 (`PcmStream`, `read_f32_le`, `spawn_ffmpeg_pcm`, `StreamWindows`) | `batch/pcm.rs` |
| diarization.rs:2284-2344 (`run_channel_diarization_stream`, `count_unique_speakers`) | `batch/orchestrator.rs` |
| diarization.rs:2345-2480 (`group_cluster_embeddings`, `persist_channel_clusters`, `persist_and_recognize_session`) | `persist/clusters.rs` |
| diarization.rs:2481-2594 (`compute_speaker_matches`, `find_best_speaker`) | `core/timeline.rs` |
| diarization.rs:2595-2689 (`emit_progress`, `MemorySampler`) | `telemetry.rs` |
| diarization.rs:2690-2777 (`DiarizationModelStatus`, `cleanup_legacy_models`, `check_diarization_models`) | `commands.rs` |
| online_diarization.rs:36-58 (`OnlineDiarizationGuard`) | `streaming/guard.rs` |
| online_diarization.rs:59-455 (`DiarizationMode`, `DiarChannel`, `ChannelStatusLine`, `OnlineDiarizationStats`, `OnlineDiarizationStatus`, stats statics/helpers) | `telemetry.rs` |
| online_diarization.rs:457-506 (`SpeakerAssignment`, `SpeakerTurn`, `OnlineClusterEmbeddings`) | `streaming/units.rs` |
| online_diarization.rs:508-678 (`PrototypeStore`, `parse_pipeline_id`) | `identity/prototypes.rs` |
| online_diarization.rs:679-690 (`create_enhanced_embedder`) | **deleted** → `core/factory.rs` |
| online_diarization.rs:691-765 (`SpeakerSegment`, `EmbeddingBuffer`) | `core/cluster.rs` |
| online_diarization.rs:766-825 (`TimelineMapper`, `FastChannel`, `Engine`) | `streaming/engine.rs` |
| online_diarization.rs:826-1475 (`OnlineDiarizationProcessor`) | `streaming/processor.rs` |
| online_diarization.rs:1476-1513 (`cluster_embeddings_by_labels`, `cluster_embeddings_by_overlap`) | **deleted** → `persist/clusters.rs::group_cluster_embeddings` |
| online_diarization.rs:1514-1533 (`create_fast_channel`) | `streaming/engine.rs` |
| online_diarization.rs:1535-1582 (`find_best_speaker`) | **deleted** → `core/timeline.rs` |
| live_diarization_reconcile.rs (all 778 lines) | `streaming/reconcile.rs` |
| speaker_recognition.rs (all 282 lines) | `identity/matching.rs` |
| recording_commands.rs:53,59,78,99 (`ONLINE_DIARIZATION_TASK`, `ONLINE_DIARIZATION_STORE`, `ONLINE_SESSION_DATA`, `ONLINE_TURN_OVERRIDES`, `ONLINE_EXPECTED_SPEAKER_IDS`) | `engine.rs` (private, reached only through the facade) |

`mod.rs` re-exports the union of today's `pub` items so the 30 external references and the `pub use` blocks in `audio/mod.rs:154-168` keep resolving. Tests move with their code, keeping their names, so the filtered test count does not change.

- Why a directory rather than splitting into sibling files: the tree is the thing 07 and 05b call; `audio::diarization::*` paths are already the dominant import spelling and stay valid.
- Alternative considered: keep two top-level modules and only extract shared helpers into a third. Rejected — it leaves two orchestrations, two parameter sources, and no place for the facade.

### D2: `DiarizationEngine` facade interface

`audio/diarization/engine.rs` exposes the whole capability to the rest of the app. Shapes are derived from what `recording_commands.rs` does today (session start 580-707, stop 1040-1155, `finalize_online_session` 1803-2043, `online_diarization_status` 2045-2117, `assign_live_speaker` 2185-2249, plus `start_diarization` diarization.rs:204-517):

```rust
pub struct DiarizationEngine;

/// Live session lifecycle -------------------------------------------------
pub struct LiveSessionConfig {
    pub mode: DiarizationMode,          // parsed from the frontend's mode string
    pub max_speakers: Option<i32>,      // user maximum; None = configured ceiling
    pub has_system_device: bool,        // fixes the mic label prefix for the session
    pub expected_speaker_ids: Vec<String>,
}

pub struct SessionOutcome {
    pub assignments: Vec<SpeakerAssignment>,
    pub cluster_embeddings: OnlineClusterEmbeddings,
    pub live_bindings: HashMap<String, String>,
}

pub struct PersistSummary { pub live_bindings: usize, pub enrolled: usize }

impl DiarizationEngine {
    /// Clears stale session state, installs telemetry, loads the prototype
    /// store, constructs the processor, and spawns the chunk-drain task.
    /// Returns Ok(false) when the mode is Off. Errors are reported to the
    /// frontend by the caller exactly as today.
    pub async fn start_live_session<R: Runtime>(
        app: &AppHandle<R>,
        pool: &SqlitePool,
        cfg: LiveSessionConfig,
        chunks: UnboundedReceiver<AudioChunk>,
    ) -> Result<bool, String>;

    /// Joins the drain task, runs the stop-time token repair, and produces the
    /// stop-time assignments. `None` when no live session was active.
    pub async fn finalize_session(
        transcripts: Vec<TranscriptSegment>,
        repair: TokenRepairSource,
    ) -> Result<Option<SessionOutcome>, String>;

    /// Persists the finalized session once the meeting row exists: expected
    /// speakers, cluster caches + recognition, enrollment, user bindings,
    /// per-turn overrides. No-op when no session is pending.
    pub async fn persist_session(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<PersistSummary, String>;

    /// Live correction: cluster-wide binding, or a single-turn override when
    /// `scope == Block`. Fails loudly when no live store is active.
    pub async fn assign_live_speaker(
        pool: &SqlitePool,
        req: LiveSpeakerAssignment,
    ) -> Result<AssignedSpeaker, String>;

    /// Read-only snapshot for the telemetry command; inactive shape when no
    /// session is running.
    pub fn telemetry_snapshot() -> OnlineDiarizationStatus;

    /// Read access to the live prototype store for the speaker commands that
    /// consult it today (replaces the direct static access in
    /// `database/speaker_commands.rs:562-566`).
    pub fn live_prototype_store() -> Option<Arc<RwLock<PrototypeStore>>>;

    /// Offline batch run over a saved meeting.
    pub async fn start_batch<R: Runtime>(
        app: &AppHandle<R>,
        req: BatchRequest,          // meeting_id, max_speakers, trigger
    ) -> Result<DiarizationResult, String>;
    pub fn cancel_batch();
    pub fn is_batch_running() -> bool;

    /// Persisted clustering parameters (the settings command's body).
    pub fn set_clustering_settings(s: ClusteringSettings) -> Result<(), String>;
}
```

- Why these members: they are exactly the diarization-owned blocks `recording_commands.rs` contains today, so change 07 can delete those blocks and leave `#[tauri::command]` wrappers.
- `start_live_session` takes the chunk receiver because the processor is driven by a `spawn_blocking` loop over it (recording_commands.rs:694-698); the sender stays owned by the recording manager.
- The `AppHandle` stays a parameter rather than being stored, keeping the engine free of Tauri state and keeping `diarize_eval`-style headless construction possible.
- Alternative considered: an instance struct holding the session. Rejected for this change — the process-wide statics (one live session at a time, enforced by `OnlineDiarizationGuard`) are moved but not redesigned, so a unit struct with associated functions is the smallest faithful shape. 05b may revisit.

### D3: `trait Clustering` with two implementations

```rust
pub trait Clustering {
    fn cluster(&self, embeddings: &[Vec<f32>]) -> Result<Vec<usize>, String>;
}
```

`GlobalAhc` wraps the existing `build_clusterer` output used by `V2Core::finish` (diarization.rs:1679-1697). `BufferedAhc` wraps what `EmbeddingBuffer::cluster` (online_diarization.rs:712) does today, including the `MinClusterSizeClusterer(.., 2)` singleton dissolution required by the `online-speaker-diarization` "Efficient mode prunes singleton clusters" requirement. Both are constructed by one factory that takes `DiarizationConfig` plus the effective ceiling, so kind, threshold, and the 255 clamp are resolved once.

- Efficient's clustering result is held constant except for the parameter source: the same AHC kind, the same min-cluster-size wrapper, the same default threshold value.
- Why a trait rather than calling `build_clusterer` directly from the live path: 05b adds an incremental/deferred implementation behind the same seam without touching the callers.

### D4: The live path resolves parameters at session start, not per chunk

`DiarizationConfig::resolved()` is read once when the live session starts and carried on the processor, so a settings change mid-recording cannot split a session across two configurations. The offline path keeps resolving per run, unchanged.

- Alternative considered: resolve inside `finalize`. Rejected because the Fast engine's construction (`create_fast_channel`) and the ceiling both need the value earlier, and a session that started under one ceiling should finish under it.

### D5: One attribution implementation in `core/timeline.rs`

`find_best_speaker` survives once, generic over the label type, together with `assign_tokens_to_speakers`'s three call sites reduced to one shared wrapper that returns blocks. The live reconcile path keeps its extra rule of declining an undecidable block (`live_diarization_reconcile.rs:301-303`) — that is a decision about *when* to attribute, not a different attribution rule, and the delta spec says so explicitly.

### D6: Both concurrency guards are kept, moved verbatim

`DiarizationGuard` and `OnlineDiarizationGuard` guard different invariants (one offline run at a time; one live session at a time) and are not merged. The `OnceLock` statics in `live_diarization_reconcile.rs:466-467` move as-is: they are what forbids two concurrent live sessions from sharing a registry, and the move must not weaken them.

### D7: Migration is move-only first, behavior second

Steps 1-3 are pure relocation and de-duplication that must leave the test count and every test name unchanged. Only steps 4-5 change behavior (parameter resolution, ceiling enforcement, the `Clustering` seam). This keeps the ~6k-line diff reviewable and keeps the two spec-visible changes isolated in small commits.

## Risks / Trade-offs

- **Step 1 touches ~6k lines and conflicts with `live-word-level-diarization` tasks 4.1/6.x** → Sequencing decision: that change lands first; this change starts from its result and never rebases it. The move-only step is a single commit so a conflict resolves as a path rename rather than a content merge.
- **Live clustering starts honoring stored overrides** → A user who tuned the offline threshold will see their live results change. Mitigated by keeping the built-in default identical to `TITANET_CLUSTER_THRESHOLD`, so an untouched installation behaves exactly as before, and by an explicit delta scenario asserting that.
- **Enforcing the ceiling on the live path changes a real value** (`0` → the configured default, clamped to 255) → For recordings with few speakers this is inert; it only binds on pathological fragmentation, which is the behavior the `diarization-param-tuning` requirement already demands offline.
- **Re-export surface drift**: a `pub` item missed in `mod.rs` breaks a caller in another crate module → The move-only step is verified with `cargo check -p meetily` plus `cargo clippy -p meetily --all-targets`, and the 12 referencing files are enumerated in D1 so each is checked.
- **Moving the session statics behind the facade touches `database/speaker_commands.rs:562-566`**, which currently `try_lock`s the store directly → Replaced by `DiarizationEngine::live_prototype_store()`; the non-blocking `try_lock` semantics there must be preserved, since that call site runs inside a command that must not block on a recording session.
- **`diarize_eval` imports must keep working** (`bin/diarize_eval.rs:9,116`) → `diarize_wav_samples`, `DiarizationConfig`, and `ClustererKindSetting` stay exported at `app_lib::audio::diarization::*`; the harness binary is not edited by this change.

## Migration Plan

1. **Move only.** Create `audio/diarization/` with `mod.rs` re-exports; relocate the four source files verbatim into the tree. No signature, no logic change. Zero behavior diff.
2. **`core/timeline.rs`.** Single attribution implementation; delete online_diarization.rs:1535.
3. **`identity/` and `persist/`.** Fold `speaker_recognition.rs` and `PrototypeStore` into `identity/`; delete `cluster_embeddings_by_*` and `create_enhanced_embedder`; move the offline inline split out of `start_diarization`.
4. **Config.** The live path reads `DiarizationConfig::resolved()` and the enforced, clamped ceiling. *(behavior change — spec delta 1)*
5. **`trait Clustering`.** Batch and Efficient both construct through the trait factory; Efficient's result held constant at default settings. *(seam for 05b)*
6. **Facade.** `DiarizationEngine` wraps the session statics and the batch entry; `database/speaker_commands.rs` and `recording_commands.rs` call it instead of the statics. Change 07 then moves orchestration bodies behind it.

Rollback: each step is its own commit; steps 1-3 and 6 are behavior-preserving, so reverting step 4 or 5 alone restores the previous parameter behavior without unwinding the move.

## Open Questions

- Should the live path also expose a per-session parameter override (distinct from the global setting), so a recording can be made with non-default clustering without changing the stored setting? Deferred: no user-facing need identified.
- Should `DiarizationEngine` eventually become an instance owned by `AppState` rather than a facade over statics? Deferred to 05b, which is the change that learns whether the session needs to be re-entrant.

## Assumptions to confirm with the owner

These are recorded as decisions taken for this change rather than blockers:

- **A1 (assumed, confirm with owner): both Fast and Efficient live engines are kept**, behind the `Clustering` trait. Removing Efficient is considered only in 05b, after eval numbers exist.
- **A2 (assumed, confirm with owner): sequencing.** `live-word-level-diarization` finishes first, then this change's move-only step lands; in-flight work is not rebased.
- **A3 (assumed, confirm with owner): default equivalence is the acceptance bar** for the parameter-unification step — with no stored overrides, live results must be identical to today's, and that is what the A/B check in task 4.3 verifies.
