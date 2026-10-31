# settings.rs 测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md“注释纪律”节），测试的验收意图与辅助契约记录在此。
> 本模块：草稿逻辑单测 + L2 kittest 渲染与交互（AccessKit 树断言）；快照基线与其他 harness 合并进同一个 SnapshotResults。

## 测试行为速览（BDD）

已统一迁往 docs/tests/bdd.md（git 追踪，以测试代码为唯一事实源）；本文件只留缘由与形态说明。

## 辅助

- harness_for：驱动一帧设置窗口，返回（harness，动作收集器）；画布拉到内容全高（ScrollArea 只物化可见区，默认小画布下折叠的控件不进 AccessKit 树）；收集器只累积非 Idle 动作（点击帧之后的帧回 Idle，不能冲掉已上交的动作）。
- 2026-09-22 任务 09 起 `harness_for` 多一个 `locale` 入参：文案表由用例显式给（`Text::get(locale)`），
  既有用例一律传 `Locale::Zh`（快照基线即出厂中文），`english_catalog_relabels_the_settings_window`
  传 `Locale::En`——语言是断言的一部分，不藏在辅助里。
- 2026-09-22 任务 09 评审整改补四条：`saving_the_language_swaps_the_rendered_labels_without_a_restart`
  在同一进程里经 `ConfigHandle` 保存 `Language::En` 再渲染，钉的是“配置快照 → `Language::resolve`
  → 选表 → 渲染”整链，而不是只断言取值函数；`a_rendered_notice_follows_the_locale` 补上提示行的
  英文渲染（此前提示行只有中文快照可查）；`every_notice_renders_its_localized_prefix_and_detail`
  逐条渲染三类提示，覆盖两条此前从未被渲染过的模板。`english_catalog_relabels_the_settings_window`
  另外补上“英文帧里不得残留中文标”的反向断言。

## snapshots_match_baseline（settings）

- 校验错误态走可观察路径进入：点“保存”被阻断后再截图，等同真实用户操作（而非直接置 `validated`），基线锁的就是这条路。
- **残留缺口**（记录项）：`GlossApp::draw` / `draw_settings` 里“把解析出的 locale 传给渲染入口”
  那一行转发没有单测覆盖——kittest harness 走的是渲染函数本身，`Frame` 只能由真实窗口栈构造，
  故这一行与窗口标题一并留真机走查。

## 跨字段规则的两类用例（2026-09-23）

- 纯逻辑一条（`a_disabled_default_task_blocks_save`）+ 渲染一条（`a_disabled_default_task_is_blocked_with_an_in_place_hint`）+ 手改配置一条（`the_default_task_rule_follows_the_selection_kind_fold`）：前者判错误键与“改对即清”，中者判阻断、提示替换与恢复保存，后者判规则判的是**收口后**的 kind（`selection_task_kind()`）而不是下拉框里那个字段——照字段面判会让“图像 kind 当默认任务 + TranslateWord 被停用”这种手改配置漏网。
- **形态坑（第一版稿子踩过）**：`SettingsState::errors()` 在 `validated == false` 时恒返回空表（“编辑中不追着标红”的实现），所以“没有错误”这类断言必须先经一次 `build_save` 进入校验态再看 `errors()`，否则断言恒真/恒假。判“放行”优先断 `build_save` 的上交物（公开契约），`errors()` 只用来钉具体错误键。
- 渲染条目的“不唠叨”断言（首次保存前树上没有停用提示）钉的是这条规则别抢在 `validated` 之前生效。
- 快照加第四态 `settings_default_kind_disabled`（改到跨字段规则或默认任务行的版式时同步重录）：基线里能看到 DANGER 下划线落在**下拉框**上（`choice_row` 的返回值改造就为它）、该行说明提示被错误提示替换、底部汇总行计数为 1。

## 快照改英文浅色（2026-10-01）

- `snapshots_match_baseline` 九张基线从 zh 暗色改为 **En + 浅色**
  （`snapshot_harness`，kittest builder `with_theme(Theme::Light)`）：
  kittest 自建上下文无 CJK 字形，zh 基线文案全 tofu 不可审。校验流程里
  的断言随语言换（保存 → “Save”、“已就地标红” → “marked in place”、
  “默认任务已停用” → “The default task is disabled”）；其余断言类用例
  仍按各自需要取 zh/en。旧基线的 `.old.png` 备份不入库。

- 2026-10-10 八张 settings 基线同 PR 更新（settings_main / notice / invalid / update_* 五态）：出厂模型 `deepseek-chat` → `deepseek-flash`（剪贴板图片方案 §5.4，gloss-core `DEFAULT_MODEL`），设置页模型输入框预填文案随之变化。逐像素 diff 核验：八张差异均为同一位置的同一文本行（各 72 px），无布局或样式漂移；属预期内容变更，显式审查后更新。
