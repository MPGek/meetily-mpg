//! Exact-size, resumable multi-file transfer (design D1), used by Parakeet and
//! word alignment.
//!
//! A local file is skipped only at exactly its catalogued size. A shorter file
//! is resumed with `Range`, and every `206`/`200`/`416` is validated before a
//! byte is written. Partial bytes are flushed and kept on cancel, stall or
//! error so a later download resumes them.

use super::owners::{DownloadCancelled, DownloadOwner};
use anyhow::{anyhow, Result};
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::time::timeout;

/// One downloadable file with its exact byte size.
#[derive(Debug, Clone, Copy)]
pub struct ArtifactSpec {
    /// File name relative to the source base URL.
    pub remote: &'static str,
    /// File name relative to the local model directory.
    pub local: &'static str,
    pub exact_bytes: u64,
}

impl ArtifactSpec {
    /// An artifact whose remote and local names coincide.
    pub const fn same(name: &'static str, exact_bytes: u64) -> Self {
        Self {
            remote: name,
            local: name,
            exact_bytes,
        }
    }
}

/// Progress over all artifacts of one transfer, in confirmed bytes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransferProgress {
    pub confirmed_bytes: u64,
    pub total_bytes: u64,
    pub speed_mbps: f64,
    /// 0-100; capped at 99 while the transfer is in flight.
    pub percent: u8,
}

impl TransferProgress {
    fn percent_of(confirmed_bytes: u64, total_bytes: u64) -> u8 {
        if total_bytes > 0 {
            ((confirmed_bytes as f64 / total_bytes as f64) * 100.0).min(100.0) as u8
        } else {
            0
        }
    }

    fn in_flight(confirmed_bytes: u64, total_bytes: u64, speed_mbps: f64) -> Self {
        Self {
            confirmed_bytes,
            total_bytes,
            speed_mbps,
            percent: Self::percent_of(confirmed_bytes, total_bytes).min(99),
        }
    }

    fn finished(total_bytes: u64, speed_mbps: f64) -> Self {
        Self {
            confirmed_bytes: total_bytes,
            total_bytes,
            speed_mbps,
            percent: Self::percent_of(total_bytes, total_bytes),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ContentRange {
    Range { start: u64, end: u64, total: u64 },
    Unsatisfied { total: u64 },
}

pub fn parse_content_range(value: &reqwest::header::HeaderValue) -> Result<ContentRange> {
    let value = value
        .to_str()
        .map_err(|e| anyhow!("Invalid Content-Range header encoding: {}", e))?;
    let value = value
        .strip_prefix("bytes ")
        .ok_or_else(|| anyhow!("Content-Range must use bytes: {}", value))?;

    if let Some(total) = value.strip_prefix("*/") {
        return total
            .parse()
            .map(|total| ContentRange::Unsatisfied { total })
            .map_err(|e| anyhow!("Invalid unsatisfied Content-Range total: {}", e));
    }

    let (range, total) = value
        .split_once('/')
        .ok_or_else(|| anyhow!("Malformed Content-Range: {}", value))?;
    let (start, end) = range
        .split_once('-')
        .ok_or_else(|| anyhow!("Malformed Content-Range range: {}", value))?;
    let start = start
        .parse()
        .map_err(|e| anyhow!("Invalid Content-Range start: {}", e))?;
    let end = end
        .parse()
        .map_err(|e| anyhow!("Invalid Content-Range end: {}", e))?;
    let total = total
        .parse()
        .map_err(|e| anyhow!("Invalid Content-Range total: {}", e))?;
    if start > end {
        return Err(anyhow!("Content-Range start exceeds end: {}", value));
    }

    Ok(ContentRange::Range { start, end, total })
}

fn declared_content_length(response: &reqwest::Response) -> Result<Option<u64>> {
    response
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .map(|value| {
            value
                .to_str()
                .map_err(|error| anyhow!("Invalid Content-Length header encoding: {}", error))?
                .parse()
                .map_err(|error| anyhow!("Invalid Content-Length header: {}", error))
        })
        .transpose()
}

pub fn validate_full_response(response: &reqwest::Response, exact_bytes: u64) -> Result<()> {
    if response.status() != reqwest::StatusCode::OK {
        return Err(anyhow!(
            "Expected full 200 response, received {}",
            response.status()
        ));
    }
    if let Some(content_length) = declared_content_length(response)? {
        if content_length != exact_bytes {
            return Err(anyhow!(
                "Full response declared {} bytes, expected {}",
                content_length,
                exact_bytes
            ));
        }
    }
    Ok(())
}

pub fn validate_partial_response(
    response: &reqwest::Response,
    expected_start: u64,
    exact_bytes: u64,
) -> Result<()> {
    if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(anyhow!(
            "Expected partial 206 response, received {}",
            response.status()
        ));
    }
    let content_range = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .ok_or_else(|| anyhow!("Partial response is missing Content-Range"))?;
    let ContentRange::Range { start, end, total } = parse_content_range(content_range)? else {
        return Err(anyhow!("Partial response has an unsatisfied Content-Range"));
    };
    if start != expected_start || end != exact_bytes - 1 || total != exact_bytes {
        return Err(anyhow!(
            "Partial response range {}-{} / {} does not match {}-{} / {}",
            start,
            end,
            total,
            expected_start,
            exact_bytes - 1,
            exact_bytes
        ));
    }
    let expected_length = end
        .checked_sub(start)
        .and_then(|length| length.checked_add(1))
        .ok_or_else(|| anyhow!("Partial response range length overflow"))?;
    if let Some(content_length) = declared_content_length(response)? {
        if content_length != expected_length {
            return Err(anyhow!(
                "Partial response declared {} bytes, expected {}",
                content_length,
                expected_length
            ));
        }
    }
    Ok(())
}

pub fn validate_unsatisfied_response(response: &reqwest::Response, exact_bytes: u64) -> Result<()> {
    if response.status() != reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        return Err(anyhow!(
            "Expected range-not-satisfiable 416 response, received {}",
            response.status()
        ));
    }
    let content_range = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .ok_or_else(|| anyhow!("416 response is missing Content-Range"))?;
    match parse_content_range(content_range)? {
        ContentRange::Unsatisfied { total } if total == exact_bytes => Ok(()),
        ContentRange::Unsatisfied { total } => Err(anyhow!(
            "416 response reports {} total bytes, expected {}",
            total,
            exact_bytes
        )),
        ContentRange::Range { .. } => Err(anyhow!("416 response has a satisfied Content-Range")),
    }
}

async fn send_download_request(
    client: &reqwest::Client,
    file_url: &str,
    range_start: Option<u64>,
    owner: &DownloadOwner,
) -> Result<reqwest::Response> {
    let mut request = client.get(file_url);
    if let Some(range_start) = range_start {
        log::info!("Requesting {} (Range: bytes={}-)", file_url, range_start);
        request = request.header(reqwest::header::RANGE, format!("bytes={range_start}-"));
    } else {
        log::info!("Requesting {}", file_url);
    }

    tokio::select! {
        biased;
        _ = owner.cancellation().cancelled() => Err(DownloadCancelled.into()),
        response = request.send() => response
            .map_err(|error| anyhow!("Failed to start download for {}: {}", file_url, error)),
    }
}

/// User-facing prefix for a stream error, kept from the fork's previous
/// Parakeet downloader.
fn stream_error_kind(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "Connection timeout - Check your internet"
    } else if error.is_connect() {
        "Connection failed - Check your internet"
    } else if error.is_body() {
        "Stream interrupted - Network unstable"
    } else {
        "Download error"
    }
}

/// Download every artifact into `model_dir`, resuming partial files.
///
/// Returns the final 100% progress on success. In-flight reports through
/// `on_progress` are capped at 99%, and the same percent is stored on the
/// owner. The caller publishes 100% only after it commits the model.
pub async fn download_artifacts(
    client: &reqwest::Client,
    base_url: &str,
    model_dir: &Path,
    artifacts: &[ArtifactSpec],
    owner: &DownloadOwner,
    on_progress: &mut (dyn FnMut(TransferProgress) + Send),
) -> Result<TransferProgress> {
    if owner.cancellation().is_cancelled() {
        return Err(DownloadCancelled.into());
    }
    fs::create_dir_all(model_dir)
        .await
        .map_err(|error| anyhow!("Failed to create model directory: {}", error))?;

    let total_bytes: u64 = artifacts.iter().map(|artifact| artifact.exact_bytes).sum();
    let download_started = Instant::now();
    let mut confirmed_bytes = 0u64;
    let mut streamed_bytes = 0u64;
    let mut bytes_since_report = 0u64;
    let mut last_report = Instant::now();
    let mut last_percent = 0u8;

    let mut report = |progress: TransferProgress| {
        owner.set_progress(progress.percent);
        on_progress(progress);
    };

    for artifact in artifacts {
        if owner.cancellation().is_cancelled() {
            return Err(DownloadCancelled.into());
        }

        let file_path = model_dir.join(artifact.local);
        let local_bytes = match fs::metadata(&file_path).await {
            Ok(metadata) => metadata.len(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(error) => {
                return Err(anyhow!(
                    "Failed to read {} metadata: {}",
                    artifact.local,
                    error
                ));
            }
        };

        if local_bytes == artifact.exact_bytes {
            log::info!(
                "Skipping complete file: {} ({} bytes)",
                artifact.local,
                artifact.exact_bytes
            );
            confirmed_bytes = confirmed_bytes
                .checked_add(artifact.exact_bytes)
                .ok_or_else(|| anyhow!("Progress overflow while skipping {}", artifact.local))?;
            let progress = TransferProgress::in_flight(confirmed_bytes, total_bytes, 0.0);
            report(progress);
            last_percent = progress.percent;
            last_report = Instant::now();
            continue;
        }

        let file_url = format!("{}/{}", base_url.trim_end_matches('/'), artifact.remote);
        let range_start =
            (local_bytes > 0 && local_bytes < artifact.exact_bytes).then_some(local_bytes);
        if local_bytes > artifact.exact_bytes {
            log::warn!(
                "{} has {} bytes, more than its exact {} bytes; fetching it again",
                artifact.local,
                local_bytes,
                artifact.exact_bytes
            );
        }
        let response = send_download_request(client, &file_url, range_start, owner).await?;

        let (response, mut artifact_bytes, append) = match range_start {
            Some(range_start) => match response.status() {
                reqwest::StatusCode::PARTIAL_CONTENT => {
                    validate_partial_response(&response, range_start, artifact.exact_bytes)?;
                    log::info!("Resuming {} from byte {}", artifact.local, range_start);
                    confirmed_bytes = confirmed_bytes.checked_add(range_start).ok_or_else(|| {
                        anyhow!("Progress overflow while resuming {}", artifact.local)
                    })?;
                    let progress = TransferProgress::in_flight(confirmed_bytes, total_bytes, 0.0);
                    report(progress);
                    last_percent = progress.percent;
                    last_report = Instant::now();
                    (response, range_start, true)
                }
                reqwest::StatusCode::OK => {
                    validate_full_response(&response, artifact.exact_bytes)?;
                    log::warn!(
                        "Server ignored Range for {}; replacing the partial file",
                        artifact.local
                    );
                    (response, 0, false)
                }
                reqwest::StatusCode::RANGE_NOT_SATISFIABLE => {
                    validate_unsatisfied_response(&response, artifact.exact_bytes)?;
                    log::warn!(
                        "Server rejected Range for {} (416); fetching it fresh",
                        artifact.local
                    );
                    let retry = send_download_request(client, &file_url, None, owner).await?;
                    validate_full_response(&retry, artifact.exact_bytes)?;
                    (retry, 0, false)
                }
                status => {
                    return Err(anyhow!(
                        "Download failed for {} with status {}",
                        artifact.local,
                        status
                    ));
                }
            },
            None => match response.status() {
                reqwest::StatusCode::OK => {
                    validate_full_response(&response, artifact.exact_bytes)?;
                    (response, 0, false)
                }
                reqwest::StatusCode::PARTIAL_CONTENT => {
                    validate_partial_response(&response, 0, artifact.exact_bytes)?;
                    (response, 0, false)
                }
                status => {
                    return Err(anyhow!(
                        "Download failed for {} with status {}",
                        artifact.local,
                        status
                    ));
                }
            },
        };

        if owner.cancellation().is_cancelled() {
            return Err(DownloadCancelled.into());
        }
        let file = if append {
            fs::OpenOptions::new()
                .append(true)
                .open(&file_path)
                .await
                .map_err(|error| anyhow!("Failed to open {} for resume: {}", artifact.local, error))?
        } else {
            fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&file_path)
                .await
                .map_err(|error| anyhow!("Failed to replace {}: {}", artifact.local, error))?
        };
        let mut writer = BufWriter::with_capacity(8 * 1024 * 1024, file);
        use futures_util::StreamExt;
        let mut stream = response.bytes_stream();

        loop {
            let next_chunk = tokio::select! {
                biased;
                _ = owner.cancellation().cancelled() => {
                    writer.flush().await.map_err(|error| {
                        anyhow!("Failed to preserve {} during cancellation: {}", artifact.local, error)
                    })?;
                    log::info!("Download cancelled; kept partial {}", artifact.local);
                    return Err(DownloadCancelled.into());
                }
                chunk = timeout(Duration::from_secs(30), stream.next()) => chunk,
            };
            let chunk = match next_chunk {
                Err(_) => {
                    writer.flush().await.map_err(|error| {
                        anyhow!("Failed to preserve {} after timeout: {}", artifact.local, error)
                    })?;
                    return Err(anyhow!(
                        "Download timeout for {}: no data received for 30 seconds",
                        artifact.local
                    ));
                }
                Ok(None) => break,
                Ok(Some(Err(error))) => {
                    writer.flush().await.map_err(|flush_error| {
                        anyhow!(
                            "Failed to preserve {} after stream error: {}",
                            artifact.local,
                            flush_error
                        )
                    })?;
                    return Err(anyhow!(
                        "{} for {}: {}",
                        stream_error_kind(&error),
                        artifact.local,
                        error
                    ));
                }
                Ok(Some(Ok(chunk))) => chunk,
            };

            let chunk_bytes = chunk.len() as u64;
            let next_artifact_bytes = artifact_bytes
                .checked_add(chunk_bytes)
                .ok_or_else(|| anyhow!("{} size overflow", artifact.local))?;
            if next_artifact_bytes > artifact.exact_bytes {
                writer.flush().await.map_err(|error| {
                    anyhow!(
                        "Failed to preserve {} after overlong response: {}",
                        artifact.local,
                        error
                    )
                })?;
                return Err(anyhow!(
                    "{} response exceeds its exact {} byte size",
                    artifact.local,
                    artifact.exact_bytes
                ));
            }
            let next_confirmed_bytes = confirmed_bytes
                .checked_add(chunk_bytes)
                .ok_or_else(|| anyhow!("Progress overflow while downloading {}", artifact.local))?;
            if next_confirmed_bytes > total_bytes {
                return Err(anyhow!("Download progress exceeds the catalog total"));
            }

            writer
                .write_all(&chunk)
                .await
                .map_err(|error| anyhow!("Failed to write {}: {}", artifact.local, error))?;
            artifact_bytes = next_artifact_bytes;
            confirmed_bytes = next_confirmed_bytes;
            streamed_bytes = streamed_bytes
                .checked_add(chunk_bytes)
                .ok_or_else(|| anyhow!("Streamed byte count overflow"))?;
            bytes_since_report = bytes_since_report
                .checked_add(chunk_bytes)
                .ok_or_else(|| anyhow!("Progress byte count overflow"))?;

            let percent = TransferProgress::in_flight(confirmed_bytes, total_bytes, 0.0).percent;
            let elapsed = last_report.elapsed();
            if percent > last_percent
                || elapsed >= Duration::from_millis(500)
                || artifact_bytes == artifact.exact_bytes
            {
                let speed_mbps = if elapsed.as_secs_f64() > 0.0 {
                    bytes_since_report as f64 / (1024.0 * 1024.0) / elapsed.as_secs_f64()
                } else {
                    0.0
                };
                report(TransferProgress::in_flight(
                    confirmed_bytes,
                    total_bytes,
                    speed_mbps,
                ));
                last_percent = percent;
                last_report = Instant::now();
                bytes_since_report = 0;
            }
        }

        writer
            .flush()
            .await
            .map_err(|error| anyhow!("Failed to flush {}: {}", artifact.local, error))?;
        drop(writer);

        if owner.cancellation().is_cancelled() {
            return Err(DownloadCancelled.into());
        }
        let stored_bytes = fs::metadata(&file_path)
            .await
            .map_err(|error| anyhow!("Failed to read {} after download: {}", artifact.local, error))?
            .len();
        if stored_bytes != artifact.exact_bytes {
            return Err(anyhow!(
                "{} stored {} bytes, expected exactly {} bytes",
                artifact.local,
                stored_bytes,
                artifact.exact_bytes
            ));
        }
        log::info!("Completed {} ({} bytes)", artifact.local, stored_bytes);
    }

    if confirmed_bytes != total_bytes {
        return Err(anyhow!(
            "Download confirmed {} bytes, expected {} bytes",
            confirmed_bytes,
            total_bytes
        ));
    }
    let elapsed = download_started.elapsed().as_secs_f64();
    let speed_mbps = if elapsed > 0.0 {
        streamed_bytes as f64 / (1024.0 * 1024.0) / elapsed
    } else {
        0.0
    };
    Ok(TransferProgress::finished(total_bytes, speed_mbps))
}

/// Loopback HTTP server for offline download tests (upstream v0.4.1
/// `parakeet_engine.rs:1257-1342`), shared by every engine's tests.
#[cfg(test)]
pub(crate) mod test_server {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    pub(crate) struct ExpectedResponse {
        pub(crate) filename: &'static str,
        pub(crate) range: Option<&'static str>,
        pub(crate) status: &'static str,
        pub(crate) content_length: Option<u64>,
        pub(crate) content_range: Option<&'static str>,
        pub(crate) body: &'static [u8],
        pub(crate) release_after_body: Option<oneshot::Receiver<()>>,
    }

    pub(crate) fn response(
        filename: &'static str,
        range: Option<&'static str>,
        status: &'static str,
        body: &'static [u8],
        content_range: Option<&'static str>,
    ) -> ExpectedResponse {
        ExpectedResponse {
            filename,
            range,
            status,
            content_length: Some(body.len() as u64),
            content_range,
            body,
            release_after_body: None,
        }
    }

    pub(crate) async fn read_request(socket: &mut tokio::net::TcpStream) -> String {
        let mut request = Vec::new();
        let mut buffer = [0u8; 1024];
        loop {
            let read = socket.read(&mut buffer).await.expect("read request");
            assert_ne!(read, 0, "request ended before its headers");
            request.extend_from_slice(&buffer[..read]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                return String::from_utf8(request).expect("request is valid UTF-8");
            }
        }
    }

    pub(crate) async fn serve_requests(
        expected_responses: Vec<ExpectedResponse>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback test server");
        let address = listener.local_addr().expect("get loopback address");
        let server = tokio::spawn(async move {
            for expected in expected_responses {
                let (mut socket, _) = listener.accept().await.expect("accept test request");
                let request = read_request(&mut socket).await;
                assert!(
                    request.starts_with(&format!("GET /{} HTTP/", expected.filename)),
                    "unexpected request path: {request}"
                );
                let requested_range = request.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("range").then_some(value.trim())
                });
                assert_eq!(requested_range, expected.range);

                let mut headers =
                    format!("HTTP/1.1 {}\r\nConnection: close\r\n", expected.status);
                if let Some(content_length) = expected.content_length {
                    headers.push_str(&format!("Content-Length: {content_length}\r\n"));
                }
                if let Some(content_range) = expected.content_range {
                    headers.push_str(&format!("Content-Range: {content_range}\r\n"));
                }
                headers.push_str("\r\n");
                socket
                    .write_all(headers.as_bytes())
                    .await
                    .expect("write response headers");
                socket
                    .write_all(expected.body)
                    .await
                    .expect("write response body");
                if let Some(release_after_body) = expected.release_after_body {
                    release_after_body.await.expect("release partial response");
                }
            }
        });

        (format!("http://{address}"), server)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    #[test]
    fn content_range_parses_range_and_unsatisfied_forms() {
        assert_eq!(
            parse_content_range(&HeaderValue::from_static("bytes 1-2/3")).unwrap(),
            ContentRange::Range {
                start: 1,
                end: 2,
                total: 3
            }
        );
        assert_eq!(
            parse_content_range(&HeaderValue::from_static("bytes 0-0/1")).unwrap(),
            ContentRange::Range {
                start: 0,
                end: 0,
                total: 1
            }
        );
        assert_eq!(
            parse_content_range(&HeaderValue::from_static("bytes */652183999")).unwrap(),
            ContentRange::Unsatisfied { total: 652_183_999 }
        );
    }

    #[test]
    fn content_range_rejects_malformed_and_inverted() {
        for value in [
            "bytes invalid",
            "items 0-1/2",
            "bytes 0-1",
            "bytes 0/2",
            "bytes a-1/2",
            "bytes 0-1/x",
            "bytes */x",
            "bytes 5-4/10",
        ] {
            assert!(
                parse_content_range(&HeaderValue::from_static(value)).is_err(),
                "{value} must be rejected"
            );
        }
    }
}
