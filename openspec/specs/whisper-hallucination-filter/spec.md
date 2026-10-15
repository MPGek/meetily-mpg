# whisper-hallucination-filter Specification

## Purpose

Keeps phrases that the Whisper engine invents from its training data — subtitle credits and video sign-offs that nobody said — out of meeting transcripts, while leaving real speech untouched.

## Requirements

### Requirement: Known Russian Whisper hallucinations are removed from transcript text
When the local Whisper engine transcribes audio, the system SHALL remove every sentence of its output that matches a known Whisper hallucination before that output becomes transcript text. The known hallucinations are subtitle credits, video sign-offs and channel calls. This SHALL apply to live transcription, re-transcription and file import, whatever the selected transcription language.

#### Scenario: Live chunk that is only a sign-off
- **WHEN** during recording Whisper returns «Продолжение следует...» for a short chunk
- **THEN** no transcript row is created for that chunk

#### Scenario: Subtitle credit in re-transcription
- **WHEN** re-transcription of a saved meeting returns «Субтитры создавал DimaTorzok» for a segment
- **THEN** the re-transcribed transcript does not contain that text

#### Scenario: Credit variants
- **WHEN** Whisper returns «Субтитры сделал DimaTorzok», «Добавил субтитры DimaTorzok», «Редактор субтитров А.Семкин Корректор А.Егорова», «Автор субтитров А.Семкин Корректор А.Егорова», «Корректор А.Кулакова» or «Спасибо за субтитры Алексею Дубровскому!»
- **THEN** each of them is removed

#### Scenario: Channel calls
- **WHEN** Whisper returns «Подписывайтесь на наш канал и ставьте лайки!» or «Подписывайтесь на мой канал.»
- **THEN** it is removed

#### Scenario: Language set to auto
- **WHEN** the transcription language is "auto" and Whisper returns «Спасибо за просмотр!» for a chunk
- **THEN** it is removed

### Requirement: Only the hallucinated sentence is removed
The system SHALL remove a matching sentence and keep the other sentences of the same Whisper output in their original order. Output that becomes empty SHALL produce no transcript text. Word timings for the transcript SHALL be derived from the kept text only.

#### Scenario: Hallucination appended to real speech
- **WHEN** Whisper returns «Пока. Редактор субтитров А.Семкин Корректор А.Егорова» for one segment
- **THEN** the transcript text is «Пока.»

#### Scenario: Real speech before a sign-off
- **WHEN** Whisper returns «Это всё на сегодня. Продолжение следует...»
- **THEN** the transcript text is «Это всё на сегодня.»

#### Scenario: Word timings skip removed text
- **WHEN** a live chunk's output is «Хорошо. Продолжение следует...»
- **THEN** the chunk's word tokens cover only «Хорошо.» and contain no token for the removed sentence

### Requirement: Sign-offs are removed only when they are the whole sentence
Sign-off patterns that also occur in ordinary speech («Продолжение следует», «Спасибо за просмотр», «До новых встреч») SHALL match only when they make up an entire sentence, ignoring case, surrounding quotes and trailing punctuation. Words such as «подписка», «подписать», «редактор» or «субтитры» SHALL NOT cause removal on their own.

#### Scenario: Sign-off words inside a real sentence
- **WHEN** Whisper returns «Продолжение следует в пятницу на планинге.»
- **THEN** the sentence is kept unchanged

#### Scenario: Ordinary words that resemble credit vocabulary
- **WHEN** Whisper returns «Ну, понятно, годовая подписка.», «Но он просил тогда подписать коммент, но он ничего не ответил.» or «Добавь субтитры к видео в плеере.»
- **THEN** each sentence is kept unchanged

#### Scenario: Plausible short replies are not touched
- **WHEN** Whisper returns «Спасибо.», «Спасибо за внимание.» or «Угу.»
- **THEN** the text is kept unchanged
