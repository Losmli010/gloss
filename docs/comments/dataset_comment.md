# dataset.rs（gloss-eval）决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## ClassifyCase / TaskCase（数据集条目）

- 分类数据集条目：输入文本 + 期望判定的任务类型。
- `ClassifyCase` 字段：`id` 稳定标识（夹具与报告按它对齐）；`text` 待分类的选区文本；`expected` 期望判定的任务类型。
- 任务数据集条目：输入 + 期望的结构化字段（reference）。`TaskCase` 字段：`id` 稳定标识；`kind` 任务类型（决定 prompt 与结构化契约）；`text` 任务输入文本；`reference` 期望的结构化字段（与 `TaskKind` 对应的产出契约同形）。

## Fixture（重放夹具条目）

- 一次真实交互的修复轨迹——每轮一组 SSE 文本增量（按时间顺序拼接即该轮原文），轮序与生产的修复循环一致（违约轮在前、修复轮在后）。单轮 = 一次通过；末轮为空 = 预算耗尽的违约收尾（生产按降级产物处理）。异常轨迹（空回复/散文/缺 note/kind 越清单）是按生产修复形态维护的受控样例，与录制回写的轨迹同一格式。
- 字段：`id` 对应的数据集条目 id；`rounds` 每一轮的文本增量（修复轮由违约触发；轮序即生产执行序）。

## load_classify

- 解析分类数据集；空文件、坏行（带行号）、重复 id 都是硬错误。

## load_task

- 解析任务数据集；reference 必须能对上 kind 的结构化契约（必需键在场）。

## load_fixtures

- 解析重放夹具；零轮轨迹（连首轮都没有）是硬错误——空回复轮是 `deltas = []`，与“没有轨迹”不同。

## required_fields

- 任务 kind 的 prompt 输出契约必需键（评测从严于生产的接受条件：生产对缺失字段有兜底，评测按契约要求其在场；允许 null 的键——phonetic / code_language null——不算必需；`note`（义）单独由判定的 note_present 计，不与疏证字段混在一张表）。三行同构契约下 `structured.examples` 是唯一恒在的疏证键（图像解读同形落 Plain，旧 interpretation 名退役）。
