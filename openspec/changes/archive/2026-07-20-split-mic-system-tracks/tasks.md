## 1. Data Model

- [x] 1.1 Add `channels: u16` field to `AudioChunk` in `audio/recording_state.rs`
- [x] 1.2 Add `channels: u16` field to `ProcessedAudioChunk` in `audio/recording_state.rs`
- [x] 1.3 Update all `AudioChunk` constructors across codebase to include `channels` value

## 2. AudioCapture — Stereo Output

- [x] 2.1 Add `stereo_interleave_mic(data: &[f32]) -> Vec<f32>` helper — interleaves mic samples as `[mic, 0.0, mic, 0.0, ...]`
- [x] 2.2 Add `stereo_interleave_sys(data: &[f32]) -> Vec<f32>` helper — interleaves sys samples as `[0.0, sys, 0.0, sys, ...]`
- [x] 2.3 Modify `AudioCapture::process_audio_data()` — replace `audio_to_mono()` with stereo interleave based on `device_type`
- [x] 2.4 Set `channels: 2` in `AudioChunk` emitted by `AudioCapture`

## 3. AudioPipeline — Dual VAD

- [x] 3.1 Replace single `vad_processor` field with `vad_processor_mic: ContinuousVadProcessor` and `vad_processor_sys: ContinuousVadProcessor` in `AudioPipeline` struct
- [x] 3.2 Add `extract_channel(data: &[f32], channel: usize, num_channels: usize) -> Vec<f32>` helper — extracts single channel from interleaved stereo
- [x] 3.3 Update `AudioPipeline::new()` — create two VAD instances with same config (redemption_time=400ms)
- [x] 3.4 Modify `AudioPipeline::run()` — extract mono channel before VAD: mic channel=0, sys channel=1
- [x] 3.5 Route VAD segments to transcription with correct `device_type` (not hardcoded `Microphone`)
- [x] 3.6 Set `channels: 1` in VAD transcription chunks, `channels: 2` in recording chunks

## 4. Stereo Recording Output

- [x] 4.1 Replace `ProfessionalAudioMixer::mix_window()` with `interleave_stereo(mic: &[f32], sys: &[f32]) -> Vec<f32>` in pipeline.rs
- [x] 4.2 Update `AudioPipeline::run()` step 4 — emit `interleave_stereo()` result with `channels: 2` to `recording_sender_for_mixed`
- [x] 4.3 Update `IncrementalAudioSaver::save_checkpoint()` — change `channels` from `1` to `2` in `encode_single_audio()` call
- [x] 4.4 Update `RecordingSaver::add_chunk()` — handle stereo chunks (`channels: 2`) for stereo `.mp4` checkpoints
- [x] 4.5 Update `MeetingMetadata` audio recording metadata if channel count is stored — N/A (not stored; header intrinsic)
- [x] 4.6 Remove `ProfessionalAudioMixer` struct and all its usage

## 5. Transcription Source Labeling

- [x] 5.1 Add `source_device: String` field to `TranscriptUpdate` in `audio/transcription/worker.rs`
- [x] 5.2 Add `source_device: String` field to `TranscriptSegment` in `audio/recording_saver.rs`
- [x] 5.3 Populate `source_device` from `chunk.device_type` in `transcribe_chunk_with_provider()`
- [x] 5.4 Map `DeviceType::Microphone` → `"Microphone"`, `DeviceType::System` → `"System"` for JSON serialization

## 6. Pipeline Flush

- [x] 6.1 Update `AudioPipeline::flush_remaining_audio()` — flush both VAD instances independently
- [x] 6.2 Tag flushed segments with correct `device_type` per VAD source

## 7. Cleanup

- [x] 7.1 Remove unused `ProfessionalAudioMixer` import — struct fully deleted
- [x] 7.2 Remove unused `audio_to_mono` import from `AudioCapture` — still used for CPAL multi-channel → mono conversion; no removal needed
- [x] 7.3 Run `cargo check` to find compilation errors from missing `channels` field in constructors — passes cleanly

## 8. Performance Remediation

- [x] 8.1 Change AudioCapture to send mono data (channels: 1) instead of stereo-interleaved — eliminates duplicate deinterleave in pipeline
- [x] 8.2 Update AudioMixerRingBuffer::add_samples() to accept mono data directly with bulk extend_from_slice — replaces per-element push_back
- [x] 8.3 Remove extract_channel() call in pipeline loop — pass mono directly to VAD
- [x] 8.4 Add per-source mono accumulation buffers in pipeline for window-batched VAD dispatch (≥200ms windows)
- [x] 8.5 Replace naive resample_to_16k() in vad.rs with rubato-based resampler or pre-resample to 16kHz before VAD
- [x] 8.6 Profile and validate CPU usage — target comparable or lower than pre-split baseline
