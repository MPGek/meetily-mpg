# mic-gain-and-ducking Specification

## Purpose

Defines microphone capture gain behavior and the system-audio ducking decision so non-speech content is never amplified by automatic gain and system audio is ducked only when microphone speech is actually present.

## Requirements

### Requirement: Live microphone capture does not amplify non-speech content
The system SHALL NOT apply any automatic level gain to the live microphone signal that raises quiet or non-speech content toward a fixed loudness target. The microphone enhancement chain SHALL keep noise suppression and the high-pass filter, and SHALL NOT include a loudness-normalization step that adapts gain while no speech is present. The captured microphone level SHALL remain at natural (unity) gain, subject only to user-controlled input volume, so ambient noise such as street sound is not boosted.

#### Scenario: No speech, ambient noise stays quiet
- **WHEN** recording captures only ambient noise on the microphone and no speech occurs
- **THEN** the processed microphone signal SHALL remain at its natural level and SHALL NOT be amplified by automatic gain toward any target loudness

#### Scenario: Quiet gaps do not pump gain
- **WHEN** the microphone captures speech interrupted by long quiet gaps
- **THEN** the quiet gaps SHALL NOT cause gain to rise such that background noise becomes louder when speech resumes

#### Scenario: Noise suppression still applied
- **WHEN** recording captures ambient noise alongside speech
- **THEN** the microphone chain SHALL still apply the high-pass filter and RNNoise-style noise suppression, reducing the noise floor without boosting it

#### Scenario: No live AGC on any capture path
- **WHEN** microphone audio is processed by any live capture path (current pipeline or future modern recorder)
- **THEN** no adaptive per-buffer or per-chunk gain (peak-normalizing or loudness-targeting without speech gating) SHALL be applied to microphone audio

### Requirement: System-audio ducking follows speech activity
The system SHALL decide when to duck system audio based on speech activity detected in the microphone channel, not on the processed microphone signal level. Ducking SHALL engage when microphone speech is detected, SHALL disengage with a short debounce (on the order of 0.5 seconds) when speech is absent, and SHALL NOT engage merely because microphone gain made noise louder.

#### Scenario: No speech keeps system audio at full level
- **WHEN** the microphone captures no speech (for example, only ambient noise)
- **THEN** system audio SHALL be mixed at full level and SHALL NOT be ducked

#### Scenario: Detected speech ducks system audio
- **WHEN** microphone speech activity is detected
- **THEN** system audio SHALL be attenuated by the configured duck factor

#### Scenario: Ducking releases smoothly after speech ends
- **WHEN** microphone speech ends and a short debounce period passes
- **THEN** system audio SHALL return to full level without abrupt level jumps

#### Scenario: Boosted noise does not trigger ducking
- **WHEN** the microphone captures loud non-speech noise and no speech is detected
- **THEN** the system SHALL NOT duck system audio based on the noise level alone