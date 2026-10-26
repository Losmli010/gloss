# inspect.rs（gloss-eval）决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## render_html

- 渲染检视页。`history` 供趋势图与「对比上一次同 mode 运行」的差值。
- 纯函数，不读环境、不落盘；数据集文本与模型回复等外来内容一律经 HTML 转义落进静态标记，交互（过滤）只用内联的十几行 vanilla JS——产物零外部资源，离线双击可开，也可直接分发给无终端的评审者。
- 口径脚注必须随页走：replay 页的准确率是生产校验器对**固化夹具**的判定，改 prompt 并不会动它（需 `--record` 重录夹具）；改 prompt 后的真实准确率看 live 页。这条不写清，检视页会把人引向错误结论。

## 过滤值与 data-v 属性（render_html 内联 JS）

- 过滤值只在 data-v 属性上传递、由事件委托读取：内联 onclick 的属性实体会先于 JS 解码，等价于把数据值拼进 JS 字符串字面量。

## render_header / render_cards / render_classify_card / render_task_cards

- `render_header`：头部——标题 + mode 徽标 + 运行上下文与覆盖面。
- `render_cards`：指标卡——分类一张、任务轨每数据集一张；与上一次同 mode 运行比出差值。
- `render_classify_card`：分类卡——accuracy 与回退率带上与上一次同 mode 运行的差值徽标。
- `render_task_cards`：任务轨卡片——每个数据集一张，字段完整率/降级率与上次比差值。

## comparable

- 可比的上次比率：两侧分母都非零才返回（`None` 即不出差值徽标）——空分母（无夹具/全跳过/零 evaluated）的 0/0 与上次的比率没有可比性。

## render_confusion / render_cases

- `render_confusion`：混淆矩阵（仅错误路径）；无错误行整段跳过。
- `render_cases`：逐条明细——状态过滤 + 轨道过滤（内联 JS），每条一个原生 `<details>` 展开逐轮原始回复。

## case_badge / case_verdict_text

- `case_badge`：状态归档（过滤键 + 徽标）——引擎失败独立于模型行为，误判/格式错/拒绝同为硬错误，缺疏证是软缺口（不降级）。
- `case_verdict_text`：判定列文本——误判给实际落点、降级给产物状态（末轮 note 预览在展开区；期望列已示期望 kind，不再重复）。

## previous_run / DeltaDir / delta_chip

- `previous_run`：上一次同 mode 运行（差值基准）——时间戳相同视为本次本身，不再向前比。
- `DeltaDir`：差值方向语义——accuracy/完整率涨是好，回退/降级率涨是坏。
- `delta_chip`：差值徽标——无基准（首次运行或分母为 0）不显示；四舍五入后为零不发声。

## group_label

- 分组的展示名：任务数据集名 → 数据集简称；其余（分类）原样。
