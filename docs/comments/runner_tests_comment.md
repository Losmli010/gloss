# runner.rs 测试形态说明

## ScriptEngine（live 轨测试的本地脚本引擎，2026-10-12）

- 为什么在 eval 测试里本地重写一份脚本引擎而不用 core 的 `MockEngine`：core 的测试桩挂在 `pub(crate) mod stubs`（lib.rs 以 `#[path]` 并入 lib 编译），不出库；gloss-eval 受依赖方向约束只能消费 core 的公共 API，故测试自带一份最小 `AiEngine` 实现（每次 `execute` 按脚本序弹出一段 chunk 列表作为流，`calls` 计数器供调用次数断言）。
- 脚本序语义：`new` 保存插入序，`execute` 依序弹出（首个条目喂第一次调用）——`live_track_records_rounds_and_repair_metrics_through_the_production_loops` 的 6 段脚本与分类/词卡/句译/代码各轮一一对应，脚本不足会在测试内 panic（`script exhausted`）而非静默错位。
