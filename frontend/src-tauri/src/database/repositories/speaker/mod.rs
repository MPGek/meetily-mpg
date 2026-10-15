//! Speaker identity repository. `SpeakerRepository` is a unit struct whose
//! associated functions are split by responsibility across the submodules;
//! every public type is re-exported here, so callers keep using
//! `crate::database::repositories::speaker::{SpeakerRepository, ...}`.

mod binding;
mod crud;
mod enrollment;
mod merge;
mod overrides;
mod stats;
#[cfg(test)]
mod test_support;
mod voiceprints;

pub use binding::*;
pub use enrollment::*;
pub use merge::*;
pub use stats::*;
pub use voiceprints::*;

/// Model tag identifying the extractor that produced stored embeddings.
///
/// Legacy ResNet34 INT8 tag is `resnet34_int8` (underscore). The DB historically
/// stored `resnet34-int8` (dash); queries accept both so no migration is needed.
/// New writes always use the underscore form. When enhanced models are active
/// the producing tag is `titanet_large` (192-d); recognition filters by tag.
pub const SPEAKER_EMBEDDING_MODEL: &str = "resnet34_int8";
/// Legacy alias retained for backward-compatible queries (pre-upgrade rows).
pub const SPEAKER_EMBEDDING_MODEL_LEGACY_DASH: &str = "resnet34-int8";

/// Best-K exemplars reparented into a speaker per enrollment (design open
/// question: start at 8). Tunable as field data accumulates.
pub const ENROLLMENT_BEST_K: usize = 8;

/// Maximum prototypes kept per speaker. Above this, lowest-duration rows are
/// pruned (design open question: start at 64).
pub const PER_PERSON_PROTOTYPE_CAP: usize = 64;

pub struct SpeakerRepository;
