## Context

The current audio pipeline captures mic and system audio as separate streams, converts both to mono, mixes them via `ProfessionalAudioMixer::mix_window()`, then runs a single `ContinuousVadProcessor` on the mixed signal. The mixed mono is also what gets saved to disk.

Problems:
1. Source identity is lost — transcription can't distinguish local from remote speech
2. Single VAD uses one threshold for both sources, which may be suboptimal (system audio often has different level characteristics)
3. Saved recording is mono — no way to extract individual tracks later
4. `AudioChunk` has no channel count, making it impossible to carry stereo data

The recording pipeline is at `frontend/src-tauri/src/audio/pipeline.rs`, the VAD at `vad.rs`, and the data model at `recording_state.rs`.

## Goals / Non-Goals

**Goals:**
- Microphone audio lives on left channel, system audio on right channel throughout the pipeline
- Two independent VAD processors — one per source — with configurable thresholds
- Transcription segments carry `source_device` metadata ("Microphone" or "System")
- Final saved recording is stereo MP4 (AAC) with mic on L, system on R
- Existing Whisper/Parakeet engine interfaces unchanged — they still receive mono 16kHz segments

**Non-Goals:**
- Per-source VAD threshold configuration via UI (use same hardcoded thresholds for now)
- Parallel VAD processing (two instances run sequentially in the current tokio task — parallelism is a future optimization)
- Separate audio file export per channel
- Changing Whisper or Parakeet interfaces
- GPU-accelerated channel extraction

## Decisions

### 1. AudioChunk stays flat with `channels` field (not separate left/right vectors)

**Rationale:** Two `Vec<f32>` fields would double allocator pressure. The `channels` discriminator lets the same struct carry:
- `channels=1` for VAD segments (mono, 16kHz) sent to transcription
- `channels=2` for recording chunks (stereo interleaved, 48kHz) sent to `RecordingSaver`

Existing FFmpeg encoding in `encode.rs` already accepts a `channels: u16` parameter — no change needed there.

**Alternative considered:** Separate `MonoChunk`/`StereoChunk` enum. Rejected — adds branching in every consumer without meaningful safety gain.

### 2. Interleaved stereo for recording, separate ring buffers for VAD

The `AudioMixerRingBuffer` already has `mic_buffer: VecDeque<f32>` and `system_buffer: VecDeque<f32>` as separate queues. This is ideal: VAD reads directly from its dedicated buffer without deinterleaving. Only at recording time does `extract_window()` interleave into a single `[L,R,L,R,...]` vector for FFmpeg.

```
RingBuffer::mic_buffer ──→ VAD_Mic  ──→ transcription_sender
RingBuffer::sys_buffer ──→ VAD_Sys  ──→ transcription_sender
                                │
                    interleave_stereo() ──→ recording_sender (stereo)
```

**Alternative considered:** Interleaved ring buffer + deinterleave for VAD. Rejected — adds unnecessary stride iteration per VAD frame (30ms × 60fps = 1800 deinterleaves/sec).

### 3. One transcription sender channel, device_type distinguishes sources

Both VAD instances send to the same `transcription_sender: mpsc::UnboundedSender<AudioChunk>`. The existing `chunk.device_type` field already exists and currently always says `DeviceType::Microphone` (hardcoded). After this change, VAD segments carry the actual source.

**Rationale:** The transcription worker is single-threaded serial. Adding a second channel would require merging/ordering logic — unnecessary complexity.

### 4. ProfessionalAudioMixer replaced by stereo interleave function

The mixer was: `sum = mic + sys`, soft-clip. The new function is: `interleave(mic, sys) → [mic₀, sys₀, mic₁, sys₁, ...]`. No scaling, no clipping — pure multiplexing.

**Rationale:** Each channel stays independent. Loudness normalization (EBU R128) continues per-source in `AudioCapture` for the mic, preserving existing audio quality processing.

### 5. VAD redemption time stays unified (400ms) for both sources

No per-source tuning yet. Both VAD instances use the same `ContinuousVadProcessor::new(sample_rate, 400)`.

## Risks / Trade-offs

- **[Risk] Stereo files double disk usage** → Mitigation: Acceptable — 128kbps AAC mono → ~192kbps AAC stereo is a ~50% increase, not 2x, due to joint stereo encoding efficiency
- **[Risk] Existing replay UI expects mono** → Mitigation: Frontend reads channel count from metadata; stereo downmix is trivial (`(L+R)/2`). Add a follow-up task if needed
- **[Risk] Two VAD instances double VAD CPU** → Mitigation: Silero VAD is extremely cheap (~0.1ms per 30ms chunk on M1); real impact is negligible. If profiling shows otherwise, VAD can be made `Send` and parallelized
- **[Risk] Ring buffer memory doubles** → Mitigation: From ~115KB to ~230KB per 600ms window — well within budget

## Open Questions

1. Should `source_device` be added to the frontend transcript display? (Out of scope for this change — backend-only)
2. Should we expose per-source VAD thresholds in settings UI? (Deferred — use same 0.50/0.35 for both initially)
