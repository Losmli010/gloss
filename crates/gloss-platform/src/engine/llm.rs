//! LlmClient：OpenAI 兼容端点的流式引擎适配器。
//!
//! 边界：**只负责把请求送出去、把响应流回来**。messages 由 core 编排渲染
//! （`AiTaskService` → `PromptRegistry`，含模态校验），模型也已由 App 按配置
//! 解析后随 [`EngineRequest`] 携带——本模块不渲染 prompt，也不看配置里的模型。
//!
//! 端点与密钥：端点取配置快照的 `base_url`；密钥按 provider 条目的 keychain
//! 条目标识**每请求直查**（不缓存，除请求头外不进任何地方——错误消息与日志里
//! 只有状态码与服务端诊断文本；红线与措辞见 `ports::ConfigStore` 的文档）。
//!
//! 超时：只设建连超时，流式响应不设总超时；
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

/// 错误响应体的读取上限：只要 `error.message`，多余字节没有价值，而错误
/// 网关的劫持页可能极大（截断后 JSON 解析失败会退化成「只报状态码」，
/// 与既有非 JSON 路径一致）。
const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;

/// 组装 OpenAI 兼容请求体（`chat/completions` 的形状）。
///
fn chat_request_body(request: &EngineRequest) -> serde_json::Value {
    serde_json::json!({
        "model": request.model,
        "messages": request.messages,
        "stream": true,
    })
}

/// 读响应体但不超过 `limit` 字节（丢失的只是诊断文本，不是业务数据）。
async fn read_bounded_body(response: &mut reqwest::Response, limit: usize) -> String {
    let mut body = Vec::new();
    while let Ok(Some(chunk)) = response.chunk().await {
        body.extend_from_slice(&chunk);
        if body.len() >= limit {
            break;
        }
    }
    String::from_utf8_lossy(&body).into_owned()
}

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
            // 带一个明确的 UA：部分前置 CDN 对空 UA 返回 403，而 403 在这里
            // 会被映射成 EngineAuth，用户会被引去重填密钥——方向完全错了。
            .user_agent(concat!("gloss/", env!("CARGO_PKG_VERSION")))
            // 不跟随重定向。
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
            // 空白模型同样是配置问题：发了也是必然 400 的请求，还会把病因
            // 藏进服务端的错误文案里（手改 model_by_kind 或设置页存了空串）。
            if model.trim().is_empty() {
                return Err(GlossError::Config("empty model id".into()));
            }
            let snapshot = config.snapshot();
            // 端点与密钥分两条解析路径，不共用返回值。
            let url = resolve_endpoint(&snapshot)?;
            let key = resolve_api_key(&snapshot, store.as_ref())?;
            let body = chat_request_body(&EngineRequest {
                kind,
                messages,
                model: model.trim().to_owned(),
            });

            let mut response = client
                .post(&url)
                .bearer_auth(key)
                .json(&body)
                .send()
                .await
                .map_err(map_transport_error)?;

            let status = response.status();
            if !status.is_success() {
                // 服务端诊断文本（可能是 JSON 错误对象）：只取 message 字段，
                // 不转述整个响应体；密钥在请求头里，不会出现在这里。读取带上限
                // ——错误网关/WAF 可能回几百 MB 的劫持页，不能整份吃进内存。
                let detail = read_bounded_body(&mut response, MAX_ERROR_BODY_BYTES).await;
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
    // 结构性规则（非空/https/无凭据/无 query·fragment）与设置页逐字段
    // 校验共源：`gloss_core::config::validate_base_url`。
    gloss_core::config::validate_base_url(base_url)
        .map_err(|err| GlossError::Config(err.to_string()))?;
    let mut endpoint = reqwest::Url::parse(base_url)
        .map_err(|err| GlossError::Config(format!("invalid provider endpoint: {err}")))?;
    // 用 Url 设路径而不是字符串拼接：手改配置的尾斜杠写没写都得到同一结果。
    let path = format!(
        "{}/{CHAT_COMPLETIONS_PATH}",
        endpoint.path().trim_end_matches('/')
    );
    endpoint.set_path(&path);
    Ok(endpoint.to_string())
}

/// API 密钥：按 provider 条目的 keychain 标识**每请求直查**（不缓存，只进
/// 请求头）。未配置时返回 [`GlossError::EngineAuth`]，由 UI 引导去设置页。
fn resolve_api_key(config: &Config, store: &dyn ConfigStore) -> Result<String, GlossError> {
    let provider = config.resolved_provider();
    // trim 后再返回：从终端/文件复制粘贴进来的尾随换行会让 Authorization
    // 头非法（reqwest 报 builder 错 → 被当成网络问题的永久失败），尾随空格
    // 则变成服务端 401。
    store
        .secret(&provider.keychain_id)?
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty())
        .ok_or(GlossError::EngineAuth)
}

/// 传输层错误 → [`GlossError`]：连不上/超时/连接中断都可重试。
fn map_transport_error(err: reqwest::Error) -> GlossError {
    // builder 错是本地构造失败（非法 header 等）——重试永远不会成功，报成
    // EngineNetwork（可重试）会把用户引到错误的方向。
    if err.is_builder() {
        GlossError::EngineResponse(format!("request rejected: {err}"))
    } else if err.is_decode() {
        GlossError::EngineResponse(format!("decode response: {err}"))
    } else {
        GlossError::EngineNetwork
    }
}

/// HTTP 状态 → [`GlossError`]（错误分类表）。
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
    /// 是否产出过增量：零增量的「成功」是坏响应（网关忽略 `stream:true` 返回
    /// 非流式 JSON、劫持页返回 200 HTML、模型只回 refusal 等），不能当成功
    /// 产物写进缓存——用户会看到空白卡片，重复触发还命同一份空白。
    produced: bool,
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
            produced: false,
            finished: false,
        }
    }

    /// 收下解码器产出：增量入队，`Done` / `Failed` 终直流。
    fn absorb(&mut self, items: Vec<SseItem>) {
        for item in items {
            match item {
                SseItem::Delta(delta) => {
                    self.produced = true;
                    self.ready.push_back(Ok(delta));
                }
                SseItem::Done => {
                    self.finish_without_error();
                }
                SseItem::Failed(error) => {
                    self.ready.push_back(Err(error));
                    self.finished = true;
                }
            }
        }
    }

    /// 正常收尾（`[DONE]` 或连接关闭）：一条增量都没产出过就是坏响应。
    /// `ready` 先于 `finished` 被排空，所以这条 Err 一定排在已入队的增量之后。
    fn finish_without_error(&mut self) {
        if !self.produced {
            self.ready
                .push_back(Err(GlossError::EngineResponse("empty completion".into())));
        }
        self.finished = true;
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
                    // 连接断了：把错误排在已收下的增量之后（端口契约是「首个
                    // Err 终结流」，之前到达的增量仍应交付）。
                    this.ready.push_back(Err(map_transport_error(err)));
                    this.finished = true;
                }
                Poll::Ready(None) => {
                    // 连接关闭即流结束：服务端没给 [DONE] 也按正常收尾，
                    // 顺便收下尾巴里那条完整但没换行终止的增量。
                    let tail = this.decoder.finish();
                    this.absorb(tail);
                    this.finish_without_error();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stubs::ports::MemoryConfigStore;
    use gloss_core::prompt::{ChatMessage, Role};
    use gloss_core::task::TaskKind;

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
            format!("{}/chat/completions", gloss_core::config::DEFAULT_BASE_URL)
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
                text.push_str(&item.expect("delta expected"));
            }
            assert_eq!(text, "光泽", "chunk_size = {chunk_size}");
        }
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
        assert_eq!(items[0], Ok("半".into()));
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
            text.push_str(&item.expect("delta expected"));
        }
        assert_eq!(text, "尾");
    }

    #[tokio::test]
    async fn adapter_reports_empty_completion_as_failure() {
        for payload in ["data: [DONE]\n\n", "<html>nope</html>\n"] {
            let mut adapted = SseStream::new(VecStream(
                vec![Ok(bytes::Bytes::from_static(payload.as_bytes()))].into_iter(),
            ));
            let item = std::future::poll_fn(|cx| Pin::new(&mut adapted).poll_next(cx))
                .await
                .expect("one item expected");
            assert!(
                matches!(item, Err(GlossError::EngineResponse(_))),
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
            kind: TaskKind::TranslateWord,
            messages: vec![ChatMessage {
                role: Role::System,
                content: "把用户给的词翻成中文".into(),
            }],
            model: "deepseek-chat".into(),
        };
        assert_eq!(
            chat_request_body(&request),
            serde_json::json!({
                "model": "deepseek-chat",
                "messages": [{"role": "system", "content": "把用户给的词翻成中文"}],
                "stream": true,
            })
        );
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
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use crate::stubs::ports::MemoryConfigStore;
    use gloss_core::config::ModelBinding;
    use gloss_core::prompt::{ChatMessage, Role};
    use gloss_core::task::TaskKind;

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
