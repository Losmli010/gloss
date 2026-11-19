# request_body.rs 决策记录（gloss-llm openai 方言）

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），代码内不留指向本文件的指针。

## chat_request_body

- 组装 OpenAI 兼容请求体（`chat/completions` 的形状）。回复长度不设限（无 `max_tokens` 键，字段已撤出 `EngineRequest`）——部分严格端点会拒绝显式的 `null`，缺键即“不设限”的 wire 语义；任务输出长度不可预算，截断交给流式侧边界与服务端收尾形态。
- `response_format` json_object **恒携带**（`EngineRequest` 不加开关）：所有请求都是 JSON 契约，不做能力协商——端点不认时以 4xx 拒绝，由传输层的失败映射识别成 JSON 模式不支持（`GlossError::JsonModeUnsupported`）。
- 思考控制（2026-11-16）：`thinking` 仅在 `ThinkingMode::Disabled` 时携带（`{"type":"disabled"}`）；`Follow` 缺键——“跟随提供方默认”的 wire 语义就是不出现在请求体上。
- 下标改写（2026-11-20，架构重构 C2，崩溃家族门禁）：思考开关的键写回改 `as_object_mut` 后 `insert`（请求体恒为对象，与下标写回同语义），下标形态消失。

## message_to_value

- 单条消息 → OpenAI 兼容 JSON：role 用小写契约名（`Role` 的 serde 拼写），正文按域形态分派（纯文本即 JSON 字符串，部件即 content 数组）。

## content_to_value

- 消息正文 → wire 值：纯文本即字符串，部件数组即 OpenAI 兼容 content 数组（`{"type":"image_url",…}`）。

## part_to_value

- 单个部件 → wire 值。图像字节在这里才做 base64 编码并拼 `data URL` 前缀（`data:image/png;base64,…`）——域类型只携带原始字节，编码不出请求体构造这一步。

## 分包迁出核心库（2026-11-18，架构重构方案 C3）

- 本文件随传输适配器整体迁入传输分包（gloss-llm）；记录沿迁（符号锚定不变），
  代码内容除包内路径与可见性（方言条目 `pub(crate)`）外零改动。
