//! Bounded live-alignment queue + re-emit consumer (tasks 5.2–5.4, design D4).
//!
//! When the transcription worker produces a **final** result carrying tokens,
//! it hands the block's in-memory samples + the emitted update to this queue
//! (never blocks on inference) and emits the transcript-update exactly as
//! before. A dedicated consumer refines tokens on the shared session pool and
//! re-emits a transcript-update for the same `sequence_id` — the existing
//! listener upsert persists the refinement. Partial results are never queued.
//! Overflow drops the **oldest** pending block (that block keeps ASR tokens —
//! a spec fallback mode). At stop the queue is closed and drained with a
//! per-block timeout before finalize.

use super::engine::AlignmentEngine;
use super::refine::SEGMENT_ALIGN_TIMEOUT;
use super::settings;
use crate::audio::transcription::TranscriptUpdate;
use crate::audio::word_alignment::engine::align_tokens_with_timeout;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Runtime};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

/// Queue capacity in bytes of held audio (~80 max-size 25 s blocks). Tunable
/// per design "Open Questions"; 128 MB is the initial bound.
pub const QUEUE_CAPACITY_BYTES: usize = 128 * 1024 * 1024;

/// One completed block awaiting refinement.
pub struct AlignmentJob {
    /// The transcript-update already emitted with ASR-timed tokens.
    pub update: TranscriptUpdate,
    /// The block's own 16 kHz mono samples (chunk-relative).
    pub samples: Arc<[f32]>,
}

impl AlignmentJob {
    fn bytes(&self) -> usize {
        self.samples.len() * std::mem::size_of::<f32>()
    }
}

struct QueueState {
    jobs: VecDeque<AlignmentJob>,
    bytes: usize,
    closed: bool,
}

/// Bounded ownership queue with drop-oldest overflow.
pub struct AlignmentQueue {
    state: Mutex<QueueState>,
    notify: Notify,
    capacity: usize,
    dropped: AtomicUsize,
}

impl AlignmentQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            state: Mutex::new(QueueState {
                jobs: VecDeque::new(),
                bytes: 0,
                closed: false,
            }),
            notify: Notify::new(),
            capacity,
            dropped: AtomicUsize::new(0),
        }
    }

    /// Enqueue a block. Never blocks. Returns the number of oldest blocks
    /// dropped to stay within capacity (0 normally). A closed queue refuses
    /// the new block (returns 1).
    pub fn push(&self, job: AlignmentJob) -> usize {
        let mut dropped = 0usize;
        {
            let mut st = self.state.lock().unwrap();
            if st.closed {
                return 1;
            }
            let job_bytes = job.bytes();
            st.jobs.push_back(job);
            st.bytes += job_bytes;
            // Overflow: drop oldest until within capacity (keep at least the
            // just-pushed block).
            while st.bytes > self.capacity && st.jobs.len() > 1 {
                if let Some(old) = st.jobs.pop_front() {
                    st.bytes -= old.bytes();
                    dropped += 1;
                } else {
                    break;
                }
            }
        }
        if dropped > 0 {
            let total = self.dropped.fetch_add(dropped, Ordering::SeqCst) + dropped;
            log::warn!(
                "Alignment queue overflow: dropped {} oldest block(s) (total {}); they keep ASR tokens",
                dropped,
                total
            );
        }
        self.notify.notify_one();
        dropped
    }

    /// Close the queue: no more pushes; pop drains then returns None.
    pub fn close(&self) {
        self.state.lock().unwrap().closed = true;
        self.notify.notify_waiters();
    }

    pub fn is_closed(&self) -> bool {
        self.state.lock().unwrap().closed
    }

    /// Next job, or None when closed and drained.
    pub async fn pop(&self) -> Option<AlignmentJob> {
        loop {
            {
                let mut st = self.state.lock().unwrap();
                if let Some(job) = st.jobs.pop_front() {
                    st.bytes -= job.bytes();
                    return Some(job);
                }
                if st.closed {
                    return None;
                }
            }
            self.notify.notified().await;
        }
    }

    /// Pending block count (for tests / drain logging).
    pub fn len(&self) -> usize {
        self.state.lock().unwrap().jobs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Spawn the consumer task that refines queued blocks and re-emits refined
/// transcript-updates for the same `sequence_id`.
pub fn spawn_consumer<R: Runtime>(
    app: AppHandle<R>,
    queue: Arc<AlignmentQueue>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut active_engine: Option<Arc<AlignmentEngine>> = None;
        while let Some(job) = queue.pop().await {
            let AlignmentJob { mut update, samples } = job;

            // Resolve the engine lazily off the async thread (the first load
            // parses a ~650 MB ONNX graph); re-check so a mid-recording
            // download is picked up, and drop (keep baseline) when disabled.
            if active_engine.is_none() {
                active_engine =
                    tokio::task::spawn_blocking(|| settings::current().engine()).await.ok().flatten();
            }
            let Some(engine) = active_engine.clone() else {
                continue;
            };

            let Some(tokens) = update.tokens.as_mut() else {
                continue;
            };
            if tokens.is_empty() || tokens.iter().all(|t| t.refined) {
                continue;
            }

            let ok = align_tokens_with_timeout(
                engine,
                tokens,
                samples.to_vec(),
                update.audio_start_time as f32,
                update.audio_end_time as f32,
                SEGMENT_ALIGN_TIMEOUT,
            );
            if ok {
                // Re-emit for the same sequence_id; the listener upserts the
                // buffered segment with refined timestamps (text unchanged).
                if let Err(e) = app.emit("transcript-update", &update) {
                    log::warn!("Failed to re-emit refined transcript-update: {}", e);
                }
            }
        }
        log::info!("Alignment queue consumer finished (queue closed and drained)");
    })
}

/// Close the queue and wait for the consumer to drain in-flight blocks, with
/// an overall bound so shutdown never hangs. Anything left unrefined is
/// handled by the finalize repair hook.
pub async fn drain_on_stop(
    queue: &Arc<AlignmentQueue>,
    consumer: JoinHandle<()>,
    overall_timeout: std::time::Duration,
) {
    queue.close();
    let pending = queue.len();
    if pending > 0 {
        log::info!("Draining {} in-flight alignment block(s) before finalize", pending);
    }
    match tokio::time::timeout(overall_timeout, consumer).await {
        Ok(Ok(())) => log::info!("✅ Alignment queue drained before finalize"),
        Ok(Err(e)) => log::warn!("Alignment consumer ended abnormally: {:?}", e),
        Err(_) => {
            log::warn!(
                "Alignment drain timed out after {:?}; finalize will repair leftovers",
                overall_timeout
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::token_assignment::Token;

    fn job(seq: u64, samples_len: usize) -> AlignmentJob {
        AlignmentJob {
            update: TranscriptUpdate {
                text: format!("seg {}", seq),
                timestamp: "[00:00]".to_string(),
                source: "Audio".to_string(),
                sequence_id: seq,
                chunk_start_time: 0.0,
                is_partial: false,
                confidence: 0.9,
                audio_start_time: 0.0,
                audio_end_time: 1.0,
                duration: 1.0,
                source_device: "Microphone".to_string(),
                speaker: None,
                tokens: Some(vec![Token {
                    text: "hi".to_string(),
                    start: 0.0,
                    end: 0.5,
                    refined: false,
                }]),
            },
            samples: Arc::from(vec![0.0f32; samples_len]),
        }
    }

    #[tokio::test]
    async fn push_pop_preserves_order() {
        let q = AlignmentQueue::new(QUEUE_CAPACITY_BYTES);
        q.push(job(1, 100));
        q.push(job(2, 100));
        assert_eq!(q.pop().await.unwrap().update.sequence_id, 1);
        assert_eq!(q.pop().await.unwrap().update.sequence_id, 2);
    }

    #[tokio::test]
    async fn overflow_drops_oldest_not_newest() {
        // Capacity for ~2 blocks of 100 samples (400 bytes each).
        let q = AlignmentQueue::new(800);
        assert_eq!(q.push(job(1, 100)), 0);
        assert_eq!(q.push(job(2, 100)), 0);
        // Third block exceeds capacity -> oldest (seq 1) dropped.
        let dropped = q.push(job(3, 100));
        assert_eq!(dropped, 1);
        // Remaining queue is seq 2 then seq 3 (newest kept).
        assert_eq!(q.pop().await.unwrap().update.sequence_id, 2);
        assert_eq!(q.pop().await.unwrap().update.sequence_id, 3);
    }

    #[tokio::test]
    async fn push_after_close_is_refused() {
        let q = AlignmentQueue::new(QUEUE_CAPACITY_BYTES);
        q.push(job(1, 100));
        q.close();
        // Existing item still drains.
        assert_eq!(q.pop().await.unwrap().update.sequence_id, 1);
        // Then None (closed + empty).
        assert!(q.pop().await.is_none());
        // New push refused.
        assert_eq!(q.push(job(2, 100)), 1);
    }

    #[tokio::test]
    async fn drain_completes_after_close() {
        let q = Arc::new(AlignmentQueue::new(QUEUE_CAPACITY_BYTES));
        q.push(job(1, 100));
        q.push(job(2, 100));
        q.close();
        // Manual drain: pop until None.
        let mut seen = Vec::new();
        while let Some(j) = q.pop().await {
            seen.push(j.update.sequence_id);
        }
        assert_eq!(seen, vec![1, 2]);
    }
}
