//! Shared diarization core: units, clustering, segmentation, turn assembly and
//! speaker attribution used by both the batch pass and a live session.

#[cfg(test)]
pub(crate) mod fixtures;
pub mod cluster;
pub mod factory;
pub mod segment;
pub mod source;
pub mod timeline;
pub mod turns;
pub mod units;
