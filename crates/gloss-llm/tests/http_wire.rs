//! 第一层（L1）线级集成测试：本地传输协议（HTTP）桩服务器驱动真实传输客户端，
//! 经方言中立的线级入口（transport::send_with_retry）过完整传输面——分块
//! 传输编码、错误信封经真实状态码与响应体、瞬态失败在线上的自动重试、连接
//! 复用与“无总超时”的流式口径。桩只在本进程回环地址上听，不碰外网；端点
//! 解析（含明文拒绝）与密钥解析归配置层单测，本文件用回环地址直连线级入口。

use std::future::poll_fn;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use gloss_core::model::GlossError;
use gloss_core::ports::{EngineRequest, TaskChunk, TaskStream, ThinkingMode};
use gloss_core::prompt::ChatMessage;
use gloss_llm::transport::send_with_retry;

const STUB_KEY: &str = "gloss-stub-fixture-key";

const SSE_PAYLOAD: &str = concat!(
    "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n",
    "data: {\"choices\":[{\"delta\":{\"content\":\"光\"}}]}\n\n",
    "data: {\"choices\":[{\"delta\":{\"content\":\"泽\"}}]}\n\n",
    "data: [DONE]\n\n",
);

#[derive(Clone)]
struct StubRequest {
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl StubRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

type Handler = Arc<dyn Fn(&StubRequest, &mut TcpStream) + Send + Sync>;

struct StubServer {
    addr: SocketAddr,
    requests: Arc<AtomicUsize>,
    connections: Arc<AtomicUsize>,
}

impl StubServer {
    #[allow(clippy::expect_used)]
    fn spawn(handler: Handler) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback bind");
        let addr = listener.local_addr().expect("listener address");
        let requests = Arc::new(AtomicUsize::new(0));
        let connections = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&requests);
        let conn_counter = Arc::clone(&connections);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                conn_counter.fetch_add(1, Ordering::SeqCst);
                let handler = Arc::clone(&handler);
                let counter = Arc::clone(&counter);
                std::thread::spawn(move || serve(stream, handler, counter));
            }
        });
        Self {
            addr,
            requests,
            connections,
        }
    }

    fn endpoint(&self) -> String {
        format!("http://{}/v1/chat/completions", self.addr)
    }
}

fn serve(mut stream: TcpStream, handler: Handler, requests: Arc<AtomicUsize>) {
    loop {
        let Ok(Some(request)) = read_request(&mut stream) else {
            return;
        };
        requests.fetch_add(1, Ordering::SeqCst);
        handler(&request, &mut stream);
    }
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<Option<StubRequest>> {
    let mut buf = Vec::new();
    let head_end = loop {
        if let Some(pos) = buf.windows(4).position(|window| window == b"\r\n\r\n") {
            break pos;
        }
        let mut chunk = [0u8; 4096];
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return if buf.is_empty() {
                Ok(None)
            } else {
                Err(std::io::Error::new(
                    ErrorKind::UnexpectedEof,
                    "partial request head",
                ))
            };
        }
        buf.extend_from_slice(&chunk[..read]);
    };
    let (path, headers) = parse_head(&buf[..head_end])?;
    let content_length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < content_length {
        let mut chunk = [0u8; 4096];
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(std::io::Error::new(
                ErrorKind::UnexpectedEof,
                "partial body",
            ));
        }
        body.extend_from_slice(&chunk[..read]);
    }
    Ok(Some(StubRequest {
        path,
        headers,
        body,
    }))
}

fn parse_head(head: &[u8]) -> std::io::Result<(String, Vec<(String, String)>)> {
    let head = std::str::from_utf8(head)
        .map_err(|_| std::io::Error::new(ErrorKind::InvalidData, "non-utf8 request head"))?;
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let Some(path) = request_line.split(' ').nth(1) else {
        return Err(std::io::Error::new(
            ErrorKind::InvalidData,
            "bad request line",
        ));
    };
    let mut headers = Vec::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
    }
    Ok((path.to_owned(), headers))
}

fn chunk_frame(payload: &[u8]) -> Vec<u8> {
    let mut out = format!("{:x}\r\n", payload.len()).into_bytes();
    out.extend_from_slice(payload);
    out.extend_from_slice(b"\r\n");
    out
}

fn chunked_frame(payload: &[u8], chunk_len: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in payload.chunks(chunk_len.max(1)) {
        out.extend_from_slice(format!("{:x}\r\n", chunk.len()).as_bytes());
        out.extend_from_slice(chunk);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"0\r\n\r\n");
    out
}

fn stream_response(payload: &str, chunk_len: usize) -> Vec<u8> {
    let mut out =
        b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n"
            .to_vec();
    out.extend_from_slice(&chunked_frame(payload.as_bytes(), chunk_len));
    out
}

fn error_response(
    status: &str,
    content_type: &str,
    retry_after: Option<&str>,
    body: &str,
) -> Vec<u8> {
    let mut head = format!("HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\n");
    if let Some(retry_after) = retry_after {
        head.push_str(&format!("retry-after: {retry_after}\r\n"));
    }
    head.push_str(&format!("content-length: {}\r\n\r\n", body.len()));
    let mut out = head.into_bytes();
    out.extend_from_slice(body.as_bytes());
    out
}

#[allow(clippy::expect_used)]
fn stub_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("test http client builds")
}

fn request(model: &str) -> EngineRequest {
    EngineRequest {
        messages: vec![
            ChatMessage::system("线级桩系统提示"),
            ChatMessage::user("线级桩用户文本"),
        ],
        model: model.to_owned(),
        thinking: ThinkingMode::Follow,
    }
}

#[allow(clippy::expect_used)]
async fn send(client: &reqwest::Client, endpoint: &str, request: &EngineRequest) -> TaskStream {
    send_with_retry(client, endpoint, STUB_KEY, request)
        .await
        .expect("stream established over the stub")
}

struct StreamOutcome {
    text: String,
    finish_reason: Option<String>,
    error: Option<GlossError>,
}

async fn drain(stream: TaskStream) -> StreamOutcome {
    let mut stream = stream;
    let mut outcome = StreamOutcome {
        text: String::new(),
        finish_reason: None,
        error: None,
    };
    while let Some(item) = poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
        match item {
            Ok(TaskChunk::Delta(delta)) => outcome.text.push_str(&delta),
            Ok(TaskChunk::Done { finish_reason }) => outcome.finish_reason = finish_reason,
            Err(error) => {
                outcome.error = Some(error);
                return outcome;
            }
        }
    }
    outcome
}

#[allow(clippy::expect_used)]
fn record_request(
    sink: &Arc<Mutex<Option<StubRequest>>>,
    request: &StubRequest,
    stream: &mut TcpStream,
    response: &[u8],
) {
    *sink.lock().expect("stub sink mutex") = Some(request.clone());
    stream
        .write_all(response)
        .expect("stub response write succeeds");
}

#[tokio::test]
async fn chunked_response_streams_deltas_across_transfer_chunks() {
    let sink = Arc::new(Mutex::new(None));
    let handler_sink = Arc::clone(&sink);
    let server = StubServer::spawn(Arc::new(move |request, stream| {
        record_request(
            &handler_sink,
            request,
            stream,
            &stream_response(SSE_PAYLOAD, 1),
        );
    }));
    let client = stub_client();
    let outcome = drain(send(&client, &server.endpoint(), &request("stub-model")).await).await;
    assert_eq!(outcome.error, None, "chunked streaming must not fail");
    assert_eq!(outcome.text, "光泽");
    assert_eq!(outcome.finish_reason, None);
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);

    let recorded = sink
        .lock()
        .expect("stub sink mutex")
        .clone()
        .expect("request recorded");
    assert_eq!(recorded.path, "/v1/chat/completions");
    assert_eq!(recorded.header("content-type"), Some("application/json"));
    assert!(
        recorded
            .header("authorization")
            .is_some_and(|value| value.starts_with("Bearer ")),
        "the key rides the authorization header, never the body"
    );
    let wire: serde_json::Value =
        serde_json::from_slice(&recorded.body).expect("wire body is a JSON object");
    assert_eq!(wire["model"], "stub-model");
    assert_eq!(wire["stream"], true, "streaming is requested on the wire");
}

#[tokio::test]
async fn json_error_envelope_maps_real_status_and_body_to_the_error() {
    let cases: [(&str, &str, &str, &str); 2] = [
        (
            "400 Bad Request",
            "application/json",
            r#"{"error":{"message":"model `stub` does not exist","type":"invalid_request_error"}}"#,
            "HTTP 400 Bad Request: model `stub` does not exist",
        ),
        (
            "400 Bad Request",
            "text/html",
            "<html>nope</html>",
            "HTTP 400 Bad Request",
        ),
    ];
    for (status, content_type, body, expected) in cases {
        let server = StubServer::spawn(Arc::new(move |_, stream| {
            let response = error_response(status, content_type, None, body);
            stream
                .write_all(&response)
                .expect("stub response write succeeds");
        }));
        let client = stub_client();
        let error = match send_with_retry(
            &client,
            &server.endpoint(),
            STUB_KEY,
            &request("stub-model"),
        )
        .await
        {
            Err(error) => error,
            Ok(_) => panic!("a 4xx status fails the request before any stream exists"),
        };
        assert_eq!(
            error,
            GlossError::EngineResponse(expected.to_owned()),
            "the real status and body land in the error: {body}"
        );
        assert_eq!(
            server.requests.load(Ordering::SeqCst),
            1,
            "a non-transient status never retries"
        );
    }
}

#[tokio::test]
async fn rate_limited_response_with_retry_after_retries_before_first_byte() {
    let served = Arc::new(AtomicUsize::new(0));
    let handler_served = Arc::clone(&served);
    let server = StubServer::spawn(Arc::new(move |_, stream| {
        let response = if handler_served.fetch_add(1, Ordering::SeqCst) == 0 {
            error_response(
                "429 Too Many Requests",
                "application/json",
                Some("0"),
                r#"{"error":{"message":"slow down","type":"rate_limit_error"}}"#,
            )
        } else {
            stream_response(SSE_PAYLOAD, 64)
        };
        stream
            .write_all(&response)
            .expect("stub response write succeeds");
    }));
    let client = stub_client();
    let outcome = drain(send(&client, &server.endpoint(), &request("stub-model")).await).await;
    assert_eq!(outcome.error, None, "the retried attempt lands");
    assert_eq!(outcome.text, "光泽");
    assert_eq!(served.load(Ordering::SeqCst), 2, "one retry after the 429");
    assert_eq!(server.requests.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn sequential_requests_reuse_one_keep_alive_connection() {
    let server = StubServer::spawn(Arc::new(|_, stream| {
        stream
            .write_all(&stream_response(SSE_PAYLOAD, 512))
            .expect("stub response write succeeds");
    }));
    let client = stub_client();
    let first = drain(send(&client, &server.endpoint(), &request("stub-model")).await).await;
    let second = drain(send(&client, &server.endpoint(), &request("stub-model")).await).await;
    for outcome in [first, second] {
        assert_eq!(outcome.error, None);
        assert_eq!(outcome.text, "光泽");
    }
    assert_eq!(
        server.connections.load(Ordering::SeqCst),
        1,
        "the pooled connection serves both requests"
    );
    assert_eq!(server.requests.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn slow_deltas_keep_streaming_without_a_total_timeout() {
    let delta = |content: &str| {
        format!("data: {{\"choices\":[{{\"delta\":{{\"content\":\"{content}\"}}}}]}}\n\n")
    };
    let server = StubServer::spawn(Arc::new(move |_, stream| {
        stream
            .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n")
            .and_then(|_| stream.write_all(&chunk_frame(delta("慢").as_bytes())))
            .expect("stub first chunk write succeeds");
        std::thread::sleep(Duration::from_millis(1000));
        stream
            .write_all(&chunk_frame(delta("流").as_bytes()))
            .expect("stub second chunk write succeeds");
        std::thread::sleep(Duration::from_millis(1000));
        stream
            .write_all(&chunked_frame(b"data: [DONE]\n\n", 64))
            .expect("stub tail write succeeds");
    }));
    let client = stub_client();
    let outcome = drain(send(&client, &server.endpoint(), &request("stub-model")).await).await;
    assert_eq!(
        outcome.error, None,
        "a slow but alive stream is not killed by a total timeout"
    );
    assert_eq!(outcome.text, "慢流");
}
