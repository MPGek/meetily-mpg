// Poison-safe std::sync::Mutex locking for the recording/diarization audio engine.
use std::sync::{Mutex, MutexGuard};

pub trait LockRecover<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T>;
}

impl<T> LockRecover<T> for Mutex<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|poisoned| {
            log::error!(
                "Recovered a poisoned lock (a prior holder panicked); continuing with its last state"
            );
            poisoned.into_inner()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn lock_or_recover_recovers_after_a_panic_while_holding_the_guard() {
        let mutex = Arc::new(Mutex::new(0));

        let handle = {
            let mutex = Arc::clone(&mutex);
            std::thread::spawn(move || {
                let mut guard = mutex.lock().unwrap();
                *guard = 42;
                panic!("simulated panic while holding the lock");
            })
        };
        let _ = handle.join();

        let guard = mutex.lock_or_recover();
        assert_eq!(*guard, 42);
    }
}
