# content.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## 字号常量（`JING_FONT` / `ZHU_FONT` / `SHU_FONT` / `WORD_FONT` / `PHON_FONT` / `SEAL_FONT`）

- 经注疏排版的字号（demo 基准，应用侧定稿）：经 15.5、注 15、疏 14；
  词条 25，音标/词性 13，印章字 12，其余小字 13。

## `SMALL_FONT`

- 小字档：注区 markdown 的 Small 样式与经位占位。与 [`SHU_FONT`] 解耦，
  疏正文调号不牵动它们。

## `serif_font`

- 宋体字（经/疏/印章）。

## `kaiti_font`

- 楷体字（注/疏的 markdown 正文）。

## `watermark`

- 页脚水印的品牌名：应用名不翻译（与窗口标题同一原则）。

## 字号规格与宋楷字体助手（已迁出，2026-10-27 样式统一阶段 4）

原 `JING_FONT`/`ZHU_FONT`/`SHU_FONT`/`WORD_FONT`/`PHON_FONT`/`SEAL_FONT`/
`SMALL_FONT` 七个字号常量与 `serif_font`/`kaiti_font` 两个构造器迁往
`style/typography.rs`（`size` 角色档与 `serif`/`kai` 等角色构造器），
对照值见排印记录。本文件只剩水印等展示文案的纯函数。
