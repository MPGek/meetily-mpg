## Why

Meetily transcribes meetings but cannot tell who said what. Users reviewing meeting transcripts must manually trace who spoke each line. Speaker diarization — identifying "who spoke when" — is the highest-impact missing feature for meeting intelligence. Existing open-source ONNX models (sherpa-onnx) make this feasible to run entirely locally, matching meetily's privacy-first design.

## What Changes

- **New `audio/diarization.rs` module** using sherpa-onnx ONNX models for speaker segmentation and embedding extraction, running as a post-processing step after recording (same pattern as "Enhance" retranscription)
- **Database schema extension**: add `speaker` and `speaker_label` columns to transcripts, `diarization_status` and `speaker_names` to meetings
- **Transcript display**: per-speaker color coding, speaker grouping sections with collapsible headers, inline speaker renaming, and a "Re-analyze Speakers" button in meeting view
- **Settings page**: enable/disable diarization, model download UI, max speaker count, auto-run toggle
- **New dependency**: `sherpa-onnx` Rust crate (v1.13.4) wrapping official sherpa-onnx C libraries, plus two ONNX models (~25MB total) downloaded lazily on first use

## Capabilities

### New Capabilities
- `speaker-diarization`: Speaker diarization pipeline — ONNX-based segmentation + embedding + clustering, triggered post-recording or manually per meeting, producing per-transcript speaker labels

### Modified Capabilities
- `audio-engine`: New `audio/diarization.rs` submodule integrated into the audio engine's stopping flow and re-processing UI
- `database`: New columns (`speaker`, `speaker_label` on transcripts; `diarization_status`, `speaker_names` on meetings) and corresponding repository queries
- `split-transcript-ui`: VirtualizedTranscriptView extended with speaker color bands, grouping, naming, and "Re-analyze Speakers" trigger

## Impact

- **Affected code**: `audio/mod.rs`, new `audio/diarization.rs`, `database/models.rs`, `database/repositories/meeting.rs`, DB migrations, `api/api.rs`, `audio/transcription/worker.rs` (TranscriptUpdate), `VirtualizedTranscriptView.tsx`, Settings page, meeting details page
- **New dependency**: `sherpa-onnx` crate (v1.13.4, ~5MB linked binary, auto-downloads prebuilt static libs during build; first build ~50MB download, cached for subsequent builds) + 2 ONNX model files (~25MB, lazy-downloaded)
- **API**: New Tauri commands (`start_diarization`, `get_diarization_status`, `update_speaker_label`), new event (`diarization-progress`)
- **No breaking changes** — speaker fields are nullable, diarization is opt-in, existing transcripts display unchanged when no speaker data exists
