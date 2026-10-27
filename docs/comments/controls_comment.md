# controls.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## `accent`

- 品牌强调色：明暗主题各取朱砂档——浅底用压暗档、深底用提亮档，与印章
  朱丝栏、品牌划线同一色源。样式统一阶段 2 起取档走语义调色板
  （`Palette.accent`），本模块不再触碰界面库的明暗态；错误态的
  下划线与提示色同理走 `Palette.danger`。

## `action_row`

- 动作行：取消（次按钮）+ 保存（朱砂主按钮），右对齐。
- 先分配固定行高再右对齐：bottom_up 里直接 with_layout(RTL) 的子区域
  会撑满剩余整高，按钮被垂直居中到窗口中部，滚动区被挤剩一条。

## `ACTION_ROW_HEIGHT`

- 动作行高度（按钮高 + 上下留白）。

## `notices`

- 提示行：校验汇总（进入校验态且有错）与壳回写提示（保存失败等）。

## `section`

- 分区：区块标（弱色小字）+ faint_bg 卡片。

## `caption`

- 弱色小字（区块标 / 子块标 / 说明提示共用一档）。

## `add_input`

- 设置页输入框统一入口：浅色主题下白底输入框与近白卡片几乎同色，
  补一圈浅灰描边保持可见；深色主题输入框本身比卡片暗，不加描边。

## `underline_if_error`

- 字段错误态：控件底边 DANGER 下划线。

## `error_text`

- 就地错误提示（控件正下方，CAPTION DANGER）。

## `TOGGLE_TRACK` / `TOGGLE_KNOB_RADIUS` / `TOGGLE_KNOB_INSET`

- 开关滑块的轨道尺寸与滑钮半径（demo 定稿比例的紧凑档：滑钮直径略小
  于轨高，上下各留一线余量）。

## `TOGGLE_KNOB_ON`

- 开启态滑钮色：朱砂着轨配白钮，与保存主按钮同观感（两主题通用）。

## `toggle_switch`

- 最小开关（switch）：egui 0.36 无内建 Switch（`WidgetType` 亦无 switch
  档），按官方 demo 的 `toggle_switch` 范式自绘——点击整块切换，无障碍
  以 `WidgetType::Checkbox` + 选中态上报（0.36 的角色映射里唯一的双态
  档，内建 `Checkbox` 也走它）。返回的响应供调用方感知变更。

## `choice_row`

- 双列行：行标签左、控件推到卡片右缘（两端对齐）；返回控件自身的响应
  （字段标红要按控件的矩形画下划线）。

## `choice_hint`

- 双列行的说明提示（左对齐，CAPTION 弱色）。
