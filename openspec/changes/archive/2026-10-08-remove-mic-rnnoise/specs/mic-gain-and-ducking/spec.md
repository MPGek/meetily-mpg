# Spec Delta

## ADDED Requirements

### Requirement: Live microphone capture keeps its natural level and noise floor
The system SHALL NOT apply any automatic level gain to the live microphone signal that raises quiet or non-speech content toward a fixed loudness target. The microphone enhancement chain SHALL consist of the high-pass filter only, SHALL NOT include a noise-suppression stage, and SHALL NOT include a loudness-normalization step. The captured microphone level SHALL remain at natural (unity) gain, subject only to user-controlled input volume, so ambient noise is not boosted.

#### Scenario: No speech, ambient noise stays quiet
- **WHEN** recording captures only ambient noise on the microphone and no speech occurs
- **THEN** the processed microphone signal SHALL remain at its natural level and SHALL NOT be amplified by automatic gain toward any target loudness

#### Scenario: Quiet gaps do not pump gain
- **WHEN** the microphone captures speech interrupted by long quiet gaps
- **THEN** the quiet gaps SHALL NOT cause gain to rise such that background noise becomes louder when speech resumes

#### Scenario: Only low-frequency rumble is removed
- **WHEN** recording captures ambient noise alongside speech
- **THEN** the microphone chain SHALL apply the high-pass filter, removing content below about 80 Hz, and SHALL otherwise pass the signal at unity gain without noise suppression, so the saved and transcribed microphone audio keeps its natural noise floor

#### Scenario: No live AGC on any capture path
- **WHEN** microphone audio is processed by any live capture path (current pipeline or future modern recorder)
- **THEN** no adaptive per-buffer or per-chunk gain (peak-normalizing or loudness-targeting without speech gating) SHALL be applied to microphone audio

## REMOVED Requirements

### Requirement: Live microphone capture does not amplify non-speech content
**Reason**: It required RNNoise-style noise suppression in the mic chain. That stage never ran correctly (its input was scaled 90 dB too low, so it left the signal unchanged) and is removed. The no-AGC rules it held are carried over unchanged.
**Migration**: Replaced by "Live microphone capture keeps its natural level and noise floor", which keeps the no-AGC scenarios and states the chain as high-pass filter only.
