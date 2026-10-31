# failure.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），代码内不留指向本文件的指针。

## MAX_ERROR_BODY_BYTES

- 错误响应体的读取上限：只要 `error.message`，多余字节没有价值，而错误网关的劫持页可能极大（截断后 JSON 解析失败会退化成“只报状态码”，与既有非 JSON 路径一致）。

## map_transport_error

- 传输层错误 → `GlossError`：连不上/超时/连接中断都可重试。
- builder 错是本地构造失败（非法 header 等）——重试永远不会成功，报成 EngineNetwork（可重试）会把用户引到错误的方向。

## 诊断标记常量（JSON_MODE_MARKERS / MULTIMODAL_MARKERS）

- JSON 模式不支持的诊断标记（小写比较）：请求体恒带 `response_format` json_object，4xx 诊断文本提到它就是端点不认这个开关。
- 图像输入不支持的诊断标记（小写比较）。

## map_failure

- HTTP 状态 → `GlossError`（错误分类表）。其余 4xx 按服务端诊断文本做两个**启发式**识别（环境问题，重发同请求无益，不落可重试的协议错误）：诊断提到 JSON 模式开关 → `GlossError::JsonModeUnsupported`；提到图像输入 → `GlossError::UnsupportedModality`。JSON 模式检查在前：它是请求信封层的拒绝，命中时对所有任务都成立。识别不出才落 `GlossError::EngineResponse`。

## server_message

- 从错误响应体里取服务端诊断文本（仅 `error.message`，OpenAI 兼容形状）。
- 上限与 SSE 诊断一致：错误文本会进 UI 与日志。
