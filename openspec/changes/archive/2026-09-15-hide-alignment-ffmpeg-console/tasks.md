## 1. Implementation

- [x] 1.1 In `frontend/src-tauri/src/audio/word_alignment/refine.rs`, add the `#[cfg(target_os = "windows")]` block setting `creation_flags(CREATE_NO_WINDOW)` on the ffmpeg `Command` in `FileSpanSource::extract` (after building args, before `.output()`), matching the pattern in `diarization.rs:2132-2137`, and verify `cargo check` passes in `frontend/src-tauri`

## 2. Verification

- [ ] 2.1 On Windows with word alignment enabled and a meeting with unrefined tokens, press "Speakers" and verify no console windows flash during analysis and refinement results are unchanged (refined tokens still persisted)
- [ ] 2.2 Stop a recording with unrefined segments and verify no console windows flash during stop-time finalize repair
