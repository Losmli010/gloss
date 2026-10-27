# code_hl/palette.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## `Class`

- 着色类别（demo 的 tok 六分类）：注释恒斜体，其余直立。
- 变体到语义色板的取色口径（色值住 `style/palette.rs` 的 `SyntaxPalette`）：
  - `Keyword`：关键字（demo --k）。
  - `String`：字符串字面量（demo --s）。
  - `Comment`：行/块注释（demo --c，恒斜体）。
  - `Number`：数字（demo --n）。
  - `Type`：类型/键/标签（demo --t）。
  - `Function`：函数形/属性/选择器（demo --f）。

## `Class::pick`

- 类别着色：从语义调色板的语法色板取色（明暗两份由 `style/palette.rs`
  定稿）。样式统一阶段 2 前本文件自带明暗两套十二色（`Class::color(dark)`），
  色值原样迁入语义调色板后，这里只留类别到色板的映射，`dark` 参数在整条
  调用链上消失。

## `Class::italic`

- 注释恒斜体（demo .tok.c 的 font-style: italic）。

## 平文字色（原 `plain_color`，已迁出）

- 代码平文字色：面板底上的主文字色（明暗两档，demo 的正文墨色）。色值
  现住 `style/palette.rs` 的 `SyntaxPalette::plain`，本文件不再承载色值。
