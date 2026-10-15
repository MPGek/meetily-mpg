//! The single-run guard for the offline pass: one diarization at a time,
//! plus the cancellation flag the stages poll.

use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) static DIARIZATION_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
pub(crate) static DIARIZATION_CANCELLED: AtomicBool = AtomicBool::new(false);

pub(crate) struct DiarizationGuard;

impl DiarizationGuard {
    pub(crate) fn acquire() -> Result<Self, String> {
        if DIARIZATION_IN_PROGRESS
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("Diarization already in progress".to_string());
        }
        Ok(DiarizationGuard)
    }
}

impl Drop for DiarizationGuard {
    fn drop(&mut self) {
        DIARIZATION_IN_PROGRESS.store(false, Ordering::SeqCst);
    }
}
