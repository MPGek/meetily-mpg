## Why

На домашней странице кнопка `Language` (панель транскрипта) показывает только статичный текст и не отражает текущий выбор языка Whisper (`auto`, `auto-translate`, `en`, `ru`, ...). Пользователь не понимает, какой режим активен, не открывая модалку `Language Settings`.

## What Changes

- Текст кнопки `Language` на домашней странице дополняется коротким кодом текущего языка: `Language (en)`, `Language (ru)`, `Language (auto)`, `Language (auto-en)`.
- Для `auto-translate` отображается distinct-лейбл `auto-en` (не путать с `auto`).
- Код виден всегда, включая mobile (вне `hidden md:inline`).
- Тултип (`title`) кнопки становится динамическим: текущий язык вместо статичного `"Language"`.
- Поведение кнопки (открытие `languageSettings`, видимость только для `localWhisper`) не меняется.

## Capabilities

### New Capabilities

- `transcription-language-indicator`: отображение выбранного языка транскрипции на кнопке домашней страницы (формат короткого кода, distinct-лейбл для auto-translate, видимость на mobile, динамический тултип).

### Modified Capabilities

- `whisper-engine`: существующее требование `Language support (auto, auto-translate, explicit)` дополняется видимым индикатором выбранного режима на домашней странице (без изменения самой логики транскрипции).

## Impact

- Affected code: `frontend/src/app/_components/TranscriptPanel.tsx` (кнопка Language), `frontend/src/contexts/ConfigContext.tsx` (источник `selectedLanguage`), `frontend/src/components/LanguageSelection.tsx` + `frontend/src/constants/languages.ts` (маппинг code -> короткий лейбл, дубликат списков).
- Без изменений бэкенда / Rust / IPC. Только отображение, источник правды (`selectedLanguage`, `primaryLanguage`, `set_language_preference`) не меняется.
- Без breaking changes.
