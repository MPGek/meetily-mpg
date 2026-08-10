### Requirement: Backend exposes provider-aware model readiness check
The system SHALL provide a Tauri command `check_active_transcription_model_ready` that returns a structured response `{ ready: bool, provider: string, downloading: bool }` by reading the user's saved transcript config and validating the correct engine.

#### Scenario: User has Whisper selected and downloaded
- **WHEN** the user's transcript config has provider `localWhisper` and a Whisper model file exists on disk
- **THEN** the command SHALL return `{ ready: true, provider: "localWhisper", downloading: false }`

#### Scenario: User has Parakeet selected and downloaded
- **WHEN** the user's transcript config has provider `parakeet` and a Parakeet model file exists on disk
- **THEN** the command SHALL return `{ ready: true, provider: "parakeet", downloading: false }`

#### Scenario: Selected provider's model is currently downloading
- **WHEN** the user's transcript config has provider `localWhisper` and a Whisper model download is in progress
- **THEN** the command SHALL return `{ ready: false, provider: "localWhisper", downloading: true }`

#### Scenario: No transcript config saved (first run)
- **WHEN** no transcript config exists in the database
- **THEN** the command SHALL default to provider `parakeet` and return readiness based on Parakeet model availability

### Requirement: Frontend recording-start gate uses provider-aware check
The frontend SHALL use `check_active_transcription_model_ready` to determine whether recording can start, instead of the hardcoded Parakeet-only check.

#### Scenario: User with only Whisper model clicks record
- **WHEN** the user has downloaded a Whisper model, has Whisper selected in settings, and clicks the record button
- **THEN** the frontend SHALL allow recording to proceed (not block with "model not ready")

#### Scenario: User's selected model is downloading
- **WHEN** the user clicks record and the active provider's model is currently downloading
- **THEN** the frontend SHALL show an informational toast "Model download in progress" and block recording

#### Scenario: No model downloaded for selected provider
- **WHEN** the user clicks record and the active provider has no model downloaded or downloading
- **THEN** the frontend SHALL show an error toast "Transcription model not ready" and open the model selector

#### Scenario: All three recording-start paths use the same check
- **WHEN** recording is started via manual button, auto-start from sidebar navigation, or direct start from sidebar
- **THEN** all three paths SHALL use `check_active_transcription_model_ready` for the readiness gate

### Requirement: Tray menu uses provider-aware check
The tray menu `check_can_record` function SHALL validate the user's active transcription provider instead of checking only Parakeet.

#### Scenario: User with only Whisper model and incomplete onboarding
- **WHEN** onboarding is not complete and the user has only Whisper downloaded
- **THEN** the tray menu SHALL show "Start Recording" as enabled (not "Downloading transcription model...")

#### Scenario: During onboarding with no models downloaded
- **WHEN** onboarding is not complete and no transcription model is downloaded
- **THEN** the tray menu SHALL show "Downloading transcription model..." as disabled
