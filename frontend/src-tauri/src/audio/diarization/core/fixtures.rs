//! Shared test fixtures for the core stages: the smallest constructors for a
//! raw segmentation output and a dense embedding unit, so the segmentation,
//! turn-assembly and clustering test modules build their inputs the same way.

use super::super::RawSegment;
use super::segment::DenseUnit;

pub(crate) fn raw_seg(start: f64, end: f64, local: u8) -> RawSegment {
    RawSegment {
        time: polyvoice::types::TimeRange { start, end },
        local_speaker_idx: local,
        is_overlap: false,
        confidence: polyvoice::types::Confidence::new(0.9).unwrap_or_default(),
    }
}

pub(crate) fn test_unit(start: f64, end: f64, local: u8, segment_idx: usize) -> DenseUnit {
    DenseUnit {
        time: polyvoice::types::TimeRange { start, end },
        local_idx: local,
        segment_idx,
        embedding: vec![1.0, 0.0],
    }
}
