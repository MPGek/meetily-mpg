//! One live diarization session at a time.

use std::sync::atomic::{AtomicBool, Ordering};

static ONLINE_DIARIZATION_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Guards against two online diarization sessions running at once.
pub struct OnlineDiarizationGuard;

impl OnlineDiarizationGuard {
    pub(crate) fn acquire() -> Result<Self, String> {
        if ONLINE_DIARIZATION_ACTIVE
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("Online diarization already active".to_string());
        }
        Ok(OnlineDiarizationGuard)
    }
}

impl Drop for OnlineDiarizationGuard {
    fn drop(&mut self) {
        ONLINE_DIARIZATION_ACTIVE.store(false, Ordering::SeqCst);
    }
}

pub fn is_online_diarization_active() -> bool {
    ONLINE_DIARIZATION_ACTIVE.load(Ordering::SeqCst)
}
