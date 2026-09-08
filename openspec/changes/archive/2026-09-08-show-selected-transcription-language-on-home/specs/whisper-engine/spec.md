## MODIFIED Requirements

### Requirement: Language support (auto, auto-translate, explicit)
The system SHALL support three language modes: automatic detection, auto-detect + translate to English, and explicit language code. The currently selected mode SHALL be visible on the home-page `Language` button as a short code (`(xx)`, `(auto)`, `(auto-en)` for auto-translate).

#### Scenario: Transcribe in French with auto mode
- **WHEN** user sets language preference to "auto" for a recording session
- **THEN** Whisper detects French automatically and transcribes in French

#### Scenario: Transcribe Japanese and translate to English
- **WHEN** user sets language preference to "auto-translate"
- **THEN** Whisper transcribes in Japanese and translates output to English

#### Scenario: Selected mode visible on home without opening settings
- **WHEN** user is on the home page with any language mode selected
- **THEN** the `Language` button displays the short code of that mode (`(auto)`, `(auto-en)`, or explicit code such as `(ru)`)
