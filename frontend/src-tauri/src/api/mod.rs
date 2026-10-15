// Named after its parent directory by convention; renaming would touch every call site.
#[allow(clippy::module_inception)]
pub mod api;
pub mod commands;

pub use api::*;
// Don't re-export commands to avoid conflicts - lib.rs will import directly
