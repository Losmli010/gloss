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
            // 端点与密钥分两条解析路径：端点只由配置决定，密钥只经 keychain，
            // 两者不共用返回值——否则污染分析会把 URL 也算成「可能含密钥的
            // 数据」，而真正该守的是「密钥只走 HTTPS 端点」这一条。
            let url = resolve_endpoint(&snapshot)?;
            let key = resolve_api_key(&snapshot, store.as_ref())?;
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

/// 请求端点：由配置快照解析 `{base_url}/chat/completions`。
fn resolve_endpoint(config: &Config) -> Result<String, GlossError> {
    let base_url = config.base_url.trim();
    if base_url.is_empty() {
        return Err(GlossError::Config("no provider endpoint configured".into()));
    }
    let endpoint = reqwest::Url::parse(base_url)
        .map_err(|err| GlossError::Config(format!("invalid provider endpoint: {err}")))?;
    // 只接受 HTTPS：密钥经这个端点送出去，明文一律拒绝（本机网关请在前面
    // 终止 TLS）。
    if endpoint.scheme() != "https" {
        return Err(GlossError::Config(
            "provider endpoint must use https".into(),
        ));
    }
    Ok(format!(
        "{}/{CHAT_COMPLETIONS_PATH}",
        endpoint.as_str().trim_end_matches('/')
    ))
}

/// API 密钥：按 provider 条目的 keychain 标识**每请求直查**（不缓存，只进
/// 请求头）。未配置时返回 [`GlossError::EngineAuth`]，由 UI 引导去设置页。
fn resolve_api_key(config: &Config, store: &dyn ConfigStore) -> Result<String, GlossError> {
    let provider = config
        .active_provider()
        .ok_or_else(|| GlossError::Config("no provider configured".into()))?;
    store
        .secret(&provider.keychain_id)?
        .filter(|key| !key.trim().is_empty())
        .ok_or(GlossError::EngineAuth)
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

    /// 端点解析：出厂配置得到 OpenAI 兼容的补全路径。
    #[test]
    fn resolves_endpoint_from_config() {
        let config = Config::default();
        assert_eq!(
            resolve_endpoint(&config).expect("resolve"),
            format!("{}/chat/completions", gloss_core::config::DEFAULT_BASE_URL)
        );
    }

    /// 端点尾斜杠不产生双斜杠（手改配置很常见）。
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

    /// 明文端点一律拒绝（含本机网关）：密钥经这个端点送出去，明文的密钥
    /// 不出门；需要本地模型时在网关前终止 TLS。
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

    /// 空串、非 http(s) 的 scheme 与解析不了的地址同样报配置错误（不猜一个
    /// 端点替用户发出去）。
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

    /// 密钥解析：出厂配置 + keychain 里的密钥可正常取出。
    #[test]
    fn resolves_key_from_keychain() {
        let (handle, store) = fixture(Some("test-key-value"));
        assert_eq!(
            resolve_api_key(&handle.snapshot(), store.as_ref()).expect("resolve"),
            "test-key-value"
        );
    }

    /// 未配置密钥 → `EngineAuth`（UI 据此引导去设置页），而不是拿空 key 去请求。
    #[test]
    fn missing_key_reports_auth_error() {
        let (handle, store) = fixture(None);
        assert_eq!(
            resolve_api_key(&handle.snapshot(), store.as_ref()).expect_err("missing secret"),
            GlossError::EngineAuth
        );
    }

    /// 空白密钥（误存了一个空串）按未配置处理。
    #[test]
    fn blank_key_counts_as_missing() {
        let (handle, store) = fixture(Some("   "));
        assert_eq!(
            resolve_api_key(&handle.snapshot(), store.as_ref()).expect_err("blank secret"),
            GlossError::EngineAuth
        );
    }

    /// 未配置 provider 条目 → 配置错误。
    #[test]
    fn missing_provider_reports_config_error() {
        let store = Arc::new(MemoryConfigStore::default());
        let config = Config {
            provider_keys: Vec::new(),
            ..Default::default()
        };
        assert!(matches!(
            resolve_api_key(&config, store.as_ref()),
            Err(GlossError::Config(_))
        ));
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
