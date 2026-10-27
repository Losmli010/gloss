# detail.rs（gloss-eval）决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## 结算函数族（classify_parts / task_parts / rounds_with_track）

- 结算函数（`classify_parts` / `task_parts`）一次校验同时产出明细与指标侧产物（`ClassifyVerdict` / `TaskVerdict` / `ViolationTrack`）：轨迹逐轮只跑一遍生产校验器，明细与报告指标天然同源；两者的计数一致由 runner 的汇总测试锁定。
- `rounds_with_track` 是轨迹的单遍折算：逐轮跑生产校验器，同时产出逐轮标签与聚合违约统计（`ViolationTrack` 同口径：修复轮次 = 轮数 − 1，末轮违约也计入）。
- `classify_parts`：分类样例的结算——明细 + 判定 + 违约轨迹，一次校验三方消费（指标入账、明细落行、报告聚合）。
- `task_parts`：任务样例的结算——明细 + 判定（完成态与轨迹一次折算）。`group` 由调用方给出（数据集名在 runner 的数据集清单里，条目自身不携带）。

## RoundDetail / ClassifyOutcome / TaskOutcome（明细视图）

- `RoundDetail`：一轮的明细——完整回复原文 + 该轮违约短名（契约轮为 `None`）。
- `ClassifyOutcome`：分类判定的可序列化视图（与 `ClassifyVerdict` 同矩阵，供检视页徽标）。
- `TaskOutcome`：任务完成态的可序列化视图（`TaskVerdict` 的四级标志 + 末轮 note）。`note` 是末轮 note 正文（降级为空串）——检视时直接读产出，不必自行解析轮文本。

## CaseDetail

- 一条样例的完整明细。字段语义：`group` 分组名（分类轨为「分类」，任务轨为数据集名如 task_translate_word）；`text` 输入文本（选区原文 / 代码等）；`expected` 期望判定（分类）或条目 kind（任务）；`repairs` 修复轮次（生产 `RunOutput.repairs` / `ClassifyDecision.repairs` 同口径）。
- `engine_error` 标记 live 轨的请求本身失败（网络/限流）：与模型行为分账，检视页按「引擎失败」而不是模型错误展示。

## engine_error_detail

- live 轨的引擎失败明细：无轮次无判定，只留输入与「请求失败」标记（指标侧分类轨单列 engine_errors、任务轨并入降级，明细统一按本标记展示）。

## CLASSIFY_GROUP

- 分类轨的分组名（检视页与 runner 共用同一常量，过滤键不漂移）。
