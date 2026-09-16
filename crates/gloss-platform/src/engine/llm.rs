//! LlmClient：OpenAI 兼容端点的流式引擎适配器（M4-T4）。
//!
//! 边界：**只负责把请求送出去、把响应流回来**。messages 由 core 编排渲染
//! （`AiTaskService` → `PromptRegistry`，含模态校验），模型也已由 App 按配置
//! 解析后随 [`EngineRequest`] 携带——本模块不渲染 prompt，也不看配置里的模型。
//!
//! 端点与密钥：端点取配置快照的 `base_url`；密钥按 provider 条目的 keychain
//! 条目标识**每请求直查**（不缓存，除请求头外不进任何地方——错误消息与日志里
//! 只有状态码与服务端诊断文本；红线与措辞见 `ports::ConfigStore` 的文档）。
//!
//! 超时：只设建连超时。流式响应不设总超时——长回答是正常情形，总超时会误杀；
//! 取消由调用方的 `CancellationToken` 竞速完成（future 被丢弃即断链）。

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use futures_core::Stream;
use gloss_core::config::Config;
use gloss_core::config_handle::ConfigHandle;
use gloss_core::log::debug;
use gloss_core::model::GlossError;
use gloss_core::ports::{AiEngine, BoxFuture, ConfigStore, EngineRequest, TaskStream};
use reqwest::StatusCode;

use super::sse::{SseDecoder, SseItem};

/// 建连超时（不含响应与流式读取）。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// 聊天补全路径（OpenAI 兼容）。
const CHAT_COMPLETIONS_PATH: &str = "chat/completions";

/// OpenAI 兼容流式引擎。
pub struct LlmClient {
    /// 复用连接池的 HTTP 客户端（建连超时在这里定）。
    client: reqwest::Client,
    /// 端点等配置快照（模型不经这里）。
    config: Arc<ConfigHandle>,
    /// 密钥直查（keychain）。
    store: Arc<dyn ConfigStore>,
}

impl LlmClient {
    /// 组装：配置句柄与存储都由组装点注入（句柄给端点，存储给密钥）。
    pub fn new(config: Arc<ConfigHandle>, store: Arc<dyn ConfigStore>) -> Result<Self, GlossError> {
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
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
        request: &EngineRequest,
    ) -> BoxFuture<'static, Result<TaskStream, GlossError>> {
        let client = self.client.clone();
        let config = Arc::clone(&self.config);
        let store = Arc::clone(&self.store);
        let kind = request.kind;
        let model = request.model.clone();
        let messages = request.messages.clone();

        Box::pin(async move {
            if messages.is_empty() {
                // 渲染层（core 编排）保证至少一条 user 消息；空请求是接线错误，
                // 不该把一个空 body 发给付费端点。
                return Err(GlossError::EngineResponse("empty request".into()));
            }
            let snapshot = config.snapshot();
            let (url, key) = resolve_target(&snapshot, store.as_ref())?;
            let body = serde_json::json!({
                "model": model,
                "messages": messages,
                "stream": true,
            });

            let response = client
                .post(&url)
                .bearer_auth(key)
                .json(&body)
                .send()
                .await
                .map_err(map_transport_error)?;

            let status = response.status();
            if !status.is_success() {
                // 服务端诊断文本（可能是 JSON 错误对象）：只取 message 字段，
                // 不转述整个响应体；密钥在请求头里，不会出现在这里。
                let detail = response.text().await.unwrap_or_default();
                return Err(map_failure(status, &detail));
            }
            debug!(
                kind = ?kind,
                model = %model,
                "llm stream established"
            );
            Ok(Box::pin(SseStream::new(response.bytes_stream())) as TaskStream)
        })
    }
}

/// 解析本请求的端点与密钥：端点在快照里，密钥每请求直查 keychain。
fn resolve_target(
    config: &Config,
    store: &dyn ConfigStore,
) -> Result<(String, String), GlossError> {
    let base_url = config.base_url.trim();
    if base_url.is_empty() {
        return Err(GlossError::Config("no provider endpoint configured".into()));
    }
    let provider = config
        .active_provider()
        .ok_or_else(|| GlossError::Config("no provider configured".into()))?;
    let key = store
        .secret(&provider.keychain_id)?
        .filter(|key| !key.trim().is_empty())
        .ok_or(GlossError::EngineAuth)?;
    Ok((
        format!("{}/{CHAT_COMPLETIONS_PATH}", base_url.trim_end_matches('/')),
        key,
    ))
}

/// 传输层错误 → [`GlossError`]：连不上/超时/连接中断都可重试。
fn map_transport_error(err: reqwest::Error) -> GlossError {
    if err.is_decode() {
        GlossError::EngineResponse(format!("decode response: {err}"))
    } else {
        GlossError::EngineNetwork
    }
}

/// HTTP 状态 → [`GlossError`]（06 §7 的错误分类表）。
fn map_failure(status: StatusCode, body: &str) -> GlossError {
    match status.as_u16() {
        401 | 403 => GlossError::EngineAuth,
        429 => GlossError::EngineRateLimited,
        500..=599 => GlossError::EngineNetwork,
        _ => match server_message(body) {
            Some(message) => GlossError::EngineResponse(format!("HTTP {status}: {message}")),
            None => GlossError::EngineResponse(format!("HTTP {status}")),
        },
    }
}

/// 从错误响应体里取服务端诊断文本（仅 `error.message`，OpenAI 兼容形状）。
fn server_message(body: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct ErrorEnvelope {
        error: ErrorBody,
    }
    #[derive(serde::Deserialize)]
    struct ErrorBody {
        message: String,
    }

    let envelope: ErrorEnvelope = serde_json::from_str(body).ok()?;
    let message = envelope.error.message;
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return None;
    }
    // 上限与 SSE 诊断一致：错误文本会进 UI 与日志。
    Some(trimmed.chars().take(200).collect())
}

/// 把响应的字节流适配成 [`TaskStream`]：SSE 逐行解码后只吐文本增量。
///
/// 手动实现 `poll_next`（与编排层的手工轮询同一风格），不引 futures-util：
/// 状态机只有「攒够一行 → 产出条目 → 继续拉字节」三步。首个 `Err` 即终结流
/// （端口契约），此后不再拉取字节。
struct SseStream {
    inner: Pin<Box<dyn Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>,
    decoder: SseDecoder,
    ready: VecDeque<Result<String, GlossError>>,
    finished: bool,
}

impl SseStream {
    fn new(
        inner: impl Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
    ) -> Self {
        Self {
            inner: Box::pin(inner),
            decoder: SseDecoder::new(),
            ready: VecDeque::new(),
            finished: false,
        }
    }

    /// 收下解码器产出：增量入队，`Done` / `Failed` 终直流。
    fn absorb(&mut self, items: Vec<SseItem>) {
        for item in items {
            match item {
                SseItem::Delta(delta) => self.ready.push_back(Ok(delta)),
                SseItem::Done => self.finished = true,
                SseItem::Failed(error) => {
                    self.ready.push_back(Err(error));
                    self.finished = true;
                }
            }
        }
    }
}

impl Stream for SseStream {
    type Item = Result<String, GlossError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            if let Some(item) = this.ready.pop_front() {
                return Poll::Ready(Some(item));
            }
            if this.finished {
                return Poll::Ready(None);
            }
            match this.inner.as_mut().poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Some(Ok(chunk))) => {
                    let items = this.decoder.push(&chunk);
                    this.absorb(items);
                }
                Poll::Ready(Some(Err(err))) => {
                    this.finished = true;
                    // 本轮解码器里可能还有已收下的增量，但连接已断：按端口
                    // 契约以第一个 Err 终结，未产出的增量随流一起丢弃。
                    return Poll::Ready(Some(Err(map_transport_error(err))));
                }
                Poll::Ready(None) => {
                    this.finished = true;
                    this.decoder.finish();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gloss_core::config::ModelBinding;
    use gloss_core::ports::mocks::MemoryConfigStore;
    use gloss_core::task::TaskKind;

    /// 配置快照 + 内存存储（内存桩来自 core 的 test-util，测试总线一致）。
    fn fixture(secret: Option<&str>) -> (Arc<ConfigHandle>, Arc<MemoryConfigStore>) {
        let store = Arc::new(MemoryConfigStore::default());
        if let Some(secret) = secret {
            store
                .set_secret("gloss/deepseek", secret)
                .expect("stub store accepts secret");
        }
        let config = Config {
            model_by_kind: vec![ModelBinding {
                kind: TaskKind::TranslateWord,
                model: "deepseek-chat".into(),
            }],
            ..Default::default()
        };
        (
            Arc::new(ConfigHandle::with_config(store.clone(), config)),
            store,
        )
    }

    /// 端点与密钥的解析：出厂配置 + keychain 里的密钥得到完整 URL。
    #[test]
    fn resolves_endpoint_and_key_from_config_and_keychain() {
        let (handle, store) = fixture(Some("sk-test"));
        let (url, key) = resolve_target(&handle.snapshot(), store.as_ref()).expect("resolve");
        assert_eq!(
            url,
            format!("{}/chat/completions", gloss_core::config::DEFAULT_BASE_URL)
        );
        assert_eq!(key, "sk-test");
    }

    /// 端点尾斜杠不产生双斜杠（手改配置很常见）。
    #[test]
    fn endpoint_trimming_avoids_double_slash() {
        let store = Arc::new(MemoryConfigStore::default());
        store
            .set_secret("gloss/deepseek", "sk-test")
            .expect("stub store accepts secret");
        let config = Config {
            base_url: "https://example.test/v1/".into(),
            ..Default::default()
        };
        let (url, _) = resolve_target(&config, store.as_ref()).expect("resolve should work");
        assert_eq!(url, "https://example.test/v1/chat/completions");
    }

    /// 未配置密钥 → `EngineAuth`（UI 据此引导去设置页），而不是拿空 key 去请求。
    #[test]
    fn missing_key_reports_auth_error() {
        let (handle, store) = fixture(None);
        let err = resolve_target(&handle.snapshot(), store.as_ref())
            .expect_err("missing secret must fail");
        assert_eq!(err, GlossError::EngineAuth);
    }

    /// 空白密钥（误存了一个空串）按未配置处理。
    #[test]
    fn blank_key_counts_as_missing() {
        let (handle, store) = fixture(Some("   "));
        assert_eq!(
            resolve_target(&handle.snapshot(), store.as_ref()).expect_err("blank secret"),
            GlossError::EngineAuth
        );
    }

    /// 端点空串 → 配置错误（不猜一个端点替用户发出去）。
    #[test]
    fn missing_endpoint_reports_config_error() {
        let store = Arc::new(MemoryConfigStore::default());
        store
            .set_secret("gloss/deepseek", "sk-test")
            .expect("stub store accepts secret");
        let config = Config {
            base_url: String::new(),
            ..Default::default()
        };
        let err = resolve_target(&config, store.as_ref()).expect_err("empty base_url");
        assert!(matches!(err, GlossError::Config(_)), "got {err:?}");
    }

    /// 未配置 provider 条目 → 配置错误。
    #[test]
    fn missing_provider_reports_config_error() {
        let store = Arc::new(MemoryConfigStore::default());
        let config = Config {
            provider_keys: Vec::new(),
            ..Default::default()
        };
        let err = resolve_target(&config, store.as_ref()).expect_err("no provider");
        assert!(matches!(err, GlossError::Config(_)), "got {err:?}");
    }

    /// 适配器状态机（不碰网络）：同一段响应按任意字节边界分块喂入，产出的
    /// 增量顺序与内容都不变；`[DONE]` 之后流即结束，剩余字节不再解析。
    ///
    /// 传输层报错那条分支（内部流吐 `Err`）无法在单测里构造——`reqwest::Error`
    /// 没有公开构造子，只能在真实网络故障时走到，由 L4（或线上）覆盖。
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
                text.push_str(&item.expect("delta expected"));
            }
            assert_eq!(text, "光泽", "chunk_size = {chunk_size}");
        }
    }

    /// 测试用的字节流：预置块逐块吐完（不模拟 pending，适配器的 pending
    /// 分支由真实网络覆盖）。
    struct VecStream(std::vec::IntoIter<Result<bytes::Bytes, reqwest::Error>>);

    impl Stream for VecStream {
        type Item = Result<bytes::Bytes, reqwest::Error>;

        fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Poll::Ready(self.0.next())
        }
    }

    /// 状态码映射（06 §7）：鉴权 / 限流 / 服务端错误分类，其余带服务端诊断。
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

        // 其它 4xx：带上服务端说法，诊断才有用。
        let err = map_failure(
            StatusCode::BAD_REQUEST,
            r#"{"error":{"message":"model `x` does not exist","type":"invalid_request_error"}}"#,
        );
        assert_eq!(
            err,
            GlossError::EngineResponse("HTTP 400 Bad Request: model `x` does not exist".into())
        );
        // 非 JSON / 空响应体：只报状态码。
        assert_eq!(
            map_failure(StatusCode::BAD_REQUEST, "<html>nope</html>"),
            GlossError::EngineResponse("HTTP 400 Bad Request".into())
        );
    }
}

/// L4 opt-in 真机测试（不进 CI）：打真实 OpenAI 兼容端点，验收「实测流式返回」。
///
/// ```bash
/// GLOSS_LIVE_API_KEY=sk-... \
/// GLOSS_LIVE_BASE_URL=https://api.deepseek.com/v1 \
/// GLOSS_LIVE_MODEL=deepseek-chat \
///   cargo test -p gloss-platform -- --ignored live_llm
/// ```
///
/// 前置检查：三个环境变量缺一就以带修复指引的消息当场失败，而不是让请求以
/// 401 / 超时这类间接症状暴露。密钥只进请求头——测试正文与失败消息都不打印它。
#[cfg(test)]
mod live_tests {
    use super::*;
    use gloss_core::config::ModelBinding;
    use gloss_core::ports::mocks::MemoryConfigStore;
    use gloss_core::prompt::{ChatMessage, Role};
    use gloss_core::task::TaskKind;

    /// 读真机测试所需环境变量；缺项立刻失败并给出可直接照抄的命令。
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
                 GLOSS_LIVE_MODEL=deepseek-chat \
                 cargo test -p gloss-platform -- --ignored live_llm"
            );
        };
        (base_url, model, key)
    }

    #[tokio::test]
    #[ignore = "真机：需 GLOSS_LIVE_API_KEY / GLOSS_LIVE_BASE_URL / GLOSS_LIVE_MODEL（见本模块文档）"]
    async fn live_llm_streams_a_translation() {
        let (base_url, model, key) = require_live_env();
        let store = Arc::new(MemoryConfigStore::default());
        store
            .set_secret("gloss/deepseek", &key)
            .expect("stub store accepts secret");
        let config = Config {
            base_url,
            model_by_kind: vec![ModelBinding {
                kind: TaskKind::TranslateSentence,
                model: model.clone(),
            }],
            ..Default::default()
        };
        let handle = Arc::new(ConfigHandle::with_config(store.clone(), config));
        let client = LlmClient::new(handle, store).expect("http client should build");

        let request = EngineRequest {
            kind: TaskKind::TranslateSentence,
            messages: vec![
                ChatMessage {
                    role: Role::System,
                    content: "把用户给的句子翻成中文，只输出译文。".into(),
                },
                ChatMessage {
                    role: Role::User,
                    content: "The quick brown fox jumps over the lazy dog.".into(),
                },
            ],
            model,
        };

        let mut stream = client
            .execute(&request)
            .await
            .expect("request should start");
        let mut text = String::new();
        while let Some(item) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            match item {
                Ok(delta) => text.push_str(&delta),
                Err(error) => panic!("stream failed: {error}"),
            }
            // 验收「流式返回」不需要读完整篇：有若干增量即可收手。
            if text.chars().count() > 80 {
                break;
            }
        }
        assert!(
            !text.trim().is_empty(),
            "expected streamed translation text"
        );
    }
}
