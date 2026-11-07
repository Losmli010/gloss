# ui/context.rs 测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md“注释纪律”节），测试的验收意图与辅助契约记录在此。

## 测试行为速览（BDD）

已统一迁往 docs/tests/bdd.md（git 追踪，以测试代码为唯一事实来源）；本文件只留缘由与形态说明。

## 辅助契约与形态

- `new_context_carries_fonts_and_theme` 的读法：egui 的 `set_fonts` 只把定义**入队**，下一个 pass
  才生效（`Context::set_fonts` 文档即“active at the start of the next pass”），所以字体表必须在
  `run_ui` 的闭包里读，不能在 `new_context` 之后直接读。
- 同处的 `output.drop_without_applying_deltas()`：测试里没有渲染后端消费文字图集增量，而 epaint 的
  `TexturesDelta` 在析构时会断言“未处理增量”，必须显式丢弃。
- 该测试依赖宿主机有系统 CJK 字体（找不到时 `fonts::definitions()` 返回 `None`），失败信息里带宿主机
  提示；与 `ui/fonts.rs` 的 `system_cjk_font_*` 同惯例，bdd 已标注。
- `reapply_writes_every_context` 用真实 `egui::Context` 观测“每个上下文都被写到”；三档主题轮换保证
  每轮目标值都异于当前值，避免空断言；空迭代那半钉“不 panic 且返回 0”。

## 与 fonts.rs 测试的分工

- 字体登记的契约（借用、零拷贝共享、只加载一次）在 `ui/fonts.rs` 的测试里；
- 这里只钉“新上下文把字体与主题都装上了”这一层，以及“重施加写满每个上下文”。

## 宿主字体前提守卫（2026-11-05，P1 测试矩阵开放 Windows）

- `new_context_carries_fonts_and_theme` 的延迟补装断言依赖宿主系统字体：经字体模块的测试期前提访问器（`system_font_discovery_present`）守卫——发现面缺席的目标（字体占位）上无此前提，直接让路；macOS 面断言不变（见 fonts 记录的测试面归宿）。

## 守卫收窄到宿主依赖段（2026-11-07，移植评审遗留 A7）

- 为什么守卫只罩补装段：主题偏好落上与命名字体族恒绑定两条断言不依赖宿主发现面（兜底绑定时族也恒注册），原本被整测提前返回一并跳过，无发现面宿主失去这两条断言面。处置为收窄守卫范围——前提访问器检查移到补装段之前，主题与族绑定断言在任何宿主照跑；`text_cursor_stroke_follows_the_palette_per_theme` 等主题断言本就不在守卫内，不变。
