//! The offline batch pass over a saved recording: streaming PCM decode,
//! chunked diarization, and the run orchestration.

pub mod chunking;
pub mod guard;
pub mod orchestrator;
pub mod pcm;
