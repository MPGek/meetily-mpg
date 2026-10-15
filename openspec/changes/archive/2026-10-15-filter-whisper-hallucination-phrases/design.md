# Design

## Context

See proposal.md (Why) for the problem and specs/whisper-hallucination-filter/spec.md for the required behaviour.

`WhisperEngine` (`whisper_engine/whisper_engine.rs`) has three transcription functions. Each of them runs `state.full(...)`, walks the Whisper segments, trims each segment's text and joins the segments with a space. It then passes the joined text through `clean_repetitive_text`.

- `transcribe_audio_with_tokens` (line 765) is the live path, called from `audio/transcription/worker.rs`. Inside the loop it also builds one `Token` per whitespace word, with time slices interpolated within the segment. These tokens feed live word diarization and CTC alignment. When the cleaner changes the text, the tokens are rebuilt proportionally.
- `transcribe_audio_with_confidence` (line 575) is used by re-transcription and import (`retranscription.rs`, `import.rs`). It builds the same tokens internally but returns only the text.
- `transcribe_audio` (line 892) returns only the text. It is used by the Tauri command, `parallel_processor.rs` and other callers.

Whisper segments are capped at `max_len = 200` characters (`hardware_detector.rs`). Because of this cap, one hallucinated sentence can come back as two segments: the data has «Редактор субтитров А.» and «Семкин Корректор А.Егорова» as separate segments. Hallucinations can also follow real speech in the same chunk, as in «…раза. Продолжение следует...».

## Goals / Non-Goals

**Goals:**
- One shared filter for all three functions, so the live and offline paths behave the same.
- Live tokens stay consistent with the text: removed words have no tokens, and kept words keep the timings they had.

**Non-Goals:**
- Fixing words that are split where Whisper cut a segment in the middle of a word (see Risks). This is tracked separately.
- Changing `is_meaningless_output` or `clean_repetitive_text`.

## Decisions

### Filter the joined word sequence, not individual Whisper segments

Each function already turns its segments into an ordered list of words; the token-building paths also keep one `Token` per word. The filter runs on that list after the segment loop and before `clean_repetitive_text`. A helper takes the words and returns a keep-mask:

1. Group the words into sentences. A sentence ends at a word whose last character, after stripping closing quotes and brackets, is `.`, `!`, `?` or `…`. A trailing group without an end mark also counts as a sentence.
2. Normalise each sentence: lowercase, `ё`→`е`, strip surrounding quotes and trailing punctuation.
3. If the sentence matches any pattern, mark all its words as dropped.

The callers then keep only the words, and the tokens, that the mask keeps, and rebuild the text by joining them with a space. Kept tokens keep their existing time slices, so live diarization and alignment see the real timings of the remaining words.

Alternatives considered:
- *Per-segment matching.* This misses sentences that Whisper split across two segments because of `max_len`.
- *Extending `is_meaningless_output`.* That check drops the whole chunk on any substring match. It would delete real speech next to a hallucination and could not handle sign-off words that also occur inside real sentences.
- *Rebuilding tokens proportionally,* as the cleaner does today. That would shift the timings of the remaining words for no reason.

### Two kinds of patterns

Each kind is a list of regular expressions over the normalised sentence, compiled once with `once_cell::sync::Lazy`. The crate already uses `once_cell` and `regex`.

- **Whole-sentence patterns** must match the entire normalised sentence. These are `продолжение следует`, `спасибо за просмотр` and `до новых встреч`. All three also occur inside ordinary sentences.
- **Contained patterns** are matched anywhere in the sentence. These are credits and channel calls that do not occur in work speech:
  - `dimatorzok`;
  - `субтитры (создавал|сделал|делал|подготовил|подготовлены)`;
  - `(добавил|спасибо за) субтитры`;
  - `(редактор|автор|корректор) субтитров`;
  - `^корректор [а-я]\.\s*[а-я]+`;
  - `подписывайтесь на (наш|мой) канал`;
  - `ставьте лайк`.

The list comes from every hallucinated sentence found in the Meetily and whisper.cpp outputs in `transcript_compare/` (40,262 rows, two meetings). It matched all 375 hallucinated sentences and no real speech, including the near-misses «годовая подписка», «подписать коммент» and «с самоподписанным токеном». «Самый редактор.» was also seen (11 times), but its origin is unclear and «редактор» is an ordinary word, so it is left out.

The filter does not depend on the selected language: the patterns are Russian text, so they cannot match other languages by accident.

### Observability

When the filter drops something, the function logs the number of removed sentences and their text at debug level (`perf_debug!`, like the existing cleaner logs). Nothing is stored.

## Risks / Trade-offs

- [A real sentence is exactly «Продолжение следует.» or «До новых встреч!»] → It is removed. This is accepted: in meetings these are almost always hallucinations, and on the measured data every occurrence was.
- [New hallucination wording not in the list] → It is not filtered. The list lives in one place and can be extended from the same `transcript_compare/` data. Packing VAD chunks into longer windows, which is out of scope, removes hallucinations regardless of wording.
- [Mid-word segment splits] → `max_len` cuts can split a word («основ ные», «октя бря»). The filter only checks the last character of a word, so a split inside a word does not create a false sentence end. A cut right after a period is a real sentence end anyway. The split-word problem itself is separate.
- [Sentence-end detection on «А.Семкин» or «А. Семкин»] → An initial followed by a period can end a "sentence" early: «Редактор субтитров А.» and «Семкин Корректор А.Егорова». The first part matches `редактор субтитров`. The second part does not match and would be kept as stray text. Mitigation: a sentence that ends in a single-letter initial («А.») is joined with the next sentence before matching. A unit test covers both the spaced and unspaced forms.

## Migration Plan

None. The change only affects new transcription output. Already saved transcripts keep any hallucinations they have; the user can re-transcribe a meeting to get filtered text. Rollback is reverting the commit.
