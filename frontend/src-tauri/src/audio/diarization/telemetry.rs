//! Diarization telemetry: progress events to the frontend and the peak-memory
//! sampler used to report a run's footprint.

use super::batch::pcm::join_within;
use super::DiarizationProgress;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};
use tauri::{AppHandle, Emitter, Runtime};

pub(crate) fn emit_progress<R: Runtime>(
    app: &AppHandle<R>,
    meeting_id: &str,
    status: &str,
    progress: u32,
    message: &str,
) {
    let _ = app.emit(
        "diarization-progress",
        DiarizationProgress {
            meeting_id: meeting_id.to_string(),
            status: status.to_string(),
            progress,
            message: message.to_string(),
        },
    );
}

// ===== Memory sampler =====

pub(crate) struct MemorySampler {
    peak_bytes: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for MemorySampler {
    fn drop(&mut self) {
        // A generous fixed timeout: `Drop` is a safety net, not the primary
        // stop path (see `finish`), so it still waits for the sampler thread
        // rather than leaking it, but bounded instead of an untimed join.
        self.stop(Duration::from_secs(1));
    }
}

impl MemorySampler {
    pub(crate) fn start() -> Self {
        let peak_bytes = Arc::new(AtomicU64::new(0));
        let running = Arc::new(AtomicBool::new(true));
        let peak_bytes_clone = peak_bytes.clone();
        let running_clone = running.clone();

        let handle = std::thread::spawn(move || {
            let pid = match sysinfo::get_current_pid() {
                Ok(pid) => pid,
                Err(_) => return,
            };
            let mut system = System::new_with_specifics(
                RefreshKind::new().with_processes(ProcessRefreshKind::new().with_memory()),
            );
            let refresh_kind = ProcessRefreshKind::new().with_memory();
            while running_clone.load(Ordering::Relaxed) {
                let _ = system.refresh_processes_specifics(
                    ProcessesToUpdate::Some(&[pid]),
                    false,
                    refresh_kind,
                );
                if let Some(proc) = system.process(pid) {
                    let mem = proc.memory();
                    let mut current = peak_bytes_clone.load(Ordering::Relaxed);
                    while mem > current {
                        match peak_bytes_clone.compare_exchange_weak(
                            current,
                            mem,
                            Ordering::Relaxed,
                            Ordering::Relaxed,
                        ) {
                            Ok(_) => break,
                            Err(c) => current = c,
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        });

        Self {
            peak_bytes,
            running,
            handle: Some(handle),
        }
    }

    /// Signal the sampler thread to stop and wait up to `timeout` for it to
    /// exit. Returns whether it joined within the timeout.
    fn stop(&mut self, timeout: Duration) -> bool {
        self.running.store(false, Ordering::Relaxed);
        join_within(&mut self.handle, timeout)
    }

    /// Stop the sampler (bounded by a generous fixed timeout) and return the
    /// peak resident memory observed, in MiB. The primary, explicit stop path
    /// (`Drop` remains a safety net for the case this is not called).
    pub(crate) fn finish(mut self) -> u64 {
        self.stop(Duration::from_secs(1));
        self.peak_bytes.load(Ordering::Relaxed) / 1024 / 1024
    }
}

#[cfg(test)]
mod stop_signal_tests {
    use std::io::Read;
    use std::time::Instant;

    use super::super::batch::pcm::drain_with_stop;
    use super::*;

    /// A `Read` that never reaches EOF and sleeps a bit on every call before
    /// returning a few bytes — simulating a pipe with no data ready yet, so a
    /// loop reading it would otherwise never stop on its own. Exercises the
    /// same generic seam (`drain_with_stop<R: Read>`) `PcmStream`'s real
    /// stderr-reader thread is built on; `PcmStream` itself cannot be
    /// constructed in a unit test without a real OS child process (it owns a
    /// `std::process::Child`/`ChildStdout`).
    struct TricklingReader {
        delay: Duration,
    }

    impl Read for TricklingReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            std::thread::sleep(self.delay);
            let n = buf.len().min(4);
            for b in buf.iter_mut().take(n) {
                *b = 0;
            }
            Ok(n)
        }
    }

    #[test]
    fn drain_with_stop_exits_a_would_otherwise_block_forever_reader_within_the_timeout() {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);
        let handle = std::thread::spawn(move || {
            let _ = drain_with_stop(
                TricklingReader {
                    delay: Duration::from_millis(10),
                },
                &stop_clone,
            );
        });
        let mut handle_opt = Some(handle);

        // Give the reader thread a couple of iterations before asking it to
        // stop, then prove `join_within` (the same helper `PcmStream::stop`
        // and `MemorySampler::stop` use) observes the stop within its bound.
        std::thread::sleep(Duration::from_millis(30));
        stop.store(true, Ordering::Relaxed);

        let joined = join_within(&mut handle_opt, Duration::from_millis(500));
        assert!(joined, "reader thread did not stop within the timeout");
    }

    #[test]
    fn memory_sampler_stop_joins_promptly() {
        let mut sampler = MemorySampler::start();
        let start = Instant::now();
        let joined = sampler.stop(Duration::from_secs(2));
        let elapsed = start.elapsed();
        assert!(joined, "MemorySampler did not stop within the timeout");
        // The sampler polls every 500ms; a prompt stop should not need to
        // wait for anywhere near that, let alone several iterations of it.
        assert!(
            elapsed < Duration::from_millis(500),
            "stop() took {:?}, expected a prompt join",
            elapsed
        );
    }
}
