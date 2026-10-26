# palette.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## `Class`

- 着色类别（demo 的 tok 六分类）：注释恒斜体，其余直立。
- 变体取色（demo token 明暗两套）：
  - `Keyword`：关键字（demo --k）。
  - `String`：字符串字面量（demo --s）。
  - `Comment`：行/块注释（demo --c，恒斜体）。
  - `Number`：数字（demo --n）。
  - `Type`：类型/键/标签（demo --t）。
  - `Function`：函数形/属性/选择器（demo --f）。

## `Class::color`

- 类别着色（demo token 明暗两套）。

## `Class::italic`

- 注释恒斜体（demo .tok.c 的 font-style: italic）。

## `plain_color`

- 代码平文字色：面板底上的主文字色（明暗两档，demo 的正文墨色）。
