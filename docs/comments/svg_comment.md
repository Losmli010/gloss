# svg.rs（gloss-eval）决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## HISTORY_SVG_MAX_POINTS

- 趋势图最多绘制的点数：只画最近 100 点（JSONL 本身不裁剪）。

## render_svg

- 确定性渲染：超 `HISTORY_SVG_MAX_POINTS` 只画最近 100 点；面板数随数据
  条件变化（延迟分位仅历史有样本才画）。
- 2026-11-01 评测工具链重构步骤 1：分类质量、任务分数据集、修复成本与
  旧评审单面板随轨删除，趋势只画延迟分位——曲线序列的唯一清单归重构
  计划后续步骤的 `trend` 出口重建。

## write_header

- 画布头部：svg 开标签、白底、标题行、右上角“最近一次”标注与口径脚注；无面板时以占位文案替代图面。

## latest_note

- 右上角“最近一次”标注：UTC 时间 + mode（+ commit）。

## latest_points

- 只保留最近 `HISTORY_SVG_MAX_POINTS` 个点。

## svg_height

- 面板数对应的画布高（含顶部标题区与底部刻度留白）。

## f2

- 浮点坐标固定两位小数。
