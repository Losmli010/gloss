# report.rs 测试形态说明

## 测试形态（2026-11-01 评测工具链重构步骤 1 后）

- 报告夹具直接构造 `EvalReport`（不再经已删除的 `assemble_report`）：
  `latency_only_shows_when_sampled` 用无样本与有样本两份报告双向断言延迟行
  的出场条件；`generated_at_is_a_numeric_unix_timestamp` 与
  `utc_formatting_is_fixed_and_timezone_free` 钉时间口径。
- `legacy_rows_with_retired_fields_still_parse` 钉历史行兼容面：带已退役字段
  （classify/tasks/judge/skipped）的旧行反序列化成功且退役字段忽略——serde
  缺省忽略未知字段，历史文件不必迁移。
