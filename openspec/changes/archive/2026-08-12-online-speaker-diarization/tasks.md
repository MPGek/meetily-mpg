## 1. Spike: Polyvoice API Verification (BYO-embedder mode)

- [x] 1.1 Add `polyvoice = "=0.17.0"` dependency in BYO mode (`default-features = false` + `clusterer`, no `onnx` feature); pin `ort = "=2.0.0-rc.10"` so the Parakeet/VAD stack is untouched; workspace resolves and compiles
- [x] 1.2 Standalone probe: sherpa-onnx 3D-Speaker embedder returns 512-dim embeddings from ~0.25s windows (readiness threshold determined)
- [x] 1.3 Verify the `StreamingPipeline` (`LatencyPreset::Balanced`) feed/flush API produces stable `SpeakerTurn`s on synthetic audio with the sherpa embedder (run end-to-end in the standalone probe)
- [x] 1.4 Verify the Efficient-mode path: sherpa embedder per speech segment + `AhcClusterer` produces cluster labels (run end-to-end in the standalone probe)
- [x] 1.5 Document API findings in design.md open questions section

## 2. Frontend: Diarization Mode Setting

- [x] 2.1 Add `diarizationMode` field to `DiarizationSettings` type in `frontend/src/lib/diarization.ts` with values `"off" | "efficient" | "fast"`
- [x] 2.2 Update `loadDiarizationSettings()` and `saveDiarizationSettings()` to include `diarizationMode` with default `"efficient"`
- [x] 2.3 Add mode dropdown UI to the diarization settings section (Fast / Efficient / Off) with descriptive tooltips
- [x] 2.4 Pass `diarizationMode` through `recordingService.startRecording` to the Tauri `start_recording_with_devices_and_meeting` command

## 3. Rust: OnlineDiarizationProcessor Module

- [x] 3.1 Create `frontend/src-tauri/src/audio/online_diarization.rs` with `OnlineDiarizationProcessor` struct containing mode enum, embedding extractor (Efficient mode), and streaming pipeline (Fast mode)
- [x] 3.2 Implement `OnlineDiarizationProcessor::new(mode: DiarizationMode, models_dir: &Path) -> Result<Self>` — initializes polyvoice components based on mode (`SherpaEmbedder` over the sherpa-onnx 3D-Speaker model for Efficient, `StreamingPipeline` for Fast)
- [x] 3.3 Implement `process_chunk(&mut self, chunk: AudioChunk)` — routes audio to embedding extraction (Efficient) or full streaming pipeline (Fast); no-op if in error state
- [x] 3.4 Implement `EmbeddingBuffer` struct: stores `Vec<(f32, f32, Vec<f32>)>` (start_time, end_time, embedding_vector)
- [x] 3.5 Implement `finalize(&mut self, transcripts: &[TranscriptSegment]) -> Result<Vec<SpeakerAssignment>>` — for Efficient mode: runs `AhcClusterer`; for Fast mode: flushes buffered stable turns; returns speaker updates keyed by `sequence_id`
- [x] 3.6 Register module in `audio/mod.rs`
- [x] 3.7 Add per-channel state: `EmbeddingBuffer` instances keyed by `device_type` (Efficient mode) and one `StreamingPipeline` instance per channel (Fast mode); route incoming chunks by `device_type`
- [x] 3.8 In `finalize()`, map microphone-buffer clusters to `MIC_SPEAKER_NN` and system-buffer clusters to `SPEAKER_NN`, matching transcripts by `source_device` (same scheme as offline per-channel diarization)

## 4. Rust: Pipeline Integration (embedding_sender Channel)

- [x] 4.1 Add `embedding_sender: Option<UnboundedSender<AudioChunk>>` field to `AudioPipelineManager` struct in `pipeline.rs`
- [x] 4.2 Add parameter to `AudioPipelineManager::start()` to accept optional `embedding_sender`
- [x] 4.3 In the VAD dispatch section of the pipeline run loop, clone the audio chunk to `embedding_sender` when it exists (after `transcription_sender.send()`)
- [x] 4.4 Add `DiarizationMode` enum to shared types in Rust (mirroring frontend)

## 5. Rust: Recording Commands Integration

- [x] 5.1 Accept `diarization_mode: Option<String>` parameter in `start_recording_with_devices_and_meeting` command in `recording_commands.rs` (and the wrapper in `lib.rs`)
- [x] 5.2 When `diarization_mode` is `"fast"` or `"efficient"`, create `embedding_sender` channel, spawn `OnlineDiarizationProcessor` in a `tokio::task::spawn_blocking`, and pass sender to `AudioPipelineManager::start()`
- [x] 5.3 Store `OnlineDiarizationProcessor` handle in `RecordingManager` alongside transcription worker
- [x] 5.4 On recording stop: call `finalize()` on the online diarization processor, carry speaker assignments (by `sequence_id`) in the `recording-stopped` event payload, and persist them via the save path (`TranscriptsRepository::save_transcript` writes the `speaker` column and sets `diarization_status = "complete"`)
- [x] 5.5 If `finalize()` returns an error, log it, flag the frontend that online diarization is unavailable, and fall back to offline diarization (existing `start_diarization` path) if auto-run is enabled

## 6. Frontend: Recording Stop Flow Integration

- [x] 6.1 In `TranscriptContext.tsx` auto-trigger logic: if `diarizationMode` is `"efficient"` or `"fast"` and speaker assignments arrived with `recording-stopped`, skip calling `startDiarization()` (speaker labels already applied at save time)
- [x] 6.2 If `diarizationMode` is `"off"` (or online diarization failed) and `autoRun` is true, call `startDiarization()` as before (unchanged offline path)
- [x] 6.3 Update `useRecordingStop.ts` with the same conditional logic; attach `speaker_assignments` to transcripts before `saveMeeting`

## 7. Testing and Polish

- [ ] 7.1 Test Efficient mode: record a meeting, verify speaker labels appear at stop without offline diarization delay
- [ ] 7.2 Test Fast mode: record a meeting, verify speaker labels appear at stop
- [ ] 7.3 Test mode Off: verify offline diarization still works via manual "Re-analyze Speakers"
- [ ] 7.4 Test fallback: force polyvoice initialization failure, verify offline diarization still works after recording
- [ ] 7.5 CPU profiling: measure CPU increase in Efficient vs Fast vs Off modes during recording
- [ ] 7.6 Test system audio transcripts labeled `SPEAKER_NN` and microphone transcripts labeled `MIC_SPEAKER_NN` in both modes
- [ ] 7.7 Test concurrent recording guard: verify second recording skips online diarization when first is active
- [ ] 7.8 Test consistency: re-analyzing an online-diarized meeting with offline diarization produces the same `SPEAKER_NN` / `MIC_SPEAKER_NN` label scheme
