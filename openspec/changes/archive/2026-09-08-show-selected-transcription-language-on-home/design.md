## Context

См. `proposal.md` (Why). Текущее состояние: кнопка в `frontend/src/app/_components/TranscriptPanel.tsx:79-91` — статичный текст `Language`, статичный `title`, рендер только при `transcriptModelConfig.provider === "localWhisper"`, `selectedLanguage` не читается. Источник правды — `ConfigContext.selectedLanguage` (`localStorage 'primaryLanguage'`, default `'auto'`, синк в Rust через `set_language_preference`). Маппинг кодов дублирован: полный список в `LanguageSelection.tsx:13`, короткий в `constants/languages.ts:2`.

## Goals / Non-Goals

**Goals:**
- Показать короткий код выбранного языка в тексте кнопки, с distinct-лейблом для `auto-translate`.
- Сохранить видимость кода на mobile и дать динамический тултип.
- Переиспользовать существующий `selectedLanguage` без изменения логики сохранения/синка.

**Non-Goals:**
- Не менять логику транскрипции, Rust, IPC, `LanguageSelection` modal.
- Не менять gating по провайдеру (кнопка по-прежнему только для `localWhisper`).
- Не делать полный рефактор дубликата `LANGUAGES` — только минимальный хелпер для лейбла, единый источник по возможности.
- Не трогать `ImportAudioDialog` / `RetranscribeDialog` (у них локальный `selectedLang`).

## Decisions

1. **Формат `Language (xx)`, lowercase как в дропдауне** — вместо UPPERCASE-бейджа или полного имени. Rationale: пользователь выбрал "достаточно короткий код"; lowercase совпадает с `LanguageSelection.tsx:195` (`(code)` suffix). Альтернатива `Language: EN` / pill-бейдж — отклонена как более заметный редизайн.
2. **`auto-translate` -> `auto-en`** — вместо `auto` (путаница) или `trans` (жаргон). Rationale: коротко, явно отличает от `auto`, отражает выход (всегда EN). Альтернатива `en-auto` — отклонена, менее читается.
3. **Код вне `hidden md:inline`** — текст `Language` остается responsive-скрытым, а `(xx)` рендерится в всегда видимом `span`. Rationale: иначе на mobile индикатор снова пропадет. Альтернатива — вынести всё наружу — отклонена (ломает текущий responsive-дизайн).
4. **Динамический `title` с кодом** — `Transcription language: <code>` вместо статичного `"Language"`. Rationale: ноль риска по верстке, плюс для a11y/скринридеров. Альтернатива — без тултипа — отклонена как упущенный бесплатный выигрыш.
5. **Хелпер `getShortLanguageLabel(code)` рядом с кнопкой или в `constants/languages.ts`** — `auto->auto`, `auto-translate->auto-en`, иначе сам код; fallback `auto` для неизвестного. Rationale: изолирует маппинг в одном месте, не тянет полный список из `LanguageSelection`. Полная унификация двух `LANGUAGES` — вне скоупа.

## Risks / Trade-offs

- [Редкие коды неочевидны] (`haw`, `bo`) → Mitigation: принято сознательно (решение пользователя "короткий код достаточен"); тултип и модалка дают полное имя.
- [Дубликат `LANGUAGES` остается] → Mitigation: хелпер не усугубляет дубликат, полный рефактор — отдельным change.
- [Неизвестный код из localStorage] → Mitigation: fallback на `auto`, кнопка никогда не пустая.

## Migration Plan

Не требуется. Чисто фронтенд-отображение, без миграций, без изменений API. Rollback — вернуть статичный текст кнопки.
