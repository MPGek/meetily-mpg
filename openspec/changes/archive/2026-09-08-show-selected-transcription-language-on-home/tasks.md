## 1. Label helper

- [x] 1.1 Add short-code label helper (`auto` -> `auto`, `auto-translate` -> `auto-en`, else code, fallback `auto`) and verify unit check for `en`/`ru`/`auto`/`auto-translate`/unknown passes
- [x] 1.2 Wire helper to use single source (reuse `constants/languages.ts` where possible) and verify no new duplicate language list is introduced

## 2. Home button indicator

- [x] 2.1 Read `selectedLanguage` from `ConfigContext` in `TranscriptPanel` and render `Language (xx)` via helper, verify button shows `Language (ru)` after selecting `ru`
- [x] 2.2 Keep short code visible below `md` breakpoint (outside `hidden md:inline`) and verify code remains visible in narrow viewport while word hides
- [x] 2.3 Make button `title`/tooltip dynamic with current code and verify hover/long-press shows e.g. `Transcription language: ru`

## 3. Regression verification

- [x] 3.1 Verify button still opens `languageSettings` modal and stays hidden for non-`localWhisper` providers
- [x] 3.2 Verify indicator updates immediately after modal change without reload, and `frontend` typecheck/lint passes
