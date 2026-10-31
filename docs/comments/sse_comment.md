# sse.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## 解码策略（UTF-8 跨块安全）

- 2026-09-19 UTF-8 不会被分块切坏的原因：只解码以 `\n` 结尾的完整行，而 0x0A 不会出现在 UTF-8 多字节序列内部，行内字节必然完整。

## SseDecoder（finish_reason 捕获，2026-10-11，契约校验与重试方案 §5）

- `choices[0].finish_reason` 先于 `[DONE]` 随最后一个内容块到达，解码器只保留最近一次声明、在 `[DONE]` 上 `take` 交出（`SseItem::Done` 携带）：流式协议里“结束原因”与“流结束”分属两帧，合并交出让消费方只看终结条目。
- 只认 `choices[0]`：本客户端每次只发一条 user 消息，choices 恒为一条（与增量提取同一假设）。
- 连接关闭（无 `[DONE]`）不产出 Done：`finish()` 只回收尾巴增量，结束原因未知就是 None，不伪造。

## finish_reason 随 choice 所有权带出（P1-1 clone 削减，2026-10-13）

- 原 `decode_data` 先 `choices.first()` 借用取 `finish_reason.clone()`，
  再 `into_iter` 找增量——结束原因被复制一次。改为单趟 `enumerate` +
  `find_map`：首条 choice 的结束原因随所有权带出（index 守卫保证只认
  第一条，与原 `choices.first()` 语义一致），增量取第一条携带 content
  的 choice，遍历顺序与终止条件不变。

## 常量上限（MAX_DIAGNOSTIC_CHARS / MAX_LINE_BYTES）

- 诊断文本上限：错误消息会进 UI 与日志，服务端给的长文本没有价值；`truncate` 按字符截断，避免切坏多字节字符（省略号带 `i18n:allow` 豁免留在代码内）。
- 单行上限：OpenAI 兼容载荷恒为单行 JSON，超限即协议异常。没有这条，一个只发字节不发 `\n` 的服务端（或坏网关）就能让 `pending` 无界增长。

## SseItem

- 一轮解码的产出。
- `Done`：服务端声明的流结束（`data: [DONE]`），携带捕获到的 `choices[0].finish_reason`（整条流从未声明则为 `None`）；`finish_reason` 为结束原因原文（`stop` / `length` / `content_filter`…）。
- `Failed`：流中错误——服务端错误对象，或解析不出的协议内容。

## SseDecoder

- 行缓冲解码器：`push` 喂入任意切分的字节，返回本次可确定产出的条目。
- 字段 `pending`：尚未凑成完整行的尾巴（跨网络分块）。
- 字段 `done`：已见到 `[DONE]`，之后的字节一律忽略。
- 字段 `finish_reason`：最近一次捕获的 `choices[0].finish_reason`（结束原因先于 `[DONE]` 随最后一个内容块到达），交出后清空。

## SseDecoder::push

- 喂入一段字节（分块边界任意），返回可产出的条目（顺序即流序）。
- 只处理完整行，剩下的尾巴留给下一块——这也是跨块 UTF-8 不碎的保证。
- 攒下的尾巴超限就是协议异常：终结流，别继续为它占内存。

## SseDecoder::finish

- 连接关闭时收尾，返回尾巴里还能交出去的增量。
- 服务端没给 `[DONE]` 就关流是兼容端点上的常见收尾方式（不算错误）。尾巴里可能是“完整但没被换行终止的最后一行”——端点 flush 完最后一块数据行直接关连接时就是这种形态，丢了就是一个真实增量；解析不出来的一律忽略（截断不该升级成任务失败）。

## decode_line

- 解一行：返回 None 表示这行不产出任何东西（分隔空行、心跳、其它字段、以及只带 role 的块）。
- 无载荷的 `data:` 行（个别兼容实现拿它当心跳）不产出东西。

## decode_data

- 单个 `data:` 载荷：捕获 `choices[0].finish_reason`、取增量内容、识别服务端错误；`None` = 这行不产出东西（例如只带 role 的首块）。
- 本客户端每次只发一条 user 消息，choices 恒为一条；结束原因只认第一条 choice，增量取第一条携带 content 的 choice（一并把结束原因随 choice 所有权带出，不再复制字符串）。

## map_api_error

- 服务端错误对象 → `GlossError`：能识别的按语义分类，其余按协议错误透出服务端给的诊断文本（只取 `error.message`，不转述整个响应体）。
- `code` 与 `type` 分别匹配：OpenAI 用 type、DeepSeek 等用 code，兼容实现可能只填其中一个，或 code 是通用值而细分类在 type 里。
- 服务端只给 message 时别造出“: message”这种带前导冒号的文案。

## wire 形状结构体（StreamChunk / StreamChoice / StreamDelta / ApiError）

- `StreamChunk`：流式响应的一块，OpenAI 兼容 `chat.completion.chunk`。
- `StreamChoice`：一条候选回复（本客户端只用第一条）；`finish_reason` 通常为 `None`，只在流收尾的内容块上非空（`stop` / `length` / `content_filter`…）。
- `ApiError.kind`：OpenAI 用 `type` 字段，DeepSeek 等兼容实现两者都给。
