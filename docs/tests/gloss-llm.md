# 测试行为清单（`gloss-llm`）

本文件登记传输分包（`gloss-llm`）的测试行为；总纲、人工测试与发版人工步骤见 `bdd.md`。

## 单元测试

### `crates/gloss-llm/src/lib.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| resolves_endpoint_from_config | 端点从配置解析 | 给定出厂配置，当解析端点，则得 `{DEFAULT_BASE_URL}/chat/completions` | 2026-09-19 |
| endpoint_trimming_avoids_double_slash | 端点尾斜杠去重 | 给定尾斜杠 `base_url`，当解析，则无双斜杠 | 2026-09-19 |
| cleartext_endpoints_are_rejected | 明文端点拒绝 | 给定 `http://` 明文端点（含 `localhost/127.0.0.1`），当解析，则报 `Config` 错 | 2026-09-19 |
| endpoints_with_credentials_query_or_fragment_are_rejected | 带凭据、查询、片段的端点拒绝 | 给定带 `userinfo/query/fragment` 的端点，当解析，则拒绝 | 2026-09-19 |
| unusable_endpoints_report_config_error | 不可用端点报配置错误 | 给定空串、空白、`file:///`无 `scheme` 地址，当解析，则报 `Config` 错 | 2026-09-19 |
| resolves_key_from_keychain | 密钥从 `keychain` 解析 | 给定出厂配置加内存密钥，当解析，则取出密钥原文 | 2026-09-19 |
| missing_key_reports_auth_error | 缺密钥报鉴权错误 | 给定未配置密钥，当解析，则 `EngineAuth` | 2026-09-19 |
| blank_key_counts_as_missing | 空白密钥视为缺失 | 给定空白密钥，当解析，则按未配置报 `EngineAuth` | 2026-09-19 |
| empty_provider_list_falls_back_to_the_factory_entry | 空 `provider` 回退出厂条目 | 给定显式空 `provider_keys` 与出厂 `keychain` 条目密钥，当解析，则回退出厂条目 `gloss/deepseek` | 2026-09-19 |
| adapter_streams_deltas_across_chunk_boundaries | 流式事件（SSE）适配器跨块流式 | 给定同一段（SSE）响应按 1、2、5、整包字节分块，当经 `SseStream` 适配，则增量恒为“光泽”、`[DONE]` 终止、余字节不解析 | 2026-09-19 |
| adapter_delivers_the_done_marker_with_its_finish_reason | 适配器交付带结束原因的终结条目 | 给定增量、声明 `stop` 的结束块与 `[DONE]` 的响应，当经 `SseStream` 消费，则先交增量、再交携带 `stop` 的 `TaskChunk::Done` 终结条目 | 2026-10-11 |
| adapter_delivers_deltas_then_terminates_on_protocol_error | 协议错误先交增量再终结 | 给定增量后跟坏（JSON），当流推进，则先交付增量、随后以 `EngineResponse` 错误终结 | 2026-09-19 |
| an_error_chunk_is_final_even_when_bytes_keep_arriving | 流语义契约·错误条目即终结 | 给定增量后跟坏（JSON）行再跟合法数据行的字节流，当经适配器消费，则恰交增量与错误各一，此后再无产出（错误条目之后的字节不解析） | 2026-10-30 |
| adapter_ends_cleanly_without_the_done_marker | 无 `[DONE]` 关流干净收尾 | 给定无 `[DONE]` 直接关流的响应，当解析，则带增量流按正常结束收尾 | 2026-09-19 |
| adapter_reports_empty_completion_as_failure | 零增量完成报失败 | 给定零增量响应（仅 `[DONE]`、`HTML` 劫持页、或声明 `stop` 却零增量），当解析，则报 `EngineResponse` 错且流即止 | 2026-10-11 |
| request_body_has_the_openai_envelope | 请求体 `OpenAI` 信封 | 给定文本请求，当构造请求体，则 `model/messages/stream/response_format` 四键 `wire` 形态精确匹配且无 `max_tokens` 与 `thinking` 键（长度与思考跟随提供方默认，均不上线） | 2026-10-11 |
| json_mode_switch_is_always_on_the_wire | JSON 契约恒在请求体上 | 给定文本与多模态（图像部件）请求，当构造请求体，则 `response_format` 恒为 `{"type":"json_object"}`（`EngineRequest` 无开关，不做能力协商） | 2026-10-11 |
| text_content_stays_a_json_string_on_the_wire | 纯文本正文 `wire` 上仍是字符串 | 给定纯文本用户消息，当构造请求体，则 `content` 是 JSON 字符串、不升级成 `content` 数组 | 2026-10-10 |
| multimodal_parts_map_to_the_openai_content_array | 多模态部件映成 `OpenAI` `content` 数组 | 给定系统文本消息加仅图像部件的用户消息，夹具为位图（PNG），当构造请求体，则图像部件映 `{"type":"image_url"}` 的 `data:image/png;base64` 数据（URL）且整包形状精确匹配（含 `response_format`） | 2026-10-11 |
| thinking_control_rides_the_wire_only_when_disabled | 思考控制仅关闭时上线 | 给定 `thinking=Follow` 与 `Disabled` 两个请求，当构造请求体，则 `Follow` 无 `thinking` 键（提供方默认不上线）、`Disabled` 携带 `{"type":"disabled"}` | 2026-11-16 |
| maps_http_status_to_error_variants | 超文本传输（HTTP）状态映射错误变体 | 给定 401、403、429、500、`400+JSON`、`400+HTML`，当映射，则分别得 `EngineAuth`、`EngineRateLimited`、`EngineNetwork`、带诊断的 `EngineResponse` | 2026-09-19 |
| json_mode_and_multimodal_rejections_are_recognized_from_diagnostics | 诊断文本识别（JSON）模式与多模态不支持 | 给定 `4xx` 且诊断含 `response_format/json_object` 或 `image/multimodal/vision` 的错误体，当映射，则分别得 `JsonModeUnsupported` 与 `UnsupportedModality`（`JSON` 模式检查在前）；`5xx` 与 401 不做启发式（网络与鉴权优先） | 2026-10-11 |
| retry_delay_covers_only_transient_failures_within_budget | 重试判定只认预算内的瞬态失败 | 给定网络、限流、鉴权、协议、配置五类错误与不同预算余额，当 `retry_delay`，则网络与限流在预算内得固定退避（`Retry-After` 指引封顶生效、低于上限时指引优先），预算耗尽与瞬时之外类别一律 `None` | 2026-10-15 |
| retry_after_header_parsing_accepts_seconds_only | `Retry-After` 只解析秒数形态 | 给定缺失、整数、带空白、超文本传输（HTTP）日期、非数字五种头，当 `parse_retry_after`，则仅整数（含首尾空白）得秒数时长，其余 `None` 回落固定退避 | 2026-10-15 |
| connect_retries_transient_failures_within_the_budget | 连接阶段瞬态失败退避重试并交还余额 | 给定“网络错误后成功”“两次瞬态失败”“鉴权失败”三条尝试脚本，当 `connect_with_retry`，则首条两次尝试后成功、余额清零；次条交还末次错误 `RateLimited`、恰 2 次调用；末条不重试原样上抛 `Auth`、余额不减 | 2026-10-15 |
| first_byte_failures_retry_within_the_shared_budget | 首条目之前的瞬态失败对消费方无感重试 | 给定“立即网络错误的空流 → 增量加`Done` 流”尝试脚本与同形首轮空流，当消费包装流，则失败条目不上交、增量与 `Done` 原序到达、恰 1 次重连调用 | 2026-10-15 |
| first_byte_budget_exhaustion_passes_the_error_through | 重试预算耗尽错误原样交付 | 给定首轮与重试流都以网络错误开场，当消费包装流，则末次错误原样上交后流即止、重连恰 1 次 | 2026-10-15 |
| non_retryable_first_byte_errors_pass_through_without_a_retry | 非瞬态失败不重试直接交付 | 给定首条目即 `EngineAuth` 的流，当消费包装流，则错误原样上交、零重连调用 | 2026-10-15 |
| errors_after_the_first_item_are_never_retried | 首条目之后错误一律不再重试 | 给定“增量后网络错误”的流，当消费包装流，则增量先到、错误原样上交、零重连调用（半截产物不重头重播） | 2026-10-15 |
| first_byte_retry_honors_a_zero_leftover_budget | 首条目重试尊重连接阶段耗尽的余额 | 给定余额为 0 与网络错误开场的首轮流，当消费包装流，则错误原样上交、零重连调用（两阶段共用同一份预算） | 2026-10-15 |
| an_idle_stream_fails_after_the_silence_budget | 静默超时终结悬死流 | 给定交付一个增量后恒 `Pending` 的字节流（虚拟时钟），当静默超过空闲预算后消费，则流以 `EngineNetwork` 终结且不再产出 | 2026-10-15 |
| arriving_bytes_postpone_the_idle_deadline | 字节到达即复位空闲看门狗 | 给定每 30 秒（虚拟时钟）到达一个增量的字节流，当连续消费三个（虚拟 90 秒超出 60 秒预算），则每个增量如期到达、看门狗不触发 | 2026-10-15 |

### `crates/gloss-llm/src/openai/sse.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| decodes_a_complete_stream | 完整流解码 | 给定完整 `OpenAI` 兼容流，当一次喂完，则两段增量加`Done`（`role` 块不产出） | 2026-09-19 |
| done_carries_the_server_finish_reason | `Done` 携带服务端结束原因 | 给定声明 `stop`、声明 `length` 与从未声明结束的三种流，当解码，则 `[DONE]` 条目携带最后一个内容块声明的 `finish_reason`（从未声明为 `None`，交出后清空） | 2026-10-11 |
| decodes_identically_for_every_chunk_split | 任意切分解码一致 | 给定同一段流按 `1..n` 每种字节切法，当逐块喂入，则产出完全一致 | 2026-09-19 |
| byte_by_byte_input_keeps_multibyte_characters_intact | 逐字节输入多字节字符完好 | 给定逐字节喂入，当解析，则多字节字符“光泽”完好 | 2026-09-19 |
| ignores_crlf_heartbeats_and_other_fields | 忽略 `CRLF`、心跳、其它字段 | 给定 `CRLF`、心跳注释、其它（SSE）字段，当解析，则不产出、`data` 行照常出增量 | 2026-09-19 |
| maps_server_errors_by_code | 服务端错误按码分类 | 给定流中错误对象（`rate_limit/invalid_api_key`、其它），当解析，则分类为 `RateLimited/Auth`、带诊断 `EngineResponse` | 2026-09-19 |
| unparsable_payload_reports_bounded_diagnostic | 解析失败诊断有上限 | 给定解析不出的载荷与超长错误消息，当解析，则报 `EngineResponse` 且诊断文本有上限 | 2026-09-19 |
| ignores_a_data_line_without_payload | 空载荷 `data` 行忽略 | 给定空载荷 `data:` 行，当解析，则按心跳忽略 | 2026-09-19 |
| ignores_bytes_after_done | `[DONE]` 后字节忽略 | 给定 `[DONE]` 后的字节，当解析，则一律忽略 | 2026-09-19 |
| done_is_the_last_item_even_with_trailing_lines | 流语义契约·完成条目为末项 | 给定完成哨兵后仍带数据行的完整流，当解码，则条目止于完成条目，其后喂入与收尾都无产出 | 2026-10-30 |
| a_terminal_failure_leaves_no_items_after_it | 流语义契约·失败条目终结流 | 给定超长行洪泛，当解码出失败条目，则其后喂入合法数据行与收尾都无产出 | 2026-10-30 |
| a_failed_chunk_terminates_the_decoder_too | 失败数据块同终结解码器 | 给定不可解析数据块或服务端错误对象，当解码出失败条目，则其后喂入数据行、完成哨兵与收尾都无产出（解码器与端口流语义同构） | 2026-10-30 |
| finish_drops_a_truncated_tail | 残缺尾行丢弃 | 给定残缺半行，当 `finish`，则丢弃且此后字节不再解析 | 2026-09-19 |
| finish_keeps_a_complete_unterminated_line | 完整无换行末行保留 | 给定完整但无换行的最后一行，当 `finish`，则作为增量交出 | 2026-09-19 |
| rejects_a_line_beyond_the_limit | 超长行拒绝终结流 | 给定超 `MAX_LINE_BYTES` 的无换行洪泛，当解析，则协议错误终结流、后续不再解析 | 2026-09-19 |
| error_without_code_reports_the_message_only | 无码错误只报消息 | 给定只有 `message` 的错误对象，当解析，则文案不带前导冒号 | 2026-09-19 |
| error_classification_checks_code_and_type | 错误分类查码与类型 | 给定 `code` 通用而 `type` 细分类（或反之），当解析，则按细分类识别 | 2026-09-19 |

## 集成测试

文件：`crates/gloss-llm/tests/http_wire.rs`（`L1`，本地回环桩服务器驱动真实传输客户端经方言中立的线级入口过完整传输面：分块传输编码、错误信封经真实状态码与响应体、线上自动重试、连接复用与无总超时口径；端点与密钥解析归配置层单测）。

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| chunked_response_streams_deltas_across_transfer_chunks | 分块传输编码逐块流回 | 给定本地桩以传输编码（chunked）按单字节分块交出流式响应（切点落在多字节字符内），当经线级入口消费，则增量恒为“光泽”、请求面（路径、`content-type`、`Bearer` 头、`model` 与 `stream` 线形）经真实套接字核对、恰一次请求 | 2026-11-18 |
| json_error_envelope_maps_real_status_and_body_to_the_error | 真实状态码与响应体进错误信封 | 给定本地桩以真实 400 状态回（JSON）错误信封与（HTML）劫持页两种响应体，当经线级入口发送，则请求以错误终结（不产生流）且诊断分别携带服务端 `message` 与只报状态码、恰一次请求不重试 | 2026-11-18 |
| rate_limited_response_with_retry_after_retries_before_first_byte | 线上限流加 `Retry-After` 首字节前重试 | 给定本地桩首响应回真实 429 状态加零秒的 `Retry-After` 指引头、次响应回流式体，当经线级入口发送，则重试对消费方无感、增量照常流回、桩恰见 2 次请求 | 2026-11-18 |
| sequential_requests_reuse_one_keep_alive_connection | 连接复用共用同一保活连接 | 给定同一客户端对同一桩端点连续两次请求（响应带分块终结帧），当依次消费完两条流，则两次请求落在同一条传输控制协议（TCP）连接上（连接数 1、请求数 2） | 2026-11-18 |
| slow_deltas_keep_streaming_without_a_total_timeout | 慢流无总超时照常流回 | 给定本地桩流式响应中途停顿 1 秒再续流两段，当消费整条流，则两段增量如期到达、无错误终结（口径：流式只设空闲看门狗、无总超时；静默悬死的终结由流级单测以虚拟时钟锁定） | 2026-11-18 |
