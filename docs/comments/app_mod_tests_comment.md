# app/mod.rs 测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md「注释纪律」节），测试的验收意图与辅助契约记录在此。
> 2026-09-20 由 app_tests_comment.md 按 R4 拆分迁入（test_support 收进 app/mod.rs）。

## 测试行为速览（BDD）

已统一迁往 docs/tests/bdd.md（git 追踪，以测试代码为唯一事实源）；本文件只留缘由与形态说明。

## 测试辅助（test_support / driven_app 家族）

- 2026-09-18 DrivenApp：`(GlossApp, Arc<ConfigHandle>, Arc<dyn ConfigStore>, 平台事件发送端, 取材命令接收端, 推理命令接收端, 回传事件发送端)`——App 本体加四条通道的驱动端点。
- 2026-09-18 driven_app：接入真实通道与配置句柄的 App，配置走 core 的内存桩，测试可 `handle.save(...)` 模拟设置页保存，观察下一次触发是否用上新配置。driven_app_with：注入必失败存储的版本（设置页失败路径测试用）。driven_app_using：再注入热键桩的版本（M4-T7 重注册接线测试用），桩由调用方持 Arc 共享，不关心热键的测试用 driven_app。
- 2026-09-18 trigger_selection：发送 `PlatformEvent::SelectionGesture` 后调 `drain_platform_events`，驱动一次划词触发走完通道①消费。
- 2026-09-18 text_input / plain_outcome：构造文本取材产物与 Plain 结局。streaming_body / outcome_body：从 `overlay_view` 取当前正文，非期望视图即 panic（测试辅助：panic 即断言）。
- 2026-09-20 R4 拆分后辅助收进 `app/mod.rs` 的 `#[cfg(test)] pub(super) mod test_support`，各子模块测试经 `crate::app::test_support` 引用——GlossApp 私有字段只在 crate::app 子树可见，辅助放这里恰好覆盖全部子模块测试。
- 2026-09-22 任务 09 给 `app/mod.rs` 补了它自己的第一个测试模块：`ui_locale_follows_the_saved_language_without_a_restart` 直接断言 `GlossApp::locale` 的落定（出厂 → `Language::En` → `Language::System` 三跳），把「语言切换不重启即生效」钉在壳侧解析这一层；渲染侧是否真的换了表由 ui 层的 `failure_card_follows_the_locale` / `english_catalog_relabels_the_settings_window` 负责，两者合起来才等于那条验收标准。
- 2026-09-27 update_wiring：更新接线的测试桩——本地 watch（恒为初始快照）+ 空发送出口，不拉起真实模块、不触碰网络；驱动 GlossApp 的测试默认用它，避免为不关心更新状态的用例拉起 gloss-update 线程。

## test_support 的剪贴板哨兵开关位（2026-10-12）

- driven_app 夹具的 `clipboard_watch_enabled` 按夹具配置快照初始化
  （`config.snapshot().watch_clipboard_images`），不另定硬编码初值：
  与 main.rs 组装段的生产装配同源——出厂默认翻转（关闭→开启）或未来
  再改默认时，夹具行为自动跟随，不会出现「生产 on、夹具 off」的两套
  口径；settings 快照基线重录也依赖这个同源初值与真机同观感。
