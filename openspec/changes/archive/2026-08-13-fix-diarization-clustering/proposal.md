## Why

Speaker diarization collapses every distinct voice into a single "Speaker 1" label. Both the offline and online clustering steps use polyvoice's `AhcClusterer` **auto-threshold** heuristic instead of the **calibrated fixed threshold** (0.45) that the Balanced profile's ResNet34 embeddings were tuned against. The result is one dominant cluster plus a few noise fragments, so a multi-person recording shows only "Speaker 1" even though the run log reports "5 unique speakers".

## What Changes

- Replace auto-threshold AHC clustering with fixed-threshold clustering at the Balanced profile's calibrated threshold (`DEFAULT_AHC_THRESHOLD` = 0.45) in the offline diarization engine (`audio/diarization.rs`).
- Apply the same fixed-threshold clustering to online Efficient mode (`audio/online_diarization.rs`).
- Wire the user's `maxSpeakers` setting into the clusterer as a hard ceiling; it is currently ignored in both paths (online always passes 0; offline passes `None` when the setting is 0).
- Optionally dissolve spurious singleton clusters via `MinClusterSizeClusterer` (polyvoice's shipped pruning behavior), defaulting to no pruning to match the powerset pipeline's tuned default.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `speaker-diarization`: offline clustering must assign distinct speaker labels to distinct speakers (calibrated threshold) instead of collapsing them into one cluster.
- `online-speaker-diarization`: Efficient-mode clustering must apply the same calibrated-threshold behavior so online and offline labels stay consistent.

## Impact

- `frontend/src-tauri/src/audio/diarization.rs` — offline clusterer construction (`create_polyvoice_diarizer`).
- `frontend/src-tauri/src/audio/online_diarization.rs` — Efficient-mode `EmbeddingBuffer::cluster`.
- No DB schema, API, migration, or frontend changes; the `SPEAKER_NN` / `MIC_SPEAKER_NN` label format is unchanged.
