# message 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## 模块的由来（2026-10-30，重构计划阶段 3 自 prompt.rs 拆出）

- 重构计划阶段 3 拆分触及五百行上限的 `prompt.rs`：消息契约类型
  （`Role`、`MessageContent`、`ContentPart`、`ChatMessage` 及构造器）
  按计划切点整段迁入本文件，类型与构造器逐字移动；既有决策随代码
  迁入本文件（原 `prompt_comment.md` 各节）。

## MessageContent 与 ContentPart（2026-10-10，剪贴板图片方案 §5.4）

- 为什么 `ChatMessage` 不再 `Serialize`：此前域形态与 wire 形态恰好重合（content 就是 JSON 字符串），`messages_serialize_to_openai_shape` 把这个巧合钉在了序列化层。多模态引入后两形态分叉（content 字符串 vs OpenAI content 数组），wire 组装整体搬到 `engine::llm::chat_request_body`（`message_to_value`/`content_to_value`/`part_to_value`），该序列化测试随之删除、由请求体快照测试接管。
- 为什么域类型不携带 base64：编码是纯传输细节且体积膨胀 4/3 倍；域部件 `ContentPart::ImagePng(Arc<[u8]>)` 只搬原始字节，保持 `PartialEq` 按字节可比、Arc 跨线程零拷贝共享，也不存在「编码态与原始态并存」的双形态漂移。data URL 前缀与 base64 编码只在 `part_to_value`（请求体构造处）发生。
- 备选与落选：让 `ChatMessage.content` 直接存 `serde_json::Value`——域层被 wire 词汇（`image_url`、data URL）侵蚀，prompt 层就得 import 传输知识，分层反向。

## Role / MessageContent / ContentPart / ChatMessage（源码注释迁入）

- `Role`：OpenAI 兼容消息角色。`System` 系统指令、`User` 用户输入、`Assistant` 模型回复（修复轮带回上一次的坏回复）。
- `MessageContent`：消息正文的域形态，纯文本或多模态部件数组。**不携带 base64**——部件数组映成 OpenAI 兼容 content、图像字节编码成 data URL 都归传输层（`engine::llm` 的请求体组装），本枚举只搬原始载荷。`Text` 纯文本正文；`Parts` 多模态部件数组（图像任务的用户消息）。
- `ContentPart`：多模态内容部件，域形态只携带原始载荷（PNG 保持字节，不预编码）。`ImagePng` PNG 图像字节。
- `ChatMessage`：OpenAI 兼容消息的域形态，`role` + `content`（wire 形状由传输层组装）。构造子：`system`/`user`/`assistant` 纯文本正文，`user_parts` 多模态部件数组（如图像任务）；`assistant` 修复轮用它带回上一次的坏回复。
