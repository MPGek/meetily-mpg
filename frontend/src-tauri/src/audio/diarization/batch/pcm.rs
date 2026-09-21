//! Streaming PCM decode for the batch pass: ffmpeg spawned to emit 16 kHz mono
//! f32le on stdout, plus the overlapping in-memory window reader the v2 core
//! consumes.

use std::io::Read;
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::super::DIARIZATION_SAMPLE_RATE;

// ===== ffmpeg streaming decode =====

/// A spawned ffmpeg process streaming 16 kHz mono f32le PCM on stdout.
/// Poll a background thread's `JoinHandle` for up to `timeout`, joining it if
/// it finishes in time. Returns whether it joined within the timeout, leaving
/// the handle in place (still `Some`) if it did not, so a caller can retry or
/// fall back to an untimed join. `std::thread::JoinHandle` has no native
/// timed join, so this polls `is_finished()` on a short sleep interval.
/// Shared by the ffmpeg stderr reader (`PcmStream`) and `MemorySampler`, the
/// two background threads this change gives an explicit, bounded-time stop.
pub(crate) fn join_within(handle: &mut Option<std::thread::JoinHandle<()>>, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        match handle.as_ref() {
            None => return true,
            Some(h) if h.is_finished() => break,
            Some(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Some(_) => return false,
        }
    }
    if let Some(h) = handle.take() {
        let _ = h.join();
    }
    true
}

/// Drain `reader` into a buffer, checking `stop` between bounded reads so the
/// loop can be asked to stop instead of only ending at EOF (`Ok(0)`) or a read
/// error. Generic over `Read` so it can be driven by a test double as well as
/// a real ffmpeg `ChildStderr` pipe.
pub(crate) fn drain_with_stop<R: Read>(mut reader: R, stop: &AtomicBool) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        match reader.read(&mut chunk) {
            Ok(0) => break, // pipe closed (ffmpeg exited)
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(_) => break,
        }
    }
    buf
}

pub(crate) struct PcmStream {
    child: Child,
    pub(crate) stdout: ChildStdout,
    stderr: Arc<Mutex<Vec<u8>>>,
    stderr_stop: Arc<AtomicBool>,
    stderr_handle: Option<std::thread::JoinHandle<()>>,
}

impl PcmStream {
    /// Wait for the process to exit and return an error if it failed.
    pub(crate) fn finish(mut self) -> Result<(), String> {
        let status = self
            .child
            .wait()
            .map_err(|e| format!("Failed to wait for ffmpeg: {}", e))?;
        if !status.success() {
            let stderr = self.stderr.lock().unwrap();
            return Err(format!(
                "ffmpeg exited with {}: {}",
                status,
                String::from_utf8_lossy(&stderr)
            ));
        }
        Ok(())
    }

    /// Kill the process (used on cancellation) and wait, bounded, for the
    /// stderr-reader thread to drain and exit — killing the child closes its
    /// stderr pipe, so the reader observes EOF promptly, but this makes the
    /// wait explicit and bounded instead of leaving the thread to exit on
    /// its own time with no one watching.
    pub(crate) fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if !self.stop(Duration::from_millis(500)) {
            log::warn!("ffmpeg stderr-reader thread did not stop within 500ms of kill()");
        }
    }

    /// Signal the stderr-reader thread to stop and wait up to `timeout` for
    /// it to exit. Returns whether it joined within the timeout.
    fn stop(&mut self, timeout: Duration) -> bool {
        self.stderr_stop.store(true, Ordering::Relaxed);
        join_within(&mut self.stderr_handle, timeout)
    }
}

impl Drop for PcmStream {
    fn drop(&mut self) {
        let still_running = self.child.try_wait().map(|o| o.is_none()).unwrap_or(false);
        if still_running {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// Read up to `count` little-endian f32 samples from `reader`, appending them
/// to `out`. Returns the number of samples appended (0 means EOF).
fn read_f32_le(reader: &mut impl Read, out: &mut Vec<f32>, count: usize) -> Result<usize, String> {
    let start = out.len();
    let mut byte_buf = [0u8; 16384];
    while out.len() - start < count {
        let remaining = count - (out.len() - start);
        let max_bytes = (remaining * 4).min(byte_buf.len());
        let n = reader
            .read(&mut byte_buf[..max_bytes])
            .map_err(|e| format!("Failed to read PCM stream: {}", e))?;
        if n == 0 {
            break;
        }
        for b in byte_buf[..n].chunks_exact(4) {
            out.push(f32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        }
    }
    Ok(out.len() - start)
}

/// Spawn ffmpeg to decode `input_path` to 16 kHz mono f32le PCM on stdout.
/// `channel: Some(0)` selects the left channel, `Some(1)` the right channel,
/// and `None` downmixes to mono.
pub(crate) fn spawn_ffmpeg_pcm(
    ffmpeg_path: &Path,
    input_path: &Path,
    channel: Option<u32>,
) -> Result<PcmStream, String> {
    let input_str = input_path
        .to_str()
        .ok_or_else(|| "Invalid audio path (non-UTF8)".to_string())?;

    let mut cmd = Command::new(ffmpeg_path);
    cmd.args(["-hide_banner", "-nostats", "-loglevel", "error"])
        .arg("-i")
        .arg(input_str)
        .arg("-vn");
    match channel {
        Some(0) => {
            cmd.args(["-af", "pan=mono|c0=c0"]);
        }
        Some(1) => {
            cmd.args(["-af", "pan=mono|c0=c1"]);
        }
        Some(_) => {
            return Err("Invalid channel index for ffmpeg streaming".to_string());
        }
        None => {
            cmd.args(["-ac", "1"]);
        }
    }
    cmd.args(["-ar", "16000", "-f", "f32le", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn ffmpeg: {}", e))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "ffmpeg stdout was not captured".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "ffmpeg stderr was not captured".to_string())?;

    // Drain stderr in a background thread so the pipe cannot fill and deadlock
    // the ffmpeg process. The thread checks `stderr_stop` between bounded
    // reads so it can be asked to stop before the pipe closes.
    let stderr_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let stderr_buf_clone = Arc::clone(&stderr_buf);
    let stderr_stop: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let stderr_stop_clone = Arc::clone(&stderr_stop);
    let stderr_handle = std::thread::spawn(move || {
        let buf = drain_with_stop(stderr, &stderr_stop_clone);
        *stderr_buf_clone.lock().unwrap() = buf;
    });

    Ok(PcmStream {
        child,
        stdout,
        stderr: stderr_buf,
        stderr_stop,
        stderr_handle: Some(stderr_handle),
    })
}

/// Yields overlapping in-memory windows of 16 kHz f32 PCM read from a stream.
/// Windows overlap by `overlap_samples`, and the final (partial) window ends at
/// the stream's end. Only the trailing overlap is retained between calls, so
/// peak memory stays at one window plus the overlap carry.
pub(crate) struct StreamWindows {
    chunk_samples: usize,
    overlap_samples: usize,
    step_samples: usize,
    carry: Vec<f32>,
    start_seconds: f32,
    first: bool,
    done: bool,
}

impl StreamWindows {
    pub(crate) fn new(chunk_samples: usize, overlap_samples: usize) -> Self {
        Self {
            chunk_samples,
            overlap_samples,
            step_samples: chunk_samples.saturating_sub(overlap_samples).max(1),
            carry: Vec::new(),
            start_seconds: 0.0,
            first: true,
            done: false,
        }
    }

    pub(crate) fn next_from(&mut self, reader: &mut impl Read) -> Result<Option<(f32, Vec<f32>)>, String> {
        if self.done {
            return Ok(None);
        }

        let mut window;
        if self.first {
            window = Vec::with_capacity(self.chunk_samples);
            read_f32_le(reader, &mut window, self.chunk_samples)?;
            self.first = false;
        } else {
            window = std::mem::take(&mut self.carry);
            let before = window.len();
            read_f32_le(reader, &mut window, self.step_samples)?;
            self.start_seconds += self.step_samples as f32 / DIARIZATION_SAMPLE_RATE as f32;
            if window.len() == before {
                self.done = true;
                return Ok(None);
            }
        }

        if window.is_empty() {
            self.done = true;
            return Ok(None);
        }

        // Retain the trailing overlap for the next window.
        let carry_start = window.len().saturating_sub(self.overlap_samples);
        self.carry = window[carry_start..].to_vec();

        if window.len() < self.chunk_samples {
            self.done = true;
        }
        Ok(Some((self.start_seconds, window)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_f32_le_converts_little_endian_samples() {
        let mut data: Vec<u8> = Vec::new();
        for v in [1.0f32, -2.5, 0.0, 3.25] {
            data.extend_from_slice(&v.to_le_bytes());
        }
        let mut cursor = std::io::Cursor::new(data);
        let mut out = Vec::new();
        let n = read_f32_le(&mut cursor, &mut out, 10).expect("read");
        assert_eq!(n, 4);
        assert_eq!(out, vec![1.0, -2.5, 0.0, 3.25]);
    }

    #[test]
    fn read_f32_le_stops_at_eof() {
        let data = 0.5f32.to_le_bytes().to_vec();
        let mut cursor = std::io::Cursor::new(data);
        let mut out = Vec::new();
        let n = read_f32_le(&mut cursor, &mut out, 10).expect("read");
        assert_eq!(n, 1);
        assert_eq!(out, vec![0.5]);
        let n2 = read_f32_le(&mut cursor, &mut out, 10).expect("read");
        assert_eq!(n2, 0);
    }

    #[test]
    fn stream_windows_overlap_and_advance() {
        // 30 seconds of 16 kHz mono f32 = 480000 samples (value == sample index).
        let total = 480_000usize;
        let mut data = Vec::with_capacity(total * 4);
        for i in 0..total {
            data.extend_from_slice(&(i as f32).to_le_bytes());
        }
        let mut cursor = std::io::Cursor::new(data);

        let mut windows = StreamWindows::new(160_000, 80_000);
        let mut collected: Vec<(f32, Vec<f32>)> = Vec::new();
        while let Some(w) = windows.next_from(&mut cursor).expect("read") {
            collected.push(w);
        }

        // 10s chunks with 5s overlap -> 5s step over 30s: 5 full windows.
        assert_eq!(collected.len(), 5);
        let starts: Vec<f32> = collected.iter().map(|(s, _)| *s).collect();
        assert_eq!(starts, vec![0.0, 5.0, 10.0, 15.0, 20.0]);
        assert!(collected.iter().all(|(_, s)| s.len() == 160_000));
        // The second window begins in the overlap region, i.e. at the tail of
        // the first window (sample value == global sample index).
        assert_eq!(collected[0].1[80_000], 80_000.0);
        assert_eq!(collected[1].1[0], 80_000.0);
    }

    #[test]
    fn stream_windows_final_partial_window() {
        // 22 seconds = 352000 samples, not a multiple of the 5s step.
        let total = 352_000usize;
        let mut data = Vec::with_capacity(total * 4);
        for _ in 0..total {
            data.extend_from_slice(&0.0f32.to_le_bytes());
        }
        let mut cursor = std::io::Cursor::new(data);

        let mut windows = StreamWindows::new(160_000, 80_000);
        let mut collected = Vec::new();
        while let Some(w) = windows.next_from(&mut cursor).expect("read") {
            collected.push(w);
        }

        assert_eq!(collected.len(), 4);
        assert_eq!(collected[3].0, 15.0);
        assert_eq!(collected[3].1.len(), 112_000);
    }
}
