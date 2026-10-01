pub mod commands;
pub mod metadata;
// Named after its parent directory by convention; renaming would touch every call site.
#[allow(clippy::module_inception)]
pub mod ollama;

pub use ollama::*;
// Don't re-export commands to avoid conflicts - lib.rs will import directly
