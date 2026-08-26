## Why

When a user corrects a speaker label on a transcript block after running enhance (retranscription) and diarization, the voiceprint is not enrolled if the transcript's `speaker` column is NULL. This happens when diarization skips a transcript segment (no overlap with any detected speech turn) or when the segment cannot be gap-filled. The current implementation silently skips enrollment in these cases, leaving the user with no feedback that their correction didn't create a voiceprint, and preventing the speaker from being recognized in future meetings.

## What Changes

- Fix the single-block speaker correction path to enroll voiceprints even when the transcript's `speaker` column is NULL
- When a transcript has no cluster label, resolve the appropriate cluster by finding the best-matching cluster centroid by time overlap with the block's audio range
- Add user feedback (toast notification or log) when enrollment produces zero voiceprints due to missing audio data
- Ensure the VoiceprintBrowser reflects newly enrolled voiceprints without requiring manual refresh

## Capabilities

### New Capabilities

_None_

### Modified Capabilities

- `speaker-correction`: Fix enrollment to work when transcript has no cluster label by resolving cluster via time-overlap matching
- `speaker-identity-registry`: Clarify that single-block corrections MUST attempt enrollment even when the transcript's speaker column is NULL

## Impact

- **Backend**: `speaker_commands.rs` (`assign_block_speaker` function) needs to handle NULL speaker case by finding the best-matching cluster centroid
- **Database**: May need a new query to find cluster centroids by time overlap with a transcript's audio range
- **Frontend**: VoiceprintBrowser may need to refresh its data after speaker correction to show newly enrolled voiceprints
- **User experience**: Users will see their corrections actually create voiceprints, improving speaker recognition accuracy across meetings
