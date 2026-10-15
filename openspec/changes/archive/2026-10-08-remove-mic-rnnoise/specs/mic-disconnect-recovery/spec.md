# Spec Delta

## MODIFIED Requirements

### Requirement: A replacement microphone gets the same processing as a start-time microphone
The replacement microphone SHALL go through the same processing chain as a microphone chosen at recording start: the high-pass filter. It SHALL stay at natural (unity) gain with no automatic gain. No per-device gain setting SHALL need to be carried over.

#### Scenario: Quiet room after the switch
- **WHEN** the replacement microphone captures only ambient noise
- **THEN** its signal SHALL NOT be amplified toward a loudness target
