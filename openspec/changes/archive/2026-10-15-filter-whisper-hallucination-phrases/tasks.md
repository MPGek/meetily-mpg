# Tasks

Run Rust commands from the repo root. "Full Rust tests" means `cargo test -p meetily --lib -- --skip audio::playback_monitor --skip audio::system_audio_commands`, the same skips earlier changes used.

## 0. Preconditions and baseline

- [x] 0.1 Re-verify the anchors that design.md (Context) cites and note any drift in this task. Verify: each one still matches.
  - Note (2026-10-14): no drift. `transcribe_audio_with_confidence` L575, `transcribe_audio_with_tokens` L765, `transcribe_audio` L892; all three trim/join segments then call `clean_repetitive_text`; both token paths push one `Token` per whitespace word in the loop. `max_len: 200` in `hardware_detector.rs`; `once_cell = "1.17.1"`, `regex = "1.11.0"` in `Cargo.toml`.
  - `whisper_engine.rs`: `transcribe_audio_with_confidence`, `transcribe_audio_with_tokens` and `transcribe_audio`. Each has a segment loop that trims and joins segment text, then calls `clean_repetitive_text`. The two token paths build one `Token` per whitespace word inside the loop.
  - `hardware_detector.rs`: `max_len: 200`.
  - `once_cell` and `regex` are dependencies of the crate in `Cargo.toml`.
- [x] 0.2 Record baselines in this task's note:
  - Note (2026-10-14): full Rust tests 648 passed / 0 failed / 10 ignored (3 filtered out); clippy warning count 31.
  - full Rust tests (pass/fail/ignored);
  - `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep -c "^frontend.*warning:"`.

  Verify: both numbers are written down. Group 3 compares against them.

## 1. Hallucination filter as a pure function

- [x] 1.1 In `whisper_engine.rs` (or a small sibling module `whisper_engine/hallucination_filter.rs` if that keeps the file readable), add:
  - the two pattern lists from design.md (whole-sentence and contained), compiled once with `once_cell::sync::Lazy`;
  - a function that takes the ordered words of one transcription and returns a keep-mask plus the removed sentences.

  The function groups words into sentences (end marks `.` `!` `?` `…` after stripping closing quotes and brackets), joins a sentence that ends in a single-letter initial («А.») with the next one, normalises each sentence (lowercase, `ё`→`е`, strip surrounding quotes and trailing punctuation) and matches it. Verify: `cargo check -p meetily` succeeds.
- [x] 1.2 Unit tests for the function, one per spec scenario. Verify: `cargo test -p meetily --lib hallucination` passes with:
  - removed: «Продолжение следует...», «Продолжение следует.», «Субтитры создавал DimaTorzok», «Субтитры сделал DimaTorzok», «Добавил субтитры DimaTorzok», «Редактор субтитров А.Семкин Корректор А.Егорова», «Автор субтитров А.Семкин Корректор А.Егорова», «Корректор А.Кулакова» (with and without «»), «Спасибо за субтитры Алексею Дубровскому!», «Спасибо за просмотр!», «До новых встреч!», «Подписывайтесь на наш канал и ставьте лайки!», «Подписывайтесь на мой канал.»;
  - the credit split as «Редактор субтитров А.» + «Семкин Корректор А.Егорова» (separate words, as after a `max_len` cut) is removed entirely, in both the «А.Семкин» and «А. Семкин» forms;
  - only the bad sentence is removed: «Пока. Редактор субтитров А.Семкин Корректор А.Егорова» → «Пока.»; «Это всё на сегодня. Продолжение следует...» → «Это всё на сегодня.»;
  - kept unchanged: «Продолжение следует в пятницу на планинге.», «Ну, понятно, годовая подписка.», «Но он просил тогда подписать коммент, но он ничего не ответил.», «Добавь субтитры к видео в плеере.», «с самоподписанным каким-нибудь токеном», «Спасибо.», «Спасибо за внимание.», «Угу.», «Самый редактор.»;
  - empty input returns an empty mask, and text with no end mark is treated as one sentence.

## 2. Apply the filter in the three transcription paths

- [x] 2.1 `transcribe_audio_with_tokens` (live): after the segment loop, apply the mask to `all_tokens` and rebuild `result` from the kept tokens before `clean_repetitive_text`. Kept tokens keep their start/end. Log removed sentences with `perf_debug!`. Verify: `cargo check -p meetily` succeeds, and a unit test on the extracted token step (tokens for «Хорошо. Продолжение следует...» with known timings) returns only the «Хорошо.» token with its timing unchanged.
- [x] 2.2 `transcribe_audio_with_confidence` (re-transcription and import): the same step on its internal tokens and `result`. Verify: `cargo check -p meetily` succeeds. If 2.1 extracted a shared helper for "filter tokens and rebuild text", this path calls the same helper; check this by reading the code.
- [x] 2.3 `transcribe_audio`: apply the mask to the whitespace words of the joined `result` before `clean_repetitive_text`. Verify: `cargo check -p meetily` succeeds and the path calls the same filter function.

## 3. Integration checks

- [x] 3.1 Full Rust tests and clippy against the 0.2 baselines. Verify: no new failures; the clippy warning count does not grow.
  - Note (2026-10-14): 656 passed / 0 failed / 10 ignored (baseline 648 + 8 new); clippy warnings 31 (baseline 31), none in the changed files.
- [ ] 3.2 Re-transcription check in the app (manual): re-transcribe meeting `2026-10-07_13-00`, which has «Продолжение следует» and DimaTorzok lines in its current transcript. Verify: the new transcript has no row matching the patterns, and the number of rows otherwise stays close to before.
- [ ] 3.3 Live check in the app (manual): record a few minutes in Russian with long silent pauses between short replies. Verify: no «Продолжение следует» / «Субтитры …» rows appear live or in the saved transcript.
