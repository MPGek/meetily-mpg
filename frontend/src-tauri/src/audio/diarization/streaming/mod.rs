//! Live (streaming) diarization: the online processor, its engines, and the
//! display-only word-level reconcile stage.

pub mod engine;
pub mod guard;
pub mod processor;
pub mod reconcile;
#[cfg(test)]
pub(crate) mod source;
pub mod units;

pub use guard::*;
pub use processor::*;
pub use reconcile::*;
pub use units::*;

// A live session's telemetry and its prototype store live in the modules that
// own those concerns; re-exported here so `audio::online_diarization::*` (the
// alias in `audio/mod.rs`) still resolves to one streaming surface.
pub use super::identity::prototypes::*;
pub use super::telemetry::*;
