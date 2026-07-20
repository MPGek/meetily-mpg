## 1. Channel Extraction

- [x] 1.1 Add `extract_channels()` method to `DecodedAudio` struct in `audio/decoder.rs` that returns `(Option<Vec<f32>>, Option<Vec<f32>>)` for left and right channels
- [x] 1.2 Implement stereo de-interleaving logic: for 2-channel audio, extract even-indexed samples as left channel, odd-indexed as right channel
- [x] 1.3 Handle mono case: return `(Some(samples), None)` for 1-channel audio
- [x] 1.4 Add unit tests for channel extraction with stereo and mono inputs

## 2. Per-Channel Resampling

- [x] 2.1 Create helper function `resample_channel()` that resamples a single channel to 16kHz using existing `resample_audio()` or `chunked_resample_with_progress()`
- [x] 2.2 Apply resampling to each extracted channel independently
- [x] 2.3 Normalize and clamp samples after resampling (existing logic)

## 3. Per-Channel VAD

- [x] 3.1 Modify `retranscription.rs` to call `get_speech_chunks_with_progress()` separately for mic channel and system channel
- [x] 3.2 Update progress reporting: allocate 20-25% for mic VAD, 25-30% for system VAD (adjust ranges if mono)
- [x] 3.3 Handle cancellation checks between channel VAD passes
- [x] 3.4 Store VAD results as `(mic_segments, system_segments)` tuple

## 4. Per-Channel Transcription

- [x] 4.1 Modify transcription loop to process mic segments separately from system segments
- [x] 4.2 Update progress reporting: allocate remaining 70% proportionally by segment count between channels
- [x] 4.3 Create `TranscriptSegment` with `source_device: Some("Microphone")` for mic channel results
- [x] 4.4 Create `TranscriptSegment` with `source_device: Some("System")` for system channel results
- [x] 4.5 For mono audio, create segments with `source_device: None`

## 5. Result Merging

- [x] 5.1 Merge mic and system transcript segments into single `Vec<TranscriptSegment>`
- [x] 5.2 Sort merged results by `audio_start_time` ascending
- [x] 5.3 Verify merged results maintain chronological order

## 6. Mono Backward Compatibility

- [x] 6.1 Add check: if `decoded.channels == 1`, use original mono path (single VAD, single transcription, `source_device: None`)
- [x] 6.2 Ensure progress reporting for mono matches original behavior
- [ ] 6.3 Test with mono audio file to verify backward compatibility

## 7. Integration Testing

- [ ] 7.1 Test stereo retranscription with meeting that has both mic and system audio
- [ ] 7.2 Verify chat-style UI displays correctly after retranscription (mic=left/blue, system=right/green)
- [ ] 7.3 Test mono retranscription with old meeting (pre-stereo era)
- [ ] 7.4 Verify mono retranscription renders as neutral legacy style
- [ ] 7.5 Test cancellation during stereo retranscription
- [ ] 7.6 Verify progress updates correctly during stereo retranscription

## 8. Documentation

- [x] 8.1 Update code comments in `retranscription.rs` to explain stereo vs mono paths
- [x] 8.2 Add doc comment to `extract_channels()` explaining return value semantics
