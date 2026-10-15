## Context

Meetily is a Tauri v2 desktop app with a Rust backend that captures mic + system audio, transcribes via Whisper/Parakeet, and displays transcripts in a chat-style UI. It already uses ONNX Runtime for Silero VAD v6. The "Enhance" (retranscription) feature established a pattern for post-processing stored audio. Speaker diarization follows this same pattern.

The current transcript model tracks `source_device` (Microphone/System) for each segment. There is already an unused `speaker TEXT` column in the database from an abandoned earlier attempt, and `TranscriptionResult` already computes a `speaker_embedding: Vec<f32>` that is thrown away.

The primary constraint is meetily's **local-first, privacy-first** design — no cloud services, all models run on-device.

## Goals / Non-Goals

**Goals:**
- Post-recording speaker diarization using ONNX models (sherpa-onnx), following the "Enhance" pattern
- Per-transcript speaker labels stored in database and displayed in UI
- Per-speaker color coding in the transcript view
- Manual "Re-analyze Speakers" trigger on any past meeting
- Settings UI for model download and feature enablement

**Non-Goals:**
- Streaming/live diarization during recording (Phase 2+)
- Voiceprint registration / cross-meeting speaker matching (Phase 2+)
- Multi-microphone speaker separation
- Overlap detection (single-channel mic capture prevents this anyway)
- DIY embedding/clustering — we use sherpa-onnx, not raw embeddings from Whisper

## Decisions

### Decision 1: sherpa-onnx official Rust crate (v1.13.4)

**Chosen**: Use the official `sherpa-onnx` crate from k2-fsa (v1.13.4), which provides safe Rust wrappers for `OfflineSpeakerDiarization`. The deprecated community `sherpa-rs` crate is not used.

**Alternatives considered**:
- **pyannote.audio directly**: Requires Python runtime. Non-starter for a packaged desktop app.
- **DIY with existing speaker_embedding from Whisper**: Whisper embeddings are not designed for speaker discrimination (they encode linguistic content). Would need a speaker embedding model anyway, plus custom clustering. Reinventing wheels.

**Rationale**: `sherpa-onnx` is maintained alongside the C/C++ library, has an idiomatic Rust API (`OfflineSpeakerDiarization::create()` / `process()`), bundles its own ONNX runtime statically via `sherpa-onnx-sys`, and auto-downloads prebuilt native libraries from GitHub releases during `cargo build`. Models are small (~1.5MB segmentation + ~25MB embedding) and run at RTF 0.1–0.3 on CPU.

### Decision 2: UNIFIED diarization across mic+system, not per-channel

**Chosen**: Feed the full stereo audio to diarization, not per-channel. Label system audio segments as "System Audio" regardless of diarization output.

**Alternatives considered**:
- **Diarize each channel separately**: Would produce different speaker labels for the same person on mic vs system. Matching them would be error-prone.

**Rationale**: System audio is usually one source (a video call). Diarization on it would label it as "SPEAKER_00" while the same person on mic gets "SPEAKER_02." Instead, we diarize the full audio, then override all segments with `source_device="System"` to `speaker = "SystemAudio"`. This keeps speaker labels meaningful only for microphone segments.

### Decision 3: Post-processing trigger — auto after recording + manual button

**Chosen**: By default OFF (opt-in). When enabled, auto-runs after `stop_recording` completes. A "Re-analyze Speakers" button on any meeting's detail view re-runs it.

**Alternatives considered**:
- **Always auto-run**: Increases recording shutdown time and forces model download. Opt-in respects user choice and resource constraints.

### Decision 4: Speaker naming — per-meeting labels, no global identity

**Chosen**: Speaker labels (`speaker_label`) are per-meeting. No cross-meeting identity.

**Rationale**: Cross-meeting voiceprint matching is Phase 2 material and significantly more complex (voiceprint enrollment, storage, matching threshold tuning). Per-meeting naming is the 80/20 solution.

### Decision 5: Build and dependency strategy

**Chosen**: Use `sherpa-onnx` with default static linking. Models are lazy-downloaded to `{app_data}/models/` on first use, NOT bundled in the installer.

**Rationale**: Static linking avoids runtime symbol conflicts with meetily's existing `ort` crate. The sherpa-onnx build script caches prebuilt static libraries in `target/` — first build downloads ~50MB per platform from GitHub releases, subsequent incremental builds are fast. Models (~25MB) are downloaded separately because bundling them would bloat every installer update even when models haven't changed, and the user can opt out of downloading them entirely if they don't use diarization.

## Risks / Trade-offs

- **[Risk] Build size and CI impact**: `sherpa-onnx`'s first build downloads ~50MB of prebuilt static libraries per platform from GitHub releases for static linking. → **Mitigation**: Cargo's build cache keeps subsequent builds fast. CI pipelines should cache `target/`. The linked binary grows by ~5MB — acceptable for desktop.
- **[Risk] Accuracy on real meeting audio**: Diarization models are trained on clean speech datasets. Meetily meetings may have background noise, overlapping speech, or poor mic quality. → **Mitigation**: Test with real meetily recordings before shipping. Accept lower accuracy as a known limitation; diarization is "best effort."
- **[Risk] Model download failures**: Users behind proxies or without internet can't download models. → **Mitigation**: Models are optional. Diarization is opt-in. Show clear error messages and retry UI in settings.
- **[Risk] Performance on long meetings**: Sherpa-onnx RTF is ~0.2 on CPU, meaning a 1-hour meeting takes ~12 minutes to diarize. → **Mitigation**: Show a progress bar. Run on a background thread so the app stays responsive. Users can do other things while it runs.
- **[Risk] Dependency conflict with `ort` crate**: Using two ONNX Runtime crates could cause symbol conflicts or doubled binary size. → **Mitigation**: `sherpa-onnx` statically links its own ONNX runtime internally via `sherpa-onnx-sys`, avoiding runtime symbol clashes. Test build on all platforms to confirm.

## Migration Plan

1. **Database migration**: Add columns (`speaker`, `speaker_label` on transcripts; `diarization_status`, `speaker_names` on meetings) via SQLx migration. All new columns are nullable — existing data is unaffected.
2. **Model download**: Users download ONNX models from Settings page. Models stored in app data directory alongside other models.
3. **Feature toggle**: Diarization disabled by default. No impact on existing users until enabled.
4. **Rollback**: If diarization is disabled, speaker columns remain NULL and UI falls back to existing display. No data migration needed to roll back.

## Open Questions

- Should "Re-analyze Speakers" also re-run transcription (like "Enhance"), or only diarization? Proposed: diarization only — it operates on existing transcripts.
- How to handle speaker count estimation? Sherpa-onnx supports both `num_clusters` and `cluster_threshold`. Proposed: expose both in settings, default to `cluster_threshold=0.5` (auto-detect).

## Resolved Questions

- ~~Should the diarization model be bundled with the app or always downloaded?~~ **Resolved**: Always downloaded to `{app_data}/models/` alongside Whisper/Parakeet models. 25MB is too large to bloat every installer update. The Settings page provides download UI (tasks 6.3, 7.2).
