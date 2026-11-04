# task.rs 测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md“注释纪律”节），测试的验收意图与辅助契约记录在此；`// SAFETY:` 仍留在代码内。

## 测试行为速览（BDD）

已统一迁往 docs/tests/bdd.md（git 追踪，以测试代码为唯一事实源）；本文件只留缘由与形态说明。

## 辅助

- text_task / image_task：构造文本与图像 Task 的夹具助手。
- confirm(app)：替测试点一次“注疏”（`handle_overlay_action(Confirm)`）——
  2026-10-27 起提交不再直接下发，凡期望通道三收到 `RunTask` 的用例都要
  在提交后补这一步；它让“谁在触发下发”在测试体里显式可见。
- warm_cache(app, input, note)（2026-11-04）：按探测将冻结的同款选项
  （`task_options(出厂配置, 系统语言)`）预热壳侧任务缓存。键必须与探测
  冻结快照逐字节同源，否则用例静默变成“未命中”，假绿——所以键经
  `cache_key` 现算而不是手拼常量。

## a_cached_selection_skips_confirmation_and_lands_the_outcome（2026-11-04）

- 缓存命中免确认的编排侧主干钉子：预热的选区直落产物卡（`Show`、通道三
  零流量、同帧显形挂起置位），紧随其后一次未缓存的划词照旧落待确认卡并
  经确认下发——同一用例里两段对照，钉住“跳过确认的前提是亲眼命中”，未
  命中的把关原样在岗。

## a_cached_pasteboard_image_skips_confirmation_too（2026-11-04）

- 图像路径的同权钉子：剪贴板图片的缓存命中同样直落产物卡、通道三零流量
  ——免确认不是文本路径的特权，模态只影响经位渲染不影响把关语义。

## the_pending_card_cancel_path_sends_no_channel3_message（2026-10-27）

- 待确认卡的取消路径钉子：收起待确认卡（取消按钮路径）不得向 tokio 发
  任何命令——待确认阶段没有在途任务，取消无可取消；迟到的同编号产物也
  无人采纳（卡不持代数会话）。通道三零流量是发送前确认的安全属性之一：
  取材产物未经用户确认就离开 App 侧的通路必须不存在。

- language_violations_default_to_an_empty_list_on_the_wire（降级批注方案
  §6）：清单逐条往返锁定序列化形态；剥列反序列化得空清单，锁定旧条目走
  界面泛化回落、不破缓存。
