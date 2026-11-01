# report.rs（gloss-eval）决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 测试形态说明见 report.rs_tests_comment.md。

## EvalReport

- 一次评测运行的完整报告（`target/eval/<mode>.json` 的形状）。
- 字段：`mode` 运行模式；`generated_at` 报告生成时间（UNIX 纪元秒，不引入
  时间库依赖）；`latency` 延迟统计（有实跑样本才有值）；`context` 采集上下文
  （历史行的 upsert 键用；旧报告 JSON 无此段照常解析）。
- 2026-11-01 评测工具链重构步骤 1：`classify`/`tasks`/`judge`/`skipped` 字段
  随重放轨、独立分类轨、旧实跑轨与旧评审单总分删除——serde 缺省忽略未知
  字段，旧历史行照常解析（分组聚合的新指标模型归重构计划后续步骤在本模块
  重建）。

## EvalContext

- 采集上下文（可选；旧报告 JSON 无此段照常解析）。`commit` git 短 SHA（流水线经 GLOSS_EVAL_COMMIT 注入；本地运行缺省）；`source` 采集来源（GLOSS_EVAL_SOURCE 注入，如 "pipeline" / "local"）。

## LatencySummary

- 延迟摘要（毫秒）——count、p50、p95。

## render_markdown 及各渲染函数

- `render_markdown`：渲染 Markdown 表格（结构沿用 bench-summary.py：总述行 + 指标表）。
- `render_summary`：总述行——模式与时间。
- `render_latency_rows`：延迟的指标行——有样本才出。

## format_utc / format_utc_short / civil_from_days

- `format_utc`：UNIX 秒 → UTC 的 `YYYY-MM-DD HH:MM`。一律按 UTC 固定换算——跟随本地时区会让同一时间戳在不同机器漂一天。
- `format_utc_short`：UNIX 秒 → UTC 的 `MM-DD HH:MM`（趋势图 x 轴刻度用）。
- `civil_from_days`：UNIX 天数 → UTC 民用日期（Howard Hinnant 的 civil_from_days 算法，零时间库依赖）。

## write_report

- 把报告写成 `report-dir/<mode>.json` + `<mode>.md`，返回 md 文本。
