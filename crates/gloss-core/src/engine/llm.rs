//! `LlmClient`：`OpenAI` 兼容端点的流式引擎适配器（[`crate::ports::AiEngine`]
//! 端口的内建实现）。
//!
//! 边界：**只负责把请求送出去、把响应流回来**。`messages` 由编排层渲染
//! （`super::AiTaskService` → `prompt::PromptRegistry`，含模态校验），模型
//! 也已由 `App` 按配置解析后随 [`EngineRequest`] 携带——本模块不渲染
//! 提示词，也不看配置里的模型。域形态的消息（[`crate::prompt::ChatMessage`]：文本或多
//! 模态部件）在这里映成 `OpenAI` 兼容 `wire` 形状；图像部件的 `base64` 与
//! `data URL` 前缀也只在请求体构造处编码（域类型不带 `base64`）。
//!
//! 端点与密钥：端点取配置快照的 `base_url`；密钥按 `provider` 条目的 `keychain`
//! 条目标识**每请求直查**（不缓存，除请求头外不进任何地方——错误消息与日志里
//! 只有状态码与服务端诊断文本；红线与措辞见 `crate::ports::ConfigStore` 的文档）。
//!
//! 超时与重试（均为传输层内建行为，编排层不重试、原样上抛）：建连超时
//! 之外，流式读取只设**空闲超时**——长回复合法地跑满全预算，总超时只会
//! 误杀；传输失败在**首个流条目之前**自动重试（仅瞬态失败）。两者的取值
//! 与调度见 `transport`。取消由调用方的 `CancellationToken` 竞速完成
//! （`future` 被丢弃即断链）。

use std::sync::Arc;
use std::time::Duration;

use crate::config::Config;
use crate::config_handle::ConfigHandle;
use crate::log::debug;
use crate::model::GlossError;
use crate::ports::{AiEngine, BoxFuture, ConfigStore, EngineRequest, TaskStream};

use self::transport::{
    FirstByteStream, RETRY_ATTEMPTS, Resolved, TransportFail, connect_with_retry, send_once,
};

mod failure;
mod request_body;
mod transport;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

const CHAT_COMPLETIONS_PATH: &str = "chat/completions";

pub struct LlmClient {
    client: reqwest::Client,
    config: Arc<ConfigHandle>,
    store: Arc<dyn ConfigStore>,
}

impl LlmClient {
    pub fn new(config: Arc<ConfigHandle>, store: Arc<dyn ConfigStore>) -> Result<Self, GlossError> {
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .user_agent(concat!("gloss/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|err| GlossError::EngineResponse(format!("build http client: {err}")))?;
        Ok(Self {
            client,
            config,
            store,
        })
    }
}

impl AiEngine for LlmClient {
    fn execute(
        &self,
        request: EngineRequest,
    ) -> BoxFuture<'static, Result<TaskStream, GlossError>> {
        let client = self.client.clone();
        let config = Arc::clone(&self.config);
        let store = Arc::clone(&self.store);
        let EngineRequest {
            messages,
            model,
            max_tokens,
        } = request;

        Box::pin(async move {
            if messages.is_empty() {
                return Err(GlossError::EngineResponse("empty request".into()));
            }
            if model.trim().is_empty() {
                return Err(GlossError::Config("empty model id".into()));
            }
            let snapshot = config.snapshot();
            let resolved = Arc::new(Resolved::build(
                resolve_endpoint(&snapshot)?,
                resolve_api_key(&snapshot, store.as_ref())?,
                &EngineRequest {
                    messages,
                    model: model.trim().to_owned(),
                    max_tokens,
                },
            )?);
            let attempt = {
                let client = client.clone();
                let resolved = Arc::clone(&resolved);
                move || {
                    let client = client.clone();
                    let resolved = Arc::clone(&resolved);
                    Box::pin(async move { send_once(&client, &resolved).await })
                        as BoxFuture<'static, Result<TaskStream, TransportFail>>
                }
            };
            let (connected, retries_left) =
                connect_with_retry(attempt.clone(), RETRY_ATTEMPTS).await;
            let stream = connected?;
            debug!(
                model = %model,
                "llm stream established"
            );
            Ok(Box::pin(FirstByteStream::new(attempt, stream, retries_left)) as TaskStream)
        })
    }
}

fn resolve_endpoint(config: &Config) -> Result<String, GlossError> {
    let base_url = config.base_url.trim();
    crate::config::validate_base_url(base_url)
        .map_err(|err| GlossError::Config(err.to_string()))?;
    let mut endpoint = reqwest::Url::parse(base_url)
        .map_err(|err| GlossError::Config(format!("invalid provider endpoint: {err}")))?;
    let path = format!(
        "{}/{CHAT_COMPLETIONS_PATH}",
        endpoint.path().trim_end_matches('/')
    );
    endpoint.set_path(&path);
    Ok(endpoint.to_string())
}

fn resolve_api_key(config: &Config, store: &dyn ConfigStore) -> Result<String, GlossError> {
    let provider = config.resolved_provider();
    store
        .secret(&provider.keychain_id)?
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty())
        .ok_or(GlossError::EngineAuth)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::VecDeque;
    use std::pin::Pin;
    use std::sync::Mutex;
    use std::task::{Context, Poll};

    use super::failure::map_failure;
    use super::request_body::chat_request_body;
    use super::transport::{
        FirstByteStream, RETRY_AFTER_CAP, RETRY_ATTEMPTS, RETRY_BACKOFF, SseStream, TransportFail,
        connect_with_retry, parse_retry_after, retry_delay,
    };
    use crate::ports::TaskChunk;
    use crate::prompt::{ChatMessage, ContentPart};
    use crate::stubs::ports::MemoryConfigStore;
    use futures_core::Stream;
    use reqwest::StatusCode;
    use tokio::time::{Instant as TokioInstant, Sleep, sleep};

    const FIXTURE_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0B, 0x49, 0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0x60,
        0x00, 0x02, 0x00, 0x00, 0x05, 0x00, 0x01, 0xE9, 0xFA, 0xDC, 0xD8, 0x00, 0x00, 0x00, 0x00,
        0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    fn fixture(secret: Option<&str>) -> (Arc<ConfigHandle>, Arc<MemoryConfigStore>) {
        let store = Arc::new(MemoryConfigStore::default());
        if let Some(secret) = secret {
            store
                .set_secret("gloss/deepseek", secret)
                .expect("stub store accepts secret");
        }
        (
            Arc::new(ConfigHandle::with_config(store.clone(), Config::default())),
            store,
        )
    }

    #[test]
    fn resolves_endpoint_from_config() {
        let config = Config::default();
        assert_eq!(
            resolve_endpoint(&config).expect("resolve"),
            format!("{}/chat/completions", crate::config::DEFAULT_BASE_URL)
        );
    }

    #[test]
    fn endpoint_trimming_avoids_double_slash() {
        let config = Config {
            base_url: "https://example.test/v1/".into(),
            ..Default::default()
        };
        assert_eq!(
            resolve_endpoint(&config).expect("resolve"),
            "https://example.test/v1/chat/completions"
        );
    }

    #[test]
    fn cleartext_endpoints_are_rejected() {
        for base_url in [
            "http://api.example.test/v1",
            "http://localhost:11434/v1",
            "http://127.0.0.1:8080/v1",
        ] {
            let config = Config {
                base_url: base_url.into(),
                ..Default::default()
            };
            assert!(
                matches!(resolve_endpoint(&config), Err(GlossError::Config(_))),
                "cleartext endpoint must be rejected: {base_url}"
            );
        }
    }

    #[test]
    fn endpoints_with_credentials_query_or_fragment_are_rejected() {
        for base_url in [
            "https://user:pass@api.example.test/v1",
            "https://user@api.example.test/v1",
            "https://api.example.test/v1?key=x",
            "https://api.example.test/v1#frag",
        ] {
            let config = Config {
                base_url: base_url.into(),
                ..Default::default()
            };
            assert!(
                matches!(resolve_endpoint(&config), Err(GlossError::Config(_))),
                "endpoint must be rejected: {base_url}"
            );
        }
    }

    #[test]
    fn unusable_endpoints_report_config_error() {
        for base_url in ["", "   ", "file:///tmp/v1", "api.example.test/v1"] {
            let config = Config {
                base_url: base_url.into(),
                ..Default::default()
            };
            assert!(
                matches!(resolve_endpoint(&config), Err(GlossError::Config(_))),
                "endpoint {base_url:?} must be rejected"
            );
        }
    }

    #[test]
    fn resolves_key_from_keychain() {
        let (handle, store) = fixture(Some("test-key-value"));
        assert_eq!(
            resolve_api_key(&handle.snapshot(), store.as_ref()).expect("resolve"),
            "test-key-value"
        );
    }

    #[test]
    fn missing_key_reports_auth_error() {
        let (handle, store) = fixture(None);
        assert_eq!(
            resolve_api_key(&handle.snapshot(), store.as_ref()).expect_err("missing secret"),
            GlossError::EngineAuth
        );
    }

    #[test]
    fn blank_key_counts_as_missing() {
        let (handle, store) = fixture(Some("   "));
        assert_eq!(
            resolve_api_key(&handle.snapshot(), store.as_ref()).expect_err("blank secret"),
            GlossError::EngineAuth
        );
    }

    #[test]
    fn empty_provider_list_falls_back_to_the_factory_entry() {
        let store = Arc::new(MemoryConfigStore::default());
        store
            .set_secret("gloss/deepseek", "legacy-key")
            .expect("stub store accepts secret");
        let config = Config {
            provider_keys: Vec::new(),
            ..Default::default()
        };
        assert!(config.active_provider().is_none(), "纯查表仍应为空");
        assert_eq!(
            resolve_api_key(&config, store.as_ref()).expect("factory fallback"),
            "legacy-key"
        );
        assert_eq!(config.resolved_provider().keychain_id, "gloss/deepseek");
    }

    #[tokio::test]
    async fn adapter_streams_deltas_across_chunk_boundaries() {
        let payload = concat!(
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"光\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"泽\"}}]}\n\n",
            "data: [DONE]\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"多余\"}}]}\n",
        )
        .as_bytes();

        for chunk_size in [1, 2, 5, payload.len()] {
            let chunks: Vec<_> = payload
                .chunks(chunk_size)
                .map(|chunk| Ok(bytes::Bytes::copy_from_slice(chunk)))
                .collect();
            let mut adapted = SseStream::new(VecStream(chunks.into_iter()));

            let mut text = String::new();
            while let Some(item) =
                std::future::poll_fn(|cx| Pin::new(&mut adapted).poll_next(cx)).await
            {
                match item.expect("delta expected") {
                    TaskChunk::Delta(delta) => text.push_str(&delta),
                    TaskChunk::Done { .. } => {}
                }
            }
            assert_eq!(text, "光泽", "chunk_size = {chunk_size}");
        }
    }

    #[tokio::test]
    async fn adapter_delivers_the_done_marker_with_its_finish_reason() {
        let payload = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"半\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        let mut adapted = SseStream::new(VecStream(
            vec![Ok(bytes::Bytes::from_static(payload.as_bytes()))].into_iter(),
        ));

        let mut items = Vec::new();
        while let Some(item) = std::future::poll_fn(|cx| Pin::new(&mut adapted).poll_next(cx)).await
        {
            items.push(item);
        }
        assert_eq!(
            items,
            vec![
                Ok(TaskChunk::Delta("半".into())),
                Ok(TaskChunk::Done {
                    finish_reason: Some("stop".into())
                }),
            ],
            "the declared end is delivered as the terminal chunk with its reason"
        );
    }

    #[tokio::test]
    async fn adapter_delivers_deltas_then_terminates_on_protocol_error() {
        let payload = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"半\"}}]}\n\n",
            "data: not json\n",
        );
        let mut adapted = SseStream::new(VecStream(
            vec![Ok(bytes::Bytes::from_static(payload.as_bytes()))].into_iter(),
        ));

        let mut items = Vec::new();
        while let Some(item) = std::future::poll_fn(|cx| Pin::new(&mut adapted).poll_next(cx)).await
        {
            items.push(item);
        }
        assert_eq!(items.len(), 2, "delta then one error: {items:?}");
        assert_eq!(items[0], Ok(TaskChunk::Delta("半".into())));
        assert!(
            matches!(items[1], Err(GlossError::EngineResponse(_))),
            "protocol error must terminate the stream as a failure"
        );
    }

    #[tokio::test]
    async fn adapter_ends_cleanly_without_the_done_marker() {
        let payload = "data: {\"choices\":[{\"delta\":{\"content\":\"尾\"}}]}\n";
        let mut adapted = SseStream::new(VecStream(
            vec![Ok(bytes::Bytes::from_static(payload.as_bytes()))].into_iter(),
        ));

        let mut text = String::new();
        while let Some(item) = std::future::poll_fn(|cx| Pin::new(&mut adapted).poll_next(cx)).await
        {
            match item.expect("delta expected") {
                TaskChunk::Delta(delta) => text.push_str(&delta),
                TaskChunk::Done { .. } => {}
            }
        }
        assert_eq!(text, "尾");
    }

    #[tokio::test]
    async fn adapter_reports_empty_completion_as_failure() {
        for payload in [
            "data: [DONE]\n\n",
            "<html>nope</html>\n",
            concat!(
                "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                "data: [DONE]\n\n",
            ),
        ] {
            let mut adapted = SseStream::new(VecStream(
                vec![Ok(bytes::Bytes::from_static(payload.as_bytes()))].into_iter(),
            ));
            let item = std::future::poll_fn(|cx| Pin::new(&mut adapted).poll_next(cx))
                .await
                .expect("one item expected");
            assert!(
                matches!(item, Err(GlossError::EmptyCompletion)),
                "payload {payload:?} must fail: {item:?}"
            );
            assert!(
                std::future::poll_fn(|cx| Pin::new(&mut adapted).poll_next(cx))
                    .await
                    .is_none(),
                "stream must end after the failure"
            );
        }
    }

    #[test]
    fn request_body_has_the_openai_envelope() {
        let request = EngineRequest {
            messages: vec![ChatMessage::system("把用户给的词翻成中文")],
            model: "deepseek-chat".into(),
            max_tokens: None,
        };
        assert_eq!(
            chat_request_body(&request),
            serde_json::json!({
                "model": "deepseek-chat",
                "messages": [{"role": "system", "content": "把用户给的词翻成中文"}],
                "stream": true,
                "response_format": {"type": "json_object"},
            })
        );
    }

    #[test]
    fn json_mode_switch_is_always_on_the_wire() {
        let text = chat_request_body(&EngineRequest {
            messages: vec![ChatMessage::user("hi")],
            model: "m".into(),
            max_tokens: Some(64),
        });
        assert_eq!(
            text.get("response_format"),
            Some(&serde_json::json!({"type": "json_object"})),
            "every request carries the JSON contract, capped or not"
        );

        let multimodal = chat_request_body(&EngineRequest {
            messages: vec![ChatMessage::user_parts(vec![ContentPart::ImagePng(
                FIXTURE_PNG.into(),
            )])],
            model: "m".into(),
            max_tokens: None,
        });
        assert_eq!(
            multimodal.get("response_format"),
            Some(&serde_json::json!({"type": "json_object"})),
            "image tasks are under the same JSON contract"
        );
    }

    #[test]
    fn text_content_stays_a_json_string_on_the_wire() {
        let request = EngineRequest {
            messages: vec![ChatMessage::user("hi")],
            model: "m".into(),
            max_tokens: None,
        };
        let body = chat_request_body(&request);
        assert_eq!(
            body["messages"][0],
            serde_json::json!({"role": "user", "content": "hi"}),
            "a plain-text message must not become a content array"
        );
    }

    #[test]
    fn multimodal_parts_map_to_the_openai_content_array() {
        let request = EngineRequest {
            messages: vec![
                ChatMessage::system("解读图片"),
                ChatMessage::user_parts(vec![ContentPart::ImagePng(FIXTURE_PNG.into())]),
            ],
            model: "deepseek-flash".into(),
            max_tokens: None,
        };
        assert_eq!(
            chat_request_body(&request),
            serde_json::json!({
                "model": "deepseek-flash",
                "messages": [
                    {"role": "system", "content": "解读图片"},
                    {"role": "user", "content": [
                        {"type": "image_url", "image_url": {"url": concat!(
                            "data:image/png;base64,",
                            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAA",
                            "AAC0lEQVR42mNgAAIAAAUAAen63NgAAAAASUVORK5CYII=",
                        )}},
                    ]},
                ],
                "stream": true,
                "response_format": {"type": "json_object"},
            })
        );
    }

    #[test]
    fn max_tokens_is_carried_only_when_set() {
        let messages = vec![ChatMessage::user("hi")];
        let unset = chat_request_body(&EngineRequest {
            messages: messages.clone(),
            model: "m".into(),
            max_tokens: None,
        });
        assert!(
            unset.get("max_tokens").is_none(),
            "an unset limit must stay off the wire, not null"
        );
        let capped = chat_request_body(&EngineRequest {
            messages,
            model: "m".into(),
            max_tokens: Some(64),
        });
        assert_eq!(capped.get("max_tokens"), Some(&serde_json::json!(64)));
    }

    struct VecStream(std::vec::IntoIter<Result<bytes::Bytes, reqwest::Error>>);

    impl Stream for VecStream {
        type Item = Result<bytes::Bytes, reqwest::Error>;

        fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Poll::Ready(self.0.next())
        }
    }

    #[test]
    fn maps_http_status_to_error_variants() {
        assert_eq!(
            map_failure(StatusCode::UNAUTHORIZED, ""),
            GlossError::EngineAuth
        );
        assert_eq!(
            map_failure(StatusCode::FORBIDDEN, ""),
            GlossError::EngineAuth
        );
        assert_eq!(
            map_failure(StatusCode::TOO_MANY_REQUESTS, ""),
            GlossError::EngineRateLimited
        );
        assert_eq!(
            map_failure(StatusCode::INTERNAL_SERVER_ERROR, ""),
            GlossError::EngineNetwork,
            "server-side failure is retryable"
        );

        let err = map_failure(
            StatusCode::BAD_REQUEST,
            r#"{"error":{"message":"model `x` does not exist","type":"invalid_request_error"}}"#,
        );
        assert_eq!(
            err,
            GlossError::EngineResponse("HTTP 400 Bad Request: model `x` does not exist".into())
        );
        assert_eq!(
            map_failure(StatusCode::BAD_REQUEST, "<html>nope</html>"),
            GlossError::EngineResponse("HTTP 400 Bad Request".into())
        );
    }

    #[test]
    fn json_mode_and_multimodal_rejections_are_recognized_from_diagnostics() {
        assert_eq!(
            map_failure(
                StatusCode::BAD_REQUEST,
                r#"{"error":{"message":"response_format is not supported by this model","type":"invalid_request_error"}}"#,
            ),
            GlossError::JsonModeUnsupported
        );
        assert_eq!(
            map_failure(
                StatusCode::BAD_REQUEST,
                r#"{"error":{"message":"json_object output is unavailable on this endpoint"}}"#,
            ),
            GlossError::JsonModeUnsupported,
            "either marker of the JSON-mode switch is enough"
        );
        assert_eq!(
            map_failure(
                StatusCode::UNPROCESSABLE_ENTITY,
                r#"{"error":{"message":"This model does not support image input (vision)."}}"#,
            ),
            GlossError::UnsupportedModality
        );
        assert_eq!(
            map_failure(
                StatusCode::BAD_REQUEST,
                r#"{"error":{"message":"`messages` contains an unsupported multimodal content part"}}"#,
            ),
            GlossError::UnsupportedModality
        );
        assert_eq!(
            map_failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                r#"{"error":{"message":"vision routing exploded","type":"server_error"}}"#,
            ),
            GlossError::EngineNetwork,
            "the diagnostics heuristics are scoped to 4xx"
        );
        assert_eq!(
            map_failure(
                StatusCode::UNAUTHORIZED,
                r#"{"error":{"message":"invalid response_format"}}"#,
            ),
            GlossError::EngineAuth,
            "auth keeps its precedence over the diagnostics"
        );
    }

    #[test]
    fn retry_delay_covers_only_transient_failures_within_budget() {
        assert_eq!(
            retry_delay(&GlossError::EngineNetwork, None, RETRY_ATTEMPTS),
            Some(RETRY_BACKOFF)
        );
        assert_eq!(
            retry_delay(&GlossError::EngineRateLimited, None, RETRY_ATTEMPTS),
            Some(RETRY_BACKOFF)
        );
        assert_eq!(
            retry_delay(&GlossError::EngineNetwork, Some(Duration::from_secs(5)), 1),
            Some(RETRY_AFTER_CAP),
            "guidance beyond the cap is capped"
        );
        assert_eq!(
            retry_delay(
                &GlossError::EngineNetwork,
                Some(Duration::from_millis(100)),
                1
            ),
            Some(Duration::from_millis(100)),
            "guidance under the cap wins over the fixed backoff"
        );
        assert_eq!(
            retry_delay(&GlossError::EngineNetwork, None, 0),
            None,
            "an exhausted budget never retries"
        );
        assert_eq!(retry_delay(&GlossError::EngineAuth, None, 1), None);
        assert_eq!(
            retry_delay(&GlossError::EngineResponse("bad reply".into()), None, 1),
            None
        );
        assert_eq!(retry_delay(&GlossError::Config("x".into()), None, 1), None);
    }

    #[test]
    fn retry_after_header_parsing_accepts_seconds_only() {
        let mut headers = reqwest::header::HeaderMap::new();
        assert_eq!(parse_retry_after(&headers), None);

        headers.insert(
            reqwest::header::RETRY_AFTER,
            "2".parse().expect("header value"),
        );
        assert_eq!(parse_retry_after(&headers), Some(Duration::from_secs(2)));

        headers.insert(
            reqwest::header::RETRY_AFTER,
            " 1 ".parse().expect("header value"),
        );
        assert_eq!(
            parse_retry_after(&headers),
            Some(Duration::from_secs(1)),
            "padding around the number is tolerated"
        );

        headers.insert(
            reqwest::header::RETRY_AFTER,
            "Wed, 21 Oct 2015 07:28:00 GMT"
                .parse()
                .expect("header value"),
        );
        assert_eq!(
            parse_retry_after(&headers),
            None,
            "the HTTP-date form falls back to the fixed backoff"
        );

        headers.insert(
            reqwest::header::RETRY_AFTER,
            "soon".parse().expect("header value"),
        );
        assert_eq!(parse_retry_after(&headers), None);
    }

    #[tokio::test]
    async fn connect_retries_transient_failures_within_the_budget() {
        tokio::time::pause();
        let (attempt, calls) = queue_attempt(vec![
            Err(TransportFail {
                error: GlossError::EngineNetwork,
                retry_after: None,
            }),
            Ok(()),
        ]);
        let (result, retries_left) = connect_with_retry(attempt, RETRY_ATTEMPTS).await;
        assert!(result.is_ok(), "the second attempt lands");
        assert_eq!(*calls.lock().unwrap(), 2);
        assert_eq!(retries_left, 0);

        let (attempt, calls) = queue_attempt::<()>(vec![
            Err(TransportFail {
                error: GlossError::EngineNetwork,
                retry_after: None,
            }),
            Err(TransportFail {
                error: GlossError::EngineRateLimited,
                retry_after: None,
            }),
        ]);
        let (result, retries_left) = connect_with_retry(attempt, RETRY_ATTEMPTS).await;
        assert!(
            matches!(result, Err(GlossError::EngineRateLimited)),
            "the last failure is reported as-is: {result:?}"
        );
        assert_eq!(*calls.lock().unwrap(), 2);
        assert_eq!(retries_left, 0);

        let (attempt, calls) = queue_attempt::<()>(vec![Err(TransportFail {
            error: GlossError::EngineAuth,
            retry_after: None,
        })]);
        let (result, retries_left) = connect_with_retry(attempt, RETRY_ATTEMPTS).await;
        assert!(matches!(result, Err(GlossError::EngineAuth)));
        assert_eq!(*calls.lock().unwrap(), 1);
        assert_eq!(retries_left, RETRY_ATTEMPTS);
    }

    #[tokio::test]
    async fn first_byte_failures_retry_within_the_shared_budget() {
        tokio::time::pause();
        let good = chunk_stream(vec![
            Ok(TaskChunk::Delta("光".into())),
            Ok(TaskChunk::Done {
                finish_reason: Some("stop".into()),
            }),
        ]);
        let (attempt, calls) = queue_attempt(vec![Ok(good)]);
        let mut stream = FirstByteStream::new(
            attempt,
            chunk_stream(vec![Err(GlossError::EngineNetwork)]),
            RETRY_ATTEMPTS,
        );
        assert_eq!(
            next_item(&mut stream).await,
            Some(Ok(TaskChunk::Delta("光".into()))),
            "the retry is invisible to the consumer: no failure item is delivered"
        );
        assert_eq!(
            next_item(&mut stream).await,
            Some(Ok(TaskChunk::Done {
                finish_reason: Some("stop".into())
            }))
        );
        assert_eq!(next_item(&mut stream).await, None);
        assert_eq!(*calls.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn first_byte_budget_exhaustion_passes_the_error_through() {
        tokio::time::pause();
        let (attempt, calls) =
            queue_attempt(vec![Ok(chunk_stream(vec![Err(GlossError::EngineNetwork)]))]);
        let mut stream = FirstByteStream::new(
            attempt,
            chunk_stream(vec![Err(GlossError::EngineNetwork)]),
            RETRY_ATTEMPTS,
        );
        assert_eq!(
            next_item(&mut stream).await,
            Some(Err(GlossError::EngineNetwork)),
            "both attempts failed, the last error is delivered as-is"
        );
        assert_eq!(next_item(&mut stream).await, None);
        assert_eq!(*calls.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn non_retryable_first_byte_errors_pass_through_without_a_retry() {
        tokio::time::pause();
        let (attempt, calls) = queue_attempt(vec![]);
        let mut stream = FirstByteStream::new(
            attempt,
            chunk_stream(vec![Err(GlossError::EngineAuth)]),
            RETRY_ATTEMPTS,
        );
        assert_eq!(
            next_item(&mut stream).await,
            Some(Err(GlossError::EngineAuth))
        );
        assert_eq!(next_item(&mut stream).await, None);
        assert_eq!(*calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn errors_after_the_first_item_are_never_retried() {
        tokio::time::pause();
        let (attempt, calls) = queue_attempt(vec![]);
        let mut stream = FirstByteStream::new(
            attempt,
            chunk_stream(vec![
                Ok(TaskChunk::Delta("光".into())),
                Err(GlossError::EngineNetwork),
            ]),
            RETRY_ATTEMPTS,
        );
        assert_eq!(
            next_item(&mut stream).await,
            Some(Ok(TaskChunk::Delta("光".into())))
        );
        assert_eq!(
            next_item(&mut stream).await,
            Some(Err(GlossError::EngineNetwork)),
            "a mid-stream failure must not restart the request"
        );
        assert_eq!(next_item(&mut stream).await, None);
        assert_eq!(*calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn first_byte_retry_honors_a_zero_leftover_budget() {
        tokio::time::pause();
        let (attempt, calls) = queue_attempt(vec![]);
        let mut stream = FirstByteStream::new(
            attempt,
            chunk_stream(vec![Err(GlossError::EngineNetwork)]),
            0,
        );
        assert_eq!(
            next_item(&mut stream).await,
            Some(Err(GlossError::EngineNetwork)),
            "the connect stage spent the budget, the stream stage gets none"
        );
        assert_eq!(*calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn an_idle_stream_fails_after_the_silence_budget() {
        tokio::time::pause();
        let payload = "data: {\"choices\":[{\"delta\":{\"content\":\"光\"}}]}\n\n";
        let mut stream = SseStream::new(StallStream::new(bytes::Bytes::from_static(
            payload.as_bytes(),
        )));
        assert_eq!(
            next_item(&mut stream).await,
            Some(Ok(TaskChunk::Delta("光".into())))
        );
        assert_eq!(
            next_item(&mut stream).await,
            Some(Err(GlossError::EngineNetwork)),
            "a stalled connection ends the stream as a network failure"
        );
        assert_eq!(next_item(&mut stream).await, None);
    }

    #[tokio::test]
    async fn arriving_bytes_postpone_the_idle_deadline() {
        tokio::time::pause();
        let mut stream = SseStream::new(TimedChunks::new());
        for step in 0..3 {
            assert_eq!(
                next_item(&mut stream).await,
                Some(Ok(TaskChunk::Delta(step.to_string()))),
                "chunk {step} lands beyond the original deadline: arrivals reset the watchdog"
            );
        }
    }

    fn chunk_stream(items: Vec<Result<TaskChunk, GlossError>>) -> TaskStream {
        Box::pin(VecChunkStream(VecDeque::from(items)))
    }

    async fn next_item<S>(stream: &mut S) -> Option<Result<TaskChunk, GlossError>>
    where
        S: Stream<Item = Result<TaskChunk, GlossError>> + Unpin,
    {
        std::future::poll_fn(|cx| Pin::new(&mut *stream).poll_next(cx)).await
    }

    fn queue_attempt<T: Send + 'static>(
        outcomes: Vec<Result<T, TransportFail>>,
    ) -> (
        impl Fn() -> BoxFuture<'static, Result<T, TransportFail>>,
        Arc<Mutex<usize>>,
    ) {
        let queue = Arc::new(Mutex::new(VecDeque::from(outcomes)));
        let calls = Arc::new(Mutex::new(0usize));
        let attempt_queue = Arc::clone(&queue);
        let attempt_calls = Arc::clone(&calls);
        let attempt = move || {
            *attempt_calls.lock().unwrap() += 1;
            let outcome = attempt_queue
                .lock()
                .unwrap()
                .pop_front()
                .expect("attempt outcome queued");
            Box::pin(async move { outcome }) as BoxFuture<'static, Result<T, TransportFail>>
        };
        (attempt, calls)
    }

    struct VecChunkStream(VecDeque<Result<TaskChunk, GlossError>>);

    impl Stream for VecChunkStream {
        type Item = Result<TaskChunk, GlossError>;

        fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Poll::Ready(self.0.pop_front())
        }
    }

    struct StallStream(Option<bytes::Bytes>);

    impl StallStream {
        fn new(chunk: bytes::Bytes) -> Self {
            Self(Some(chunk))
        }
    }

    impl Stream for StallStream {
        type Item = Result<bytes::Bytes, reqwest::Error>;

        fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            match self.0.take() {
                Some(chunk) => Poll::Ready(Some(Ok(chunk))),
                None => Poll::Pending,
            }
        }
    }

    struct TimedChunks {
        step: u32,
        next: Pin<Box<Sleep>>,
    }

    impl TimedChunks {
        fn new() -> Self {
            Self {
                step: 0,
                next: Box::pin(sleep(Duration::from_secs(30))),
            }
        }
    }

    impl Stream for TimedChunks {
        type Item = Result<bytes::Bytes, reqwest::Error>;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            if self.next.as_mut().poll(cx).is_ready() {
                self.next
                    .as_mut()
                    .reset(TokioInstant::now() + Duration::from_secs(30));
                let payload = format!(
                    "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{}\"}}}}]}}\n\n",
                    self.step
                );
                self.step += 1;
                return Poll::Ready(Some(Ok(bytes::Bytes::from(payload))));
            }
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    use crate::ports::TaskChunk;
    use crate::prompt::{ChatMessage, PromptRegistry};
    use crate::schema;
    use crate::stubs::ports::MemoryConfigStore;
    use crate::task::{Task, TaskInput, TaskKind, TaskOptions};

    const ONE_PIXEL_PNG: [u8; 70] = [
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 218, 99, 100, 96, 248, 95, 15,
        0, 2, 135, 1, 128, 235, 71, 186, 146, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];

    fn require_live_env() -> (String, String, String) {
        let read = |name: &str| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
        };
        let (Some(base_url), Some(model), Some(key)) = (
            read("GLOSS_LIVE_BASE_URL"),
            read("GLOSS_LIVE_MODEL"),
            read("GLOSS_LIVE_API_KEY"),
        ) else {
            panic!(
                "真机测试缺少环境变量。修复：GLOSS_LIVE_API_KEY=sk-... \
                 GLOSS_LIVE_BASE_URL=https://api.deepseek.com/v1 \
                 GLOSS_LIVE_MODEL=deepseek-flash \
                 cargo test -p gloss-core -- --ignored live_llm"
            );
        };
        (base_url, model, key)
    }

    fn live_client() -> (String, LlmClient) {
        let (base_url, model, key) = require_live_env();
        let store = Arc::new(MemoryConfigStore::default());
        store
            .set_secret("gloss/deepseek", &key)
            .expect("stub store accepts secret");
        let config = Config {
            base_url,
            model: model.clone(),
            ..Default::default()
        };
        let handle = Arc::new(ConfigHandle::with_config(store.clone(), config));
        let client = LlmClient::new(handle, store).expect("http client should build");
        (model, client)
    }

    #[tokio::test]
    #[ignore = "真机：需 GLOSS_LIVE_API_KEY / GLOSS_LIVE_BASE_URL / GLOSS_LIVE_MODEL（见本模块文档）"]
    async fn live_llm_streams_a_translation() {
        let (model, client) = live_client();

        let request = EngineRequest {
            messages: vec![
                ChatMessage::system("把用户给的句子翻成中文，只输出译文。"),
                ChatMessage::user("The quick brown fox jumps over the lazy dog."),
            ],
            model,
            max_tokens: None,
        };

        let mut stream = client.execute(request).await.expect("request should start");
        let mut text = String::new();
        while let Some(item) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            match item {
                Ok(TaskChunk::Delta(delta)) => text.push_str(&delta),
                Ok(TaskChunk::Done { .. }) => {}
                Err(error) => panic!("stream failed: {error}"),
            }
            if text.chars().count() > 80 {
                break;
            }
        }
        assert!(
            !text.trim().is_empty(),
            "expected streamed translation text"
        );
    }

    #[tokio::test]
    #[ignore = "真机：需 GLOSS_LIVE_* 与支持图像输入的模型（bdd.md 人工测试有照抄步骤）"]
    async fn live_llm_streams_an_image_contract() {
        let (model, client) = live_client();

        let task = Task::new(
            TaskKind::ImageExplain,
            TaskInput::Image {
                png: Arc::from(&ONE_PIXEL_PNG[..]),
            },
            TaskOptions::default(),
        )
        .expect("the test task is legal");
        let messages = PromptRegistry::new()
            .render(&task)
            .expect("image task passes the modality check");

        let mut stream = client
            .execute(EngineRequest {
                messages,
                model,
                max_tokens: None,
            })
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "真机图像请求失败（模型须支持图像输入与 JSON 输出模式）：{error}；\
                     修复：把 GLOSS_LIVE_MODEL 换成支持 vision 的模型后重跑"
                )
            });
        let mut text = String::new();
        while let Some(item) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            match item {
                Ok(TaskChunk::Delta(delta)) => text.push_str(&delta),
                Ok(TaskChunk::Done { .. }) => {}
                Err(error) => panic!("stream failed: {error}"),
            }
        }
        let (note, _) =
            schema::validate_task(TaskKind::ImageExplain, &text).unwrap_or_else(|violation| {
                panic!(
                    "真机图像回复未过契约校验（{violation}）：端点未按 JSON 模式输出完整图注契约"
                )
            });
        assert!(
            !note.trim().is_empty(),
            "the streamed note must be non-empty"
        );
    }
}
