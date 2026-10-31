# llm.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），代码内不留指向本文件的指针。

## chat_request_body（抽成纯函数）

- 2026-09-19 抽成纯函数是为了让它可断言：model / messages / stream 三个键是发往付费端点的硬契约，写错了只有真机跑才会发现。

## 请求构建（不跟随重定向）

- 2026-09-19 不跟随重定向的原因：OpenAI 兼容端点不需要，且避免 Authorization 在“同 host:port 的 https→http 降级”这类窄条件下被转发出去。

## resolve（端点与密钥分两条路径）

- 2026-09-19 两者不共用返回值的理由：否则污染分析会把 URL 也算成“可能含密钥的数据”，而真正该守的是“密钥只走 HTTPS 端点”这一条。

## 超时策略

- 2026-09-19 流式响应不设总超时的理由：长回答是正常情形，总超时会误杀；取消由调用方的 `CancellationToken` 竞速完成（future 被丢弃即断链）。

## 多模态消息的 wire 映射（2026-10-10，剪贴板图片方案 §5.4）

- 三层分派（`message_to_value` → `content_to_value` → `part_to_value`）：role 沿用小写契约名；**纯文本消息 wire 上仍是 JSON 字符串**、不升级成 content 数组——部分 OpenAI 兼容端点对数组形式的纯文本兼容度不一，已有文本路径的 wire 形态一字不动；只有 `Parts` 才映成 content 数组（`{"type":"text"}` / `{"type":"image_url","image_url":{"url":"data:image/png;base64,…"}}`）。
- base64 编码与 data URL 前缀只在请求体构造处发生（域类型不带 base64，见 prompt_comment.md）。
- 快照测试的夹具 PNG：手工构造的 1×1 合法 PNG（61 字节），整包 base64 data URL 钉进断言——model id 是不透明字符串，另一个信封测试刻意沿用 "deepseek-chat" 佐证其不依赖出厂值。

## chat_request_body（response_format 恒携带，2026-10-11，契约校验与重试方案 §5）

- json_mode 破坏式常开，`EngineRequest` 不加开关（方案 §1.5）：所有请求都是 JSON 契约，分类与任务同规；不做能力协商——备选（开关 + 每端点能力表/探测请求）要么多一次付费往返，要么多一份要维护的能力状态，而“端点不支持 json_object”是环境问题，正确的出口是失败卡 + 打开设置，不是静默降级重发。
- 端点不认时的拒绝形态不可假定（4xx + 诊断文本措辞各家不一），由 `map_failure` 的启发式识别（见下节）。

## map_failure（诊断文本启发式：JSON 模式 / 多模态不支持，2026-10-11，方案 §2）

- 两个新类别都只做**启发式**识别（4xx 且诊断文本小写包含标记），不做第二层兜底：能拿到手的只有 `error.message`，精确识别要维护每家端点的错误码表，收益撑不起成本；识别不出仍落带诊断的 `EngineResponse`。
- JSON 模式检查排在多模态之前：`response_format` 是请求**信封**层的拒绝（命中时对所有任务成立），多模态只对图像任务成立。
- 标记选取：`response_format` / `json_object` 是开关自身的名字，出现在无关 4xx 里的概率可忽略；`image` / `multimodal` / `vision` 覆盖常见措辞，误报面是“提到 image 的其余 4xx”——把方向仍指向“换模型服务”，与真实因由的修复动作一致，代价可接受。
- 优先级不变：401/403（鉴权）、429（限流）、5xx（网络）先于启发式——这三类有既定语义与出口，不能被诊断文本里的字样劫持（如 401 文案提及 response_format 仍是鉴权问题）。

## SseStream（终结条目 TaskChunk::Done，2026-10-11，方案 §5）

- `[DONE]` 从“静默置 finished”升级为显式终结条目并携带 `finish_reason`：编排层要结束原因，而流条目里此前没有携带它的位置。
- 零增量 + 声明正常结束仍报 `EngineResponse("empty completion")`（与既有语义一致）：完成态解析以 raw 为唯一真相源；空回复“转成 EmptyReply 违约”归修复轮步骤（方案 §5/§6），本步不改失败语义。
- 连接关闭（无 `[DONE]`）不产出 Done：结束原因未知，宁可 None 也不猜；流自然耗尽，编排层 `finish_reason` 落 None。

## EMPTY_COMPLETION 单点常量（2026-10-12，契约校验与重试方案 §5）

- 为什么把“empty completion”提为 `pub(crate)` 常量：该诊断原本是适配器内的字面量；修复轮步骤让 run 循环要按它把零增量完成识别为 `Violation::EmptyReply`（方案定稿“不改 GlossError 变体”）——两处按字符串协作就必须共享同一常量，改一处措辞另一处即编译期可见，不存在措辞漂移。

## 常量（CONNECT_TIMEOUT / CHAT_COMPLETIONS_PATH）

- 建连超时不含响应与流式读取（流式读取只设空闲超时，见 transport_comment.md）。
- 聊天补全路径为 OpenAI 兼容的 `chat/completions`。

## LlmClient（结构体字段）

- `client`：复用连接池的 HTTP 客户端（建连超时在这里定）。
- `config`：端点等配置快照（模型不经这里）。
- `store`：密钥直查（keychain）。

## LlmClient::new

- 组装：配置句柄与存储都由组装点注入（句柄给端点，存储给密钥）。
- 带一个明确的 UA：部分前置 CDN 对空 UA 返回 403，而 403 在这里会被映射成 EngineAuth，用户会被引去重填密钥——方向完全错了。
- 不跟随重定向（缘由见上“请求构建”节）。

## AiEngine::execute

- `reqwest::Client` 内部是 Arc 连接池：克隆只是递引用计数（零堆分配），为满足 'static future 的所有权要求，勿改回传引用或重建客户端。
- 空 messages 直接报错：渲染层（core 编排）保证至少一条 user 消息；空请求是接线错误，不该把一个空 body 发给付费端点。
- 空白模型同样是配置问题：发了也是必然 400 的请求，还会把病因藏进服务端的错误文案里（手改配置或设置页存了空串）。
- 端点与密钥分两条解析路径，不共用返回值（缘由见上“resolve”节）。
- 单次尝试闭包：连接阶段与首个流条目之前的重试共用同一形状，重试即再调一次；端点、密钥与请求体都在 Arc 里，克隆只是递引用计数。

## resolve_endpoint

- 请求端点由配置快照解析 `{base_url}/chat/completions`；结构性规则（非空/https/无凭据/无 query·fragment）与设置页逐字段校验共源：`crate::config::validate_base_url`。
- 用 Url 设路径而不是字符串拼接：手改配置的尾斜杠写没写都得到同一结果。

## resolve_api_key

- API 密钥按 provider 条目的 keychain 标识**每请求直查**（不缓存，只进请求头）。未配置时返回 `GlossError::EngineAuth`，由 UI 引导去设置页。
- trim 后再返回：从终端/文件复制粘贴进来的尾随换行会让 Authorization 头非法（reqwest 报 builder 错 → 被当成网络问题的永久失败），尾随空格则变成服务端 401。

## 空闲超时与首字节前自动重试（2026-10-15，feat PR）

### 为什么修复轮不加间隔、重试只放传输层

- 回复违约是内容级失败（模型输出不合规），等待不会改善下一轮输出；间隔只会
  恶化划词场景最核心的感知延迟（L3 预算按最差情况判定，sleep 直接计入）。
  触发频率是每次用户划词、预算 1+3 轮，无拥塞风险需要退避。
- 自动重试只放 LlmClient（传输适配器）而非状态机/编排层：MockEngine 与
  gloss-app 时序测试不受影响，`task_stage_engine_errors_are_never_retried`
  等“编排层不重试”的既有契约不变——从编排层看，execute 仍是要么成功要么
  失败的一次调用。

### 重试口径

- 只重试 `EngineNetwork | EngineRateLimited`（与 model.rs 文档注释、machine.rs
  失败卡按钮的口径同源）；鉴权/参数/环境类拒绝重发无益。
- 只在“首个流条目之前”：零增量丢失无物，重头请求对消费方无感；首条目交付
  后的错误一律放行——中途断流多为确定性故障，且重头重播会让已渲染的流式正文
  错乱（与编排层 EmptyReply 原样重发的既有语义互不干扰）。
- 预算 RETRY_ATTEMPTS=1 由连接阶段与首条目阶段共用（connect_with_retry 把余
  额传给 FirstByteStream），最差情况延迟增量有界（一次退避 + 一次往返）。
- Retry-After 只取秒数形态、封顶 1s：更长的等待不自动进行，失败卡按钮在手。

### 空闲超时取值与形状

- IDLE_TIMEOUT=60s 统一一档：首字节等待（推理类模型）远长于流中间歇，拆两档
  留待真实数据支持；总超时不可用——长回复合法地跑满全预算。
- 形状是“字节级看门狗”装在 SseStream：每次收到字节复位，到点未复位即判悬
  死，流以 EngineNetwork 终结（首个 Err 终结流的端口契约不变，消费方失败卡
  路径零改动）。改动前悬死连接会无限挂着直到用户取消——这是严格改进。
- 超时错误映射 EngineNetwork 而非新错误类别：UI 已有该类别的出口（重试按钮），
  不扩错误面。

### 审查后记（code-reviewer，PR #194）

- P2 已修：首条目重试分支补 warn（与 connect_with_retry 同形），不再静默吞掉
  被替换的错误。
- P3 已修：请求体 Arc 化（多模态 base64 可达 MB 级，重试闭包克隆退化为递引
  用计数）；FirstByteStream 文档收紧——重连只此一次、失败原样上交，不链式
  重判（既描述现行预算下的真实行为，也是对失败端点的重试上限）。
- P3 留观（不改代码）：IDLE_TIMEOUT=60s 覆盖推理模型首字节是断言而非证据，
  上线后按真实 provider 的首字节静默分布校准，届时同步 bdd.md 口径。

### 重试路径克隆的归零（PR #194 追加）

- 复盘：端点/密钥字符串（url/key）在重试闭包外层与每次调用各深拷贝一次，
  共 4 份副本，密钥还多驻留了敏感字符串。
- 方案：端点、密钥、请求体打包成 Resolved 结构体 Arc 共享。'static 重连
  future 必须持有数据（AiEngine 端口契约），这是不可消除的下限；Arc 之后
  所有克隆（含 attempt 闭包克隆）都是引用计数递增，深拷贝归零。
- client.clone() 与测试桩的 Arc 克隆本来就是引用计数（reqwest::Client 内部
  Arc，原代码已注明），不动。

## Resolved（2026-10-16，内存优化方案）

- 为什么 body 从 serde_json::Value 改为一次性序列化的 Bytes:.json(&Value) 在每次传输尝试（send_once）都把整个 Value 重新序列化一份线上字节——图像任务的 base64 体可达 MB 级，重试与修复轮各付一份临时缓冲；Bytes 克隆只递引用计数，重试零拷贝。content-type 需手动补（.json 的另一半职责），线上字节与旧路径逐位一致。
