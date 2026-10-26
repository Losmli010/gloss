# report.rs（gloss-eval）决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 测试形态说明见 report.rs_tests_comment.md。

## EvalReport

- 一次评测运行的完整报告（`target/eval/<mode>.json` 的形状）。
- 字段：`mode` 运行模式（replay / live）；`generated_at` 报告生成时间（UNIX 纪元秒，不引入时间库依赖）；`classify` 分类轨指标（replay 只统计有夹具的条目）；`tasks` 任务轨指标（数据集名 → 指标）；`latency` 延迟统计（仅 live 轨有样本；replay 恒 None）；`judge` judge 摘要（仅 live 轨且 `GLOSS_LIVE_JUDGE=1` 时有值）；`skipped` 数据集条目中无夹具覆盖、被跳过的数量；`context` 采集上下文（历史行的 upsert 键用；旧报告 JSON 无此段照常解析）。

## EvalContext

- 采集上下文（可选；旧报告 JSON 无此段照常解析）。`commit` git 短 SHA（流水线经 GLOSS_EVAL_COMMIT 注入；本地运行缺省）；`source` 采集来源（GLOSS_EVAL_SOURCE 注入，如 "pipeline" / "local"）。

## ClassifyStats / ConfusionRow / TaskStats / LatencySummary / JudgeSummary

- `ClassifyStats`：分类轨的序列化视图（混淆矩阵拍平成行）。`metrics` 各指标数值；`accuracy` 准确率（0..1）；`fallback_rate` 回退率（0..1）；`confusion_rows` 混淆矩阵行（期望 → 实际 → 次数，仅计错误路径）。
- `ConfusionRow`：混淆矩阵一行——期望 kind、实际 kind、次数。
- `TaskStats`：单个任务数据集的指标视图——数据集名（文件名去扩展名）+ 指标数值。
- `LatencySummary`：延迟摘要（毫秒）——count、p50、p95。
- `JudgeSummary`：judge 摘要（1–5 分的逐条均值；解析不出分数的条目不计入）。`attempted` 是尝试评分的条数（含引擎失败与解析失败）——`attempted > count` 即「judge 启用但部分失败」；旧报告 JSON 无此字段，反序列化默认 0（`attempted == 0` 时渲染回退到只报 count，不显示失败数）。

## render_markdown 及各渲染函数

- `render_markdown`：渲染 Markdown 表格（结构沿用 bench-summary.py：总述行 + 指标表 + 混淆矩阵表）。
- `render_summary`：总述行——模式、时间与覆盖面（评测/跳过条数）。
- `render_metric_rows`：指标表——分类一行组、每任务数据集一行组、延迟与 judge（在轨才出）。
- `render_classify_rows`：分类轨的指标行——accuracy、无效 JSON 率、回退率、修复轮次。
- `render_task_rows`：任务轨的指标行——每个数据集五行（契约 JSON 率、note 在场率、字段完整率、降级率、修复轮次）。
- `render_latency_and_judge_rows`：延迟与 judge 的指标行——各自在轨才出。
- `render_confusion`：混淆矩阵表——仅分类轨在轨且有错误路径时出。

## format_utc / format_utc_short / civil_from_days

- `format_utc`：UNIX 秒 → UTC 的 `YYYY-MM-DD HH:MM`。一律按 UTC 固定换算——跟随本地时区会让同一时间戳在不同机器漂一天。
- `format_utc_short`：UNIX 秒 → UTC 的 `MM-DD HH:MM`（趋势图 x 轴刻度用）。
- `civil_from_days`：UNIX 天数 → UTC 民用日期（Howard Hinnant 的 civil_from_days 算法，零时间库依赖）。

## repair_line

- 修复轮次行的内容：轮次总数加违约类别计数（BTreeMap 按类别序稳定迭代）；无违约轮时只报轮次。

## TrackOutputs / assemble_report

- `TrackOutputs`：各轨的结算产物（组装报告的全部轨道输入）——分类可选、任务按数据集，延迟与 judge 在轨才出。
- `assemble_report`：把评分器的输出组装成报告（runner 与测试共用）——混淆矩阵拍平、比率预计算、时间戳取 UNIX 纪元秒（报告不带额外的时间库依赖）。

## write_report

- 把报告写成 `report-dir/<mode>.json` + `<mode>.md`，返回 md 文本。
