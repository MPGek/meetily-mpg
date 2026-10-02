// audio/recording/mod.rs
//
// Recording orchestration behind the thin command layer in
// `recording_commands.rs`, which re-exports these functions under their
// original paths.

pub mod device_recovery;
pub mod devices;
pub mod lifecycle;
pub mod stop;
