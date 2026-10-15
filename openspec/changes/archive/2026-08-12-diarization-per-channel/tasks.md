## 1. Rust: Per-channel diarization

- [x] 1.1 In `run_diarization_blocking` (`audio/diarization.rs`), de-interleave the decoded audio via `extract_channels()` instead of passing `decoded.samples` directly; handle mono (`right == None`) as a single remote-only stream
- [x] 1.2 Refactor `run_sherpa_diarization` so the `OfflineSpeakerDiarization` instance is created once and reused for both channel runs (split creation from the per-channel `process()` call)
- [x] 1.3 Run the pipeline twice for stereo: once on the mic stream, once on the system stream, keeping the per-channel segment lists separate
- [x] 1.4 Update `compute_speaker_matches` to take both segment lists: transcripts with `source_device="System"` match system-channel segments; all others match mic-channel segments; remove the `"SystemAudio"` override branch
- [x] 1.5 Map cluster indices to namespaced IDs: mic-run cluster N → `MIC_SPEAKER_0N`, system-run cluster N → `SPEAKER_0N` (format `{:02}`), including the mono fallback where all transcripts get `SPEAKER_0N`
- [x] 1.6 Verify `diarization_status` / progress flow is unchanged (loading → decoding → diarizing → matching → complete)

## 2. Frontend: Speaker ID rendering

- [x] 2.1 Update `formatSpeakerId` in `VirtualizedTranscriptView.tsx` to handle `MIC_SPEAKER_NN` (display "Mic Speaker N+1") and `SPEAKER_NN` (display "Speaker N+1")
- [x] 2.2 Update `getSpeakerColor` to extract the numeric index from both prefixes (fallback: stable fixed color for non-numeric IDs)
- [x] 2.3 Render the legacy `SystemAudio` value as "System Audio" with a stable color instead of "Speaker NaN"
- [x] 2.4 Verify inline rename persists per namespace (`speaker_names` map keyed by full ID) for both `MIC_SPEAKER_NN` and `SPEAKER_NN`

## 3. Testing

- [x] 3.1 Record a meeting with both channels active; run "Speakers" — verify local speech labeled `MIC_SPEAKER_NN` and remote speech labeled `SPEAKER_NN`
- [x] 3.2 Verify a meeting where only the mic side speaks: system transcripts remain unlabeled, no run failure
- [x] 3.3 Import a mono audio file; run diarization — verify all matched transcripts get `SPEAKER_NN`
- [x] 3.4 Re-analyze a meeting previously diarized in v1 (had `SystemAudio` transcripts) — verify system transcripts now get `SPEAKER_NN` and legacy labels are replaced
- [x] 3.5 Verify rename persistence: rename `MIC_SPEAKER_00` and `SPEAKER_00` separately in the UI, confirm both labels survive a reload
- [x] 3.6 Regression: meetings with only `SPEAKER_NN` transcripts (pre-stereo era) still render and rename correctly
