## 1. Database Schema

- [x] 1.1 Create SQLx migration adding `speaker TEXT`, `speaker_label TEXT` columns to `transcripts` table
- [x] 1.2 Create SQLx migration adding `diarization_status TEXT`, `speaker_names TEXT` columns to `meetings` table
- [x] 1.3 Add `speaker` and `speaker_label` fields to `Transcript` model in `database/models.rs`
- [x] 1.4 Add `diarization_status` and `speaker_names` fields to `MeetingModel` in `database/models.rs`
- [x] 1.5 Add `update_speaker_label()` query to `database/repositories/meeting.rs`
- [x] 1.6 Add `update_diarization_status()` and `update_speaker_names()` queries to meeting repository

## 2. Backend: Diarization Module

- [x] 2.1 Add `sherpa-rs` dependency to `Cargo.toml` with diarization feature
- [x] 2.2 Create `audio/diarization.rs` with `DiarizationConfig`, `DiarizationResult`, and `DiarizationProgress` types
- [x] 2.3 Implement `start_diarization(app, meeting_id) -> Result<()>` — decode audio, run sherpa-onnx pipeline, match to transcripts, persist
- [x] 2.4 Implement overlap-based speaker-to-transcript matching logic
- [x] 2.5 Implement SystemAudio override for `source_device="System"` segments
- [x] 2.6 Add `DiarizationManager` with cancellation support (`CancellationToken`)
- [x] 2.7 Emit `diarization-progress` events with status/progress/message
- [x] 2.8 Register `audio/diarization` module in `audio/mod.rs`
- [x] 2.9 Add `speaker: Option<String>` field to `TranscriptUpdate` in `transcription/worker.rs`

## 3. Backend: Tauri Commands & API

- [x] 3.1 Add `start_diarization` Tauri command
- [x] 3.2 Add `get_diarization_status` Tauri command
- [x] 3.3 Add `update_speaker_label` Tauri command
- [x] 3.4 Add `speaker` and `speaker_label` fields to `MeetingTranscript` API response struct
- [x] 3.5 Include speaker fields in `write_transcripts_json()` in `audio/common.rs`
- [x] 3.6 Register all new commands in `lib.rs`

## 4. Frontend: Data Layer

- [x] 4.1 Add `speaker?: string` and `speakerLabel?: string` to TypeScript `Transcript`, `TranscriptUpdate`, `TranscriptSegmentData` types
- [x] 4.2 Add `DiarizationProgress` and `SpeakerMap` TypeScript types
- [x] 4.3 Add `invoke` wrappers for `start_diarization`, `get_diarization_status`, `update_speaker_label` in frontend services
- [x] 4.4 Add `diarization-progress` event listener in `TranscriptContext` or meeting detail page
- [x] 4.5 Extend `usePaginatedTranscripts` to include speaker fields in returned segments

## 5. Frontend: Transcript Display

- [x] 5.1 Define 8-color speaker palette and color assignment logic
- [x] 5.2 Add speaker dot + label rendering above transcript bubble in `VirtualizedTranscriptView.tsx`
- [x] 5.3 Add speaker color left-border on transcript bubbles
- [x] 5.4 Maintain backward compatibility — no speaker data → existing source_device-only display
- [ ] 5.5 Add inline speaker rename UI (click label, type name, persist via Tauri command)
- [ ] 5.6 Add speaker grouping sections with collapsible headers (Phase 5+ stretch goal — mark as optional)
- [x] 5.7 Add "Re-analyze Speakers" button in meeting detail view, with loading/disabled states
- [ ] 5.8 Add diarization progress bar above transcript list when `diarization_status="processing"`

## 6. Frontend: Settings Page

- [ ] 6.1 Add "Speaker Diarization" section to settings page
- [ ] 6.2 Add enable/disable toggle with persistent state
- [ ] 6.3 Add model download UI with progress and status display
- [ ] 6.4 Add max speaker count selector (or threshold slider)
- [ ] 6.5 Add auto-run after recording toggle

## 7. Integration & Polish

- [ ] 7.1 Wire auto-trigger: call `start_diarization` in `stop_recording` flow when enabled
- [ ] 7.2 Handle diarization model download in settings (download to app data directory)
- [ ] 7.3 Handle model-not-downloaded graceful errors (clear message, link to settings)
- [ ] 7.4 Verify build on all target platforms (Windows primary, macOS, Linux)
- [ ] 7.5 Manual testing: record a meeting with 2+ speakers, run diarization, verify labels in UI and transcripts.json
