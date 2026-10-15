# Design

## Context

See proposal.md (Why) for the measurement. Relevant code today:

- `AudioCapture` (`audio/pipeline.rs:332-350`) holds `noise_suppressor: Arc<Mutex<Option<NoiseSuppressionProcessor>>>` next to `high_pass_filter`. Both are built in `AudioCapture::new` for `DeviceType::Microphone` only (`pipeline.rs:403-443`). `process_audio_data` runs them in order HPF → RNNoise (`pipeline.rs:645-698`) on the capture callback thread.
- Every microphone, including a hot-swapped replacement, is built through `AudioCapture::new` (`audio/stream.rs:138`, `:195`), so one edit covers both the start-time and the replacement chain.
- `RNNOISE_APPLY_ENABLED` lives in `audio/ffmpeg_mixer.rs:20` with a comment that contradicts its value, and is re-exported from `audio/mod.rs:150`. Nothing else reads it.
- `nnnoiseless` is used only by `NoiseSuppressionProcessor`. `realfft` is used only by `spectral_subtraction`. Neither of the two dead functions, nor `HighPassFilter::reset`, has a caller.
- RNNoise output is one 480-sample frame behind its input, and the processor holds up to 479 more samples until the next callback. The mic chunk timestamp is taken after processing (`pipeline.rs:734`), so the delay is in the sample stream, not in the timestamp.

## Goals / Non-Goals

**Goals:**
- The mic chain carries no stage that runs and does nothing.
- No code path, flag or dependency remains that suggests mic denoising exists.

**Non-Goals:**
- Changing the high-pass filter's design (first-order, 80 Hz). It is cheap, works, and is outside what was measured.
- Touching the system channel, the mixer, VAD or ASR.
- Re-processing past recordings. Their mic audio was never denoised, so it already matches the new behavior.

## Decisions

**D1: Remove the stage outright, not fix the scale.**
Multiplying by 32768 before the call and dividing after would turn on real denoising for the first time. That changes the audio that ASR, diarization and enrolled voiceprints see, and nothing has measured whether this helps on GigaAM/Whisper. Speech enhancement often raises WER on models trained on noisy data. Removal keeps today's audible behavior.
Alternatives: fix the scale now (rejected, untested behavior change); set the flag to `false` and keep the code (rejected, it leaves a broken processor that someone may turn back on with the same bug).

**D2: Delete the flag rather than keep it as a switch.**
With the processor gone, the flag has nothing to switch. A future real noise-suppression change should come with its own A/B and its own config. It can recover the processor from git history, adding the input scaling it lacked.

**D3: Delete the dead DSP code in the same change.**
`spectral_subtraction`, `average_noise_spectrum` and `HighPassFilter::reset` with its unused fields are noise-suppression leftovers in the same file. Removing them lets `realfft` go too. `HighPassFilter::new`'s log line uses its arguments directly, so dropping the two stored fields does not change the filter or its log.

**D4: Accept the 10 ms mic shift.**
Without RNNoise the mic samples reach the mixer one frame (10 ms) earlier against the system channel. RNNoise added that delay, so removing it brings the channels closer to true alignment. 10 ms is below anything the transcript, play-from-block or diarization can resolve, so nothing compensates for it and nothing needs to change.

## Risks / Trade-offs

- [A user relied on noise suppression] → It never ran (see proposal.md), so no output changes except the 10 ms shift.
- [Removing `realfft` breaks a transitive user] → Only the direct dependency goes. Crates that pull `realfft` in transitively keep their own entry in `Cargo.lock`. `cargo build` verifies this.
- [`Cargo.toml` has unrelated uncommitted edits (the `asr-eval` bin)] → Remove only the two dependency lines and leave the rest of the working-tree diff alone.

## Migration Plan

One code change; no data or config migration. Rollback: revert the commit.
