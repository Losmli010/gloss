# svg.rs（gloss-eval）决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## dataset_order 去重期借用（P1-1 clone 削减，2026-10-13）

- 数据集名去重集合改存 `&str`（借自历史点），末尾一次性落成有副本的
  `Vec<String>`——每个数据集只复制一次（原先逐历史点重复克隆同一批
  名字），首次出现序不变。

## HISTORY_SVG_MAX_POINTS

- 趋势图最多绘制的点数：只画最近 100 点（JSONL 本身不裁剪）。

## COST_COLORS

- 修复成本面板里各任务数据集的线色（分类轨固定蓝色）。

## render_svg

- 确定性渲染：超 `HISTORY_SVG_MAX_POINTS` 只画最近 100 点；面板数随数据条件变化（judge 与延迟仅历史有样本才画，任务面板随数据集出现）。

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
