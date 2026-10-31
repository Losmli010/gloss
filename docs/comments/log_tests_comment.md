# log.rs 测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md“注释纪律”节），测试的验收意图与辅助契约记录在此；`// SAFETY:` 仍留在代码内。

## 测试行为速览（BDD）

已统一迁往 docs/tests/bdd.md（git 追踪，以测试代码为唯一事实源）；本文件只留缘由与形态说明。

## 辅助

- temp_dir：每个用例独占一个临时目录，避免并行跑测试时互相踩。

## 豁免

- 模块级 `#[allow(clippy::let_underscore_must_use)]`：测试对临时目录/文件的清理是尽力而为（`let _ = fs::remove_*`），删除失败不影响任何断言。

## task_span_carries_generation_into_events

- 2026-09-23 这条钉的是“span 定义”而不是 tracing 本身：字段名 `generation`、span 名 `task`、级别 `INFO`。用线程局部的捕获订阅者（`tracing::subscriber::with_default`，不碰全局单例）把格式化后的行收进内存缓冲再断言——变异验证过：把 `info_span!("task", generation)` 的字段去掉，测试当场红。
- 捕获写入器（`CaptureWriter` / `CaptureHandle`）只在本测试模块里用；`with_default` 是线程局部的，测试并行安全。

## probe_line（辅助契约）

- 2026-09-23 断言助手：在 `capture` 捕获到的文本里按消息找那一行。`capture` 只会捕到本测试线程与本次范围的事件，因此“probe”这类独有消息足以定位自己的行；跨线程或用共享调用点断言时改用 `capture_global` 并自己做“字段同时匹配”的筛选。
- 配套约束：JSON 之后断言要按字段路径取值（`value["span"]["generation"]`）；`gloss-app` 侧不引 `serde_json`，用 `"generation":N` 子串断言，其结构正确性由本文件的格式契约测试兜底。

## json_lines_carry_the_structured_contract（变异验证）

- 2026-09-23 变异结果：去掉 `flatten_event(true)` → 契约测试与 span 测试**双双变红**（事件字段退进 `"fields"`，`value["message"]` 与 `value["thread"]` 取不到）；把层换回文本形态**无法用一行变异表达**（返回类型跟着变，编译期就挡住）。
- 未覆盖：`with_span_list(true)` 不会让任何断言变红（`span` 字段仍在，只是多一份 `spans` 数组）——那是体积/冗余取舍，不是正确性契约，故不补断言，记在此处。
- 跨 crate 断言的依据：`gloss-app` 没有 `serde_json`，两处断言用 `"generation":N` 子串；其结构正确性由本测试兜底，所以本测试若被删，那两处的可靠性会一起下降。
