# stubs/engine.rs 决策记录（tests/stubs/）

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 测试桩按生产代码对待（AGENTS.md“注释纪律”节）；本文件承接自代码外迁的注释。

## MockEngine

- 脚本引擎：按预置序列产流，支持整体失败、chunk 间延迟与调用计数。
- 克隆共享脚本与计数（内部 `Arc`）——同一实例注入多处时断言的是同一台引擎的总调用量。

## Inner（字段契约）

- `execute_failure`：execute 的整体失败——Some 时 execute 直接返回 Err，不产流。
- `once_failures`：一次性失败队列，每次 execute 消费一个，耗尽后照常产流。
- `execute_panics`：一次性 panic 队列，每次 execute 消费一个（驱动“后台 panic 被 tokio 捕获”的验收）。
- `scripts`：按调用次序消费的脚本队列——第 n 次 execute 取第 n 份，耗尽后重放最后一份（单份脚本即 `with_chunks` 的重放语义）。
- `last_script`：耗尽后重放的最后一份脚本。
- `chunk_delay`：相邻 chunk 之间的延迟，模拟真实流式节奏。
- `calls`：execute 被调用的次数。
- `requests`：收到的全部请求（修复轮对话形状的断言面）。

## MockEngine::new

- 无延迟、脚本为空、不失败的默认引擎。

## with_chunks

- 注入流式增量脚本（可含 `Err` 模拟流中失败）。脚本作用于共享状态，克隆体同样可见；每次 execute 重放同一份。

## with_script_queue

- 注入按调用次序消费的脚本队列：每份脚本直接是 `TaskChunk` 流（可含 `Done` 结束帧与 `Err`），第 n 次 execute 取第 n 份，耗尽后重放最后一份。作用于共享状态，克隆体同样可见。

## with_chunk_delay

- 注入 chunk 间延迟（作用于共享状态，克隆体同样可见）。

## with_execute_failure

- 注入 execute 整体失败。

## with_execute_failure_once

- 注入一次性 execute 失败：仅下一次 execute 返回 Err，之后照常产流（“失败落 Error 态可重试”的验收驱动）。

## with_execute_panic

- 注入一次 execute panic：下一次 execute 的任务 future 在首次 poll 时炸掉，之后照常产流（panic 是这里的注入语义，tokio 桥的 catch_unwind 是被测行为；一次性语义让“循环存活”可与正常任务同引擎验证）。
- 两处 `#[allow(clippy::panic)]` 的豁免缘由：测试桩里 panic 即注入语义（见方法文档）。

## call_count / recorded_requests

- `call_count`：引擎被调用的次数（“缓存命中不调引擎”断言用）。
- `recorded_requests`：收到的全部请求快照（按到达次序；修复轮对话形状的断言面）。

## ChunkStream

- 逐 chunk 吐脚本的流：每个 chunk 之前等待 `delay`（首个 chunk 也等，统一模拟“首 token 延迟 + 后续节奏”）。
- `poll_next`：有下一个 chunk 才需要等待；等待完成后吐 chunk。
