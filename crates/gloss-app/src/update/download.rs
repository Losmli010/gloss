//! 整包下载：流式落盘、响应体上限、size/sha256 校验与原子改名落位。
//!
//! 只信任清单给的三件事：https 直链、字节数、sha256——传输层不提供签名。
//! 先按累计字节数挡超长（写入前即拒），再比对最终 size，最后算 sha256；
//! 先 size 后 hash 是因为前者是流式常量时间，hash 要整读一遍文件。
//! 续传仅限进程内重试：206 才续、200 一律从头；size 偏小与取消保留
//! `.partial` 供续传，超长与 sha 不符则丢弃重下，校验通过才原子改名。

use std::path::{Path, PathBuf};

use futures::StreamExt;
use gloss_core::log::{debug, thread};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use super::manifest::Artifact;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadError {
    Network(String),
    TooLarge,
    SizeMismatch { expected: u64, actual: u64 },
    ShaMismatch,
    Io(String),
    Cancelled,
}

pub async fn download(
    client: &reqwest::Client,
    artifact: &Artifact,
    work_dir: &Path,
    resume: bool,
    cancel: &CancellationToken,
) -> Result<PathBuf, DownloadError> {
    let name = artifact_file_name(&artifact.url)?;
    tokio::fs::create_dir_all(work_dir)
        .await
        .map_err(|err| DownloadError::Io(format!("create work dir: {err}")))?;
    let partial = work_dir.join(format!("{name}.partial"));
    let dest = work_dir.join(name);

    let start = plan_resume_start(artifact, &partial, resume, cancel).await?;
    let response = request_partial(artifact, client, start).await?;
    let resume_accepted = response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    let offset = if resume_accepted { start } else { 0 };
    let mut writer = open_partial(&partial, offset, resume_accepted).await?;
    match stream_body(&mut writer, response, artifact, offset, cancel).await {
        Ok(()) => {}
        Err(DownloadError::TooLarge) => {
            if let Err(err) = tokio::fs::remove_file(&partial).await {
                debug!(thread = thread::TOKIO, error = %err, "update: oversize partial cleanup failed");
            }
            return Err(DownloadError::TooLarge);
        }
        Err(err) => return Err(err),
    }
    flush(&mut writer).await?;
    drop(writer);

    verify_and_rename(&partial, &dest, artifact).await
}

async fn plan_resume_start(
    artifact: &Artifact,
    partial: &Path,
    resume: bool,
    cancel: &CancellationToken,
) -> Result<u64, DownloadError> {
    let mut start = 0u64;
    if resume {
        if let Ok(meta) = tokio::fs::metadata(partial).await {
            let len = meta.len();
            if len > 0 && len < artifact.size {
                start = len;
            }
        }
    } else if let Err(err) = tokio::fs::remove_file(partial).await {
        debug!(thread = thread::TOKIO, error = %err, "update: stale partial cleanup failed");
    }
    if cancel.is_cancelled() {
        return Err(DownloadError::Cancelled);
    }
    Ok(start)
}

async fn request_partial(
    artifact: &Artifact,
    client: &reqwest::Client,
    start: u64,
) -> Result<reqwest::Response, DownloadError> {
    let mut request = client.get(&artifact.url);
    if start > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={start}-"));
    }
    let response = request
        .send()
        .await
        .map_err(|err| DownloadError::Network(format!("request: {err}")))?;
    let status = response.status();
    if !(status == reqwest::StatusCode::PARTIAL_CONTENT || status == reqwest::StatusCode::OK) {
        return Err(DownloadError::Network(format!(
            "unexpected status {status}"
        )));
    }
    if status == reqwest::StatusCode::PARTIAL_CONTENT {
        let content_range = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        if !content_range.starts_with(&format!("bytes {start}-")) {
            return Err(DownloadError::Network(format!(
                "content-range mismatch: asked from {start}, got {content_range}"
            )));
        }
    }
    Ok(response)
}

async fn open_partial(
    partial: &Path,
    offset: u64,
    resume_accepted: bool,
) -> Result<tokio::io::BufWriter<tokio::fs::File>, DownloadError> {
    let file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(!resume_accepted)
        .open(partial)
        .await
        .map_err(|err| DownloadError::Io(format!("open partial: {err}")))?;
    file.set_len(offset)
        .await
        .map_err(|err| DownloadError::Io(format!("truncate partial: {err}")))?;
    let mut writer = tokio::io::BufWriter::new(file);
    writer
        .seek(std::io::SeekFrom::Start(offset))
        .await
        .map_err(|err| DownloadError::Io(format!("seek partial: {err}")))?;
    Ok(writer)
}

async fn stream_body(
    writer: &mut tokio::io::BufWriter<tokio::fs::File>,
    response: reqwest::Response,
    artifact: &Artifact,
    offset: u64,
    cancel: &CancellationToken,
) -> Result<(), DownloadError> {
    let mut total = offset;
    let mut stream = response.bytes_stream();
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                flush(writer).await?;
                return Err(DownloadError::Cancelled);
            }
            chunk = stream.next() => match chunk {
                None => break,
                Some(Err(err)) => {
                    flush(writer).await?;
                    return Err(DownloadError::Network(format!("stream: {err}")));
                }
                Some(Ok(bytes)) => {
                    total += bytes.len() as u64;
                    if total > artifact.size {
                        return Err(DownloadError::TooLarge);
                    }
                    writer
                        .write_all(&bytes)
                        .await
                        .map_err(|err| DownloadError::Io(format!("write partial: {err}")))?;
                }
            }
        }
    }
    Ok(())
}

async fn flush(writer: &mut tokio::io::BufWriter<tokio::fs::File>) -> Result<(), DownloadError> {
    writer
        .flush()
        .await
        .map_err(|err| DownloadError::Io(format!("flush partial: {err}")))
}

async fn verify_and_rename(
    partial: &Path,
    dest: &Path,
    artifact: &Artifact,
) -> Result<PathBuf, DownloadError> {
    let actual = tokio::fs::metadata(partial)
        .await
        .map_err(|err| DownloadError::Io(format!("stat partial: {err}")))?
        .len();
    if actual != artifact.size {
        return Err(DownloadError::SizeMismatch {
            expected: artifact.size,
            actual,
        });
    }
    let sha = sha256_hex_file(partial).await?;
    if sha != artifact.sha256 {
        if let Err(err) = tokio::fs::remove_file(partial).await {
            debug!(thread = thread::TOKIO, error = %err, "update: mismatched partial cleanup failed");
        }
        return Err(DownloadError::ShaMismatch);
    }
    tokio::fs::rename(partial, dest)
        .await
        .map_err(|err| DownloadError::Io(format!("rename partial: {err}")))?;
    Ok(dest.to_path_buf())
}

fn artifact_file_name(url: &str) -> Result<String, DownloadError> {
    let name = reqwest::Url::parse(url)
        .ok()
        .and_then(|parsed| {
            parsed
                .path_segments()
                .and_then(|mut segments| segments.next_back())
                .map(str::to_owned)
        })
        .filter(|name| !name.is_empty() && name != "." && name != "..")
        .ok_or_else(|| DownloadError::Network(format!("bad artifact url: {url}")))?;
    Ok(name)
}

async fn sha256_hex_file(path: &Path) -> Result<String, DownloadError> {
    use sha2::Digest;
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|err| DownloadError::Io(format!("open for hash: {err}")))?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|err| DownloadError::Io(format!("read for hash: {err}")))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = hasher.finalize();
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::PathBuf;

    use sha2::Digest;

    use super::*;

    const BODY_LEN: usize = 64 * 1024 + 123;

    fn test_body() -> Vec<u8> {
        (0..BODY_LEN).map(|i| (i % 251) as u8).collect()
    }

    fn sha_hex(bytes: &[u8]) -> String {
        let mut hasher = sha2::Sha256::new();
        hasher.update(bytes);
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder().build().expect("test client")
    }

    fn artifact_of(url: &str, size: u64, sha256: &str) -> Artifact {
        Artifact {
            url: url.to_owned(),
            size,
            sha256: sha256.to_owned(),
        }
    }

    #[derive(Clone, Copy)]
    enum ServerBehavior {
        Full,
        RangeAware,
        IgnoreRange,
        TruncateAfter(usize),
    }

    fn spawn_server(behavior: ServerBehavior, body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let range = read_request_range(&mut stream);
                let full = body.len() as u64;
                match behavior {
                    ServerBehavior::Full | ServerBehavior::IgnoreRange => {
                        respond(&mut stream, "200 OK", None, &body);
                    }
                    ServerBehavior::RangeAware => match range {
                        Some(start) => {
                            let head = format!(
                                "206 Partial Content\r\nContent-Range: bytes {start}-{end}/{full}\r\n",
                                end = full - 1,
                            );
                            respond(&mut stream, &head, Some(start as usize), &body);
                        }
                        None => respond(&mut stream, "200 OK", None, &body),
                    },
                    ServerBehavior::TruncateAfter(k) => {
                        respond(&mut stream, "200 OK", None, &body[..k]);
                        break;
                    }
                }
            }
        });
        format!("http://{addr}/gloss-test.zip")
    }

    fn read_request_range(stream: &mut TcpStream) -> Option<u64> {
        let mut line = Vec::new();
        loop {
            let mut byte = [0u8; 1];
            match stream.read(&mut byte) {
                Ok(0) | Err(_) => return None,
                Ok(_) => {
                    line.push(byte[0]);
                    if line.ends_with(b"\r\n") {
                        let text = String::from_utf8_lossy(&line).to_string();
                        line.clear();
                        if text == "\r\n" {
                            return None;
                        }
                        if let Some(rest) = text.strip_prefix("Range: bytes=") {
                            let start = rest.trim_end().trim_end_matches('-');
                            return start.parse().ok();
                        }
                    }
                }
            }
        }
    }

    fn respond(stream: &mut TcpStream, status_head: &str, slice_from: Option<usize>, body: &[u8]) {
        let payload = slice_from.map_or(body, |from| &body[from..]);
        let head = format!(
            "HTTP/1.1 {status_head}Content-Length: {}\r\nConnection: close\r\n\r\n",
            payload.len(),
        );
        drop(stream.write_all(head.as_bytes()));
        drop(stream.write_all(payload));
        drop(stream.flush());
        drop(stream.shutdown(std::net::Shutdown::Write));
    }

    async fn run_download(
        url: &str,
        size: u64,
        sha256: &str,
        work_dir: &Path,
        resume: bool,
    ) -> Result<PathBuf, DownloadError> {
        let cancel = CancellationToken::new();
        download(
            &client(),
            &artifact_of(url, size, sha256),
            work_dir,
            resume,
            &cancel,
        )
        .await
    }

    #[tokio::test]
    async fn full_download_verifies_and_lands_at_dest() {
        let body = test_body();
        let url = spawn_server(ServerBehavior::Full, body.clone());
        let work = temp_dir("full-ok");

        let dest = run_download(&url, body.len() as u64, &sha_hex(&body), &work, false)
            .await
            .expect("download must verify");
        assert_eq!(dest, work.join("gloss-test.zip"));
        assert_eq!(std::fs::read(&dest).expect("dest"), body);
        assert!(!work.join("gloss-test.zip.partial").exists());
    }

    #[tokio::test]
    async fn truncated_stream_keeps_partial_for_resume() {
        let body = test_body();
        let keep = body.len() / 3;
        let url = spawn_server(ServerBehavior::TruncateAfter(keep), body.clone());
        let work = temp_dir("truncated");

        let error = run_download(&url, body.len() as u64, &sha_hex(&body), &work, false)
            .await
            .expect_err("truncated stream must fail");
        assert!(
            matches!(
                error,
                DownloadError::SizeMismatch { .. } | DownloadError::Network(_)
            ),
            "got {error:?}"
        );

        let partial = work.join("gloss-test.zip.partial");
        assert_eq!(
            std::fs::metadata(&partial).expect("partial kept").len() as usize,
            keep,
            "in-flight progress must survive for the retry"
        );
        assert!(!work.join("gloss-test.zip").exists());
    }

    #[tokio::test]
    async fn resume_from_partial_completes_and_verifies() {
        let body = test_body();
        let keep = body.len() / 2;
        let url = spawn_server(ServerBehavior::RangeAware, body.clone());
        let work = temp_dir("resume-ok");
        std::fs::write(work.join("gloss-test.zip.partial"), &body[..keep]).expect("seed partial");

        let dest = run_download(&url, body.len() as u64, &sha_hex(&body), &work, true)
            .await
            .expect("resumed download must verify");
        assert_eq!(std::fs::read(&dest).expect("dest"), body);
        assert!(!work.join("gloss-test.zip.partial").exists());
    }

    #[tokio::test]
    async fn server_without_range_support_restarts_from_scratch() {
        let body = test_body();
        let keep = body.len() / 2;
        let url = spawn_server(ServerBehavior::IgnoreRange, body.clone());
        let work = temp_dir("no-range");
        std::fs::write(work.join("gloss-test.zip.partial"), &body[..keep]).expect("seed partial");

        let dest = run_download(&url, body.len() as u64, &sha_hex(&body), &work, true)
            .await
            .expect("non-range server must trigger a full restart");
        assert_eq!(std::fs::read(&dest).expect("dest"), body);
        assert!(!work.join("gloss-test.zip.partial").exists());
    }

    #[tokio::test]
    async fn sha_mismatch_discards_the_partial() {
        let body = test_body();
        let url = spawn_server(ServerBehavior::Full, body.clone());
        let work = temp_dir("sha-bad");
        let wrong_sha = sha_hex(b"different bytes");

        let error = run_download(&url, body.len() as u64, &wrong_sha, &work, false)
            .await
            .expect_err("sha mismatch must fail");
        assert_eq!(error, DownloadError::ShaMismatch);
        assert!(!work.join("gloss-test.zip.partial").exists());
        assert!(!work.join("gloss-test.zip").exists());
    }

    #[tokio::test]
    async fn oversize_response_is_rejected_and_discarded() {
        let body = test_body();
        let url = spawn_server(ServerBehavior::Full, body.clone());
        let work = temp_dir("oversize");

        let error = run_download(&url, (body.len() / 2) as u64, &sha_hex(&body), &work, false)
            .await
            .expect_err("oversize response must fail");
        assert_eq!(error, DownloadError::TooLarge);
        assert!(!work.join("gloss-test.zip.partial").exists());
    }

    #[tokio::test]
    async fn cancellation_returns_cancelled_without_dest() {
        let body = test_body();
        let url = spawn_server(ServerBehavior::Full, body.clone());
        let work = temp_dir("cancelled");
        let cancel = CancellationToken::new();
        cancel.cancel();

        let error = download(
            &client(),
            &artifact_of(&url, body.len() as u64, &sha_hex(&body)),
            &work,
            false,
            &cancel,
        )
        .await
        .expect_err("pre-cancelled download must not run");
        assert_eq!(error, DownloadError::Cancelled);
        assert!(!work.join("gloss-test.zip").exists());
    }

    #[tokio::test]
    async fn bad_artifact_url_is_rejected_before_any_request() {
        let work = temp_dir("bad-url");
        let error = run_download("https://pages.example/", 1, &sha_hex(b"x"), &work, false)
            .await
            .expect_err("segment-less url must fail early");
        assert!(matches!(error, DownloadError::Network(_)), "got {error:?}");
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gloss-download-test-{tag}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("work dir");
        dir
    }
}
