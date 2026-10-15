# Proposal

## Why

Whisper writes phrases into Russian meeting transcripts that nobody said: «Продолжение следует...», «Субтитры создавал DimaTorzok», «Редактор субтитров А.Семкин Корректор А.Егорова», «Спасибо за просмотр!», «До новых встреч!». These are subtitle credits and video sign-offs from Whisper's training data. Whisper produces them when a speech chunk is short (0.4–1.5 s) and surrounded by silence. On the two hand-checked meetings in `transcript_compare/` the live transcript had 23 such lines in 99 minutes and 9 in 30 minutes. Re-transcription had 8 in the 99-minute meeting. They show up as transcript rows and are fed to the summary.

Meetily already drops a list of meaningless outputs in `WhisperEngine::is_meaningless_output` (`whisper_engine/whisper_engine.rs:458`). The list is English only («thank you for watching», «like and subscribe», …), so none of the Russian phrases match. The check also tests whether a phrase occurs anywhere in the whole chunk text and then drops the whole chunk. Applied to Russian this would delete real speech that happens to contain such a phrase.

Changing how audio is cut also removes these lines: packing VAD pieces into ≤25 s windows gave zero hallucinations at the same WER. That is a larger change, so it is deliberately not part of this one. A phrase filter fixes the visible problem now in all modes, live included.

## What Changes

- A fixed list of known Whisper hallucination patterns for Russian is added. They are subtitle credits («Субтитры создавал/сделал …», «Редактор/Автор/Корректор субтитров …», «Спасибо за субтитры …», «Добавил субтитры …», any text naming DimaTorzok, «Корректор А.<Фамилия>») and sign-offs that match only when they make up the whole sentence («Продолжение следует», «Спасибо за просмотр», «До новых встреч»), plus channel calls («Подписывайтесь на наш/мой канал», «ставьте лайк»).
- Each Whisper segment is checked **per sentence**. A matching sentence is removed and the rest of the segment is kept. A segment left empty is skipped. Its words do not get token timings.
- The filter runs in all three Whisper transcription paths: live (`transcribe_audio_with_tokens`), re-transcription and import (`transcribe_audio_with_confidence`), and `transcribe_audio`. It runs regardless of the selected language, because Whisper produces these lines even when the language is set to auto.
- Removed lines are counted in a debug log line so they can be inspected; they are not stored.

Out of scope:

- Packing VAD chunks into longer windows (offline or live). The measurements are in `transcript_compare/`; it would be a separate change.
- Hallucinations that are also plausible speech: «Спасибо.», «Спасибо за внимание.», «Угу.» on silence. They cannot be told apart from real replies by text alone.
- The existing English list and its whole-chunk behaviour stay as they are.
- Cleaning transcripts that are already saved.
- Parakeet. It does not produce these phrases.

## Capabilities

### New Capabilities

- `whisper-hallucination-filter`: which Whisper output text is treated as a known hallucination and removed before it becomes transcript text, and what is kept.

### Modified Capabilities

None.

## Impact

- `frontend/src-tauri/src/whisper_engine/whisper_engine.rs`: a pattern list and a sentence-level filter, applied inside the per-segment loops of the three transcription functions before text and tokens are collected; unit tests.
- Live transcription, re-transcription ("Enhance"), and file import, whenever the local Whisper engine is used.
- No API, database, settings or UI changes. No new dependencies: `regex` is already a dependency of the crate.
- Validated against the real outputs in `transcript_compare/`. The patterns match all 375 hallucinated sentences found across 40,262 rows of Meetily and whisper.cpp output on both meetings, and no real speech.
