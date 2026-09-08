# transcription-language-indicator Specification

## Purpose
Показывает выбранный язык транскрипции прямо на кнопке домашней страницы коротким кодом, чтобы пользователь видел активный режим без открытия настроек.
## Requirements
### Requirement: Short-code language indicator on home button
The system SHALL display the currently selected transcription language as a short code inside the home-page `Language` button text.

#### Scenario: Explicit language shown as code
- **WHEN** user selects explicit language `ru` in Language Settings and returns to home
- **THEN** the button reads `Language (ru)`

#### Scenario: Auto mode shown as auto
- **WHEN** selected language is `auto`
- **THEN** the button reads `Language (auto)`

#### Scenario: Auto-translate distinguished from auto
- **WHEN** selected language is `auto-translate`
- **THEN** the button reads `Language (auto-en)` and NOT `Language (auto)`

#### Scenario: Indicator updates without reopening settings
- **WHEN** user changes language in the modal and closes it
- **THEN** the button text updates immediately to the new code (reactive to `selectedLanguage`)

### Requirement: Indicator visible on narrow screens
The system SHALL keep the short code visible even when the full `Language` word is hidden by responsive rules.

#### Scenario: Mobile viewport
- **WHEN** viewport is below `md` breakpoint (word `Language` hidden, icon only)
- **THEN** the short code `(xx)` remains visible next to the icon

### Requirement: Dynamic tooltip with current language
The system SHALL set the button `title`/tooltip to include the current language code.

#### Scenario: Hover tooltip
- **WHEN** user hovers the `Language` button while `en` is selected
- **THEN** tooltip reads `Transcription language: en (click to change)` or equivalent containing the code
