# request_body.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），代码内不留指向本文件的指针。

## chat_request_body

- 组装 OpenAI 兼容请求体（`chat/completions` 的形状）。`max_tokens` 只在设限时携带：部分严格端点会拒绝显式的 `null`，缺键才是“不设限”的 wire 语义。
- `response_format` json_object **恒携带**（`EngineRequest` 不加开关）：所有请求都是 JSON 契约，不做能力协商——端点不认时以 4xx 拒绝，由传输层的失败映射识别成 JSON 模式不支持（`GlossError::JsonModeUnsupported`）。

## message_to_value

- 单条消息 → OpenAI 兼容 JSON：role 用小写契约名（`Role` 的 serde 拼写），正文按域形态分派（纯文本即 JSON 字符串，部件即 content 数组）。

## content_to_value

- 消息正文 → wire 值：纯文本即字符串，部件数组即 OpenAI 兼容 content 数组（`{"type":"image_url",…}`）。

## part_to_value

- 单个部件 → wire 值。图像字节在这里才做 base64 编码并拼 `data URL` 前缀（`data:image/png;base64,…`）——域类型只携带原始字节，编码不出请求体构造这一步。
