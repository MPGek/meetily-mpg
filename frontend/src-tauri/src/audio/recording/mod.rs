// audio/recording/mod.rs
//
// Recording orchestration behind the thin command layer in
// `recording_commands.rs`, which re-exports these functions under their
// original paths.

pub mod devices;
pub mod lifecycle;
pub mod stop;
