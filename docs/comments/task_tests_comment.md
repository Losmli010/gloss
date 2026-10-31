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

## the_pending_card_cancel_path_sends_no_channel3_message（2026-10-27）

- 待确认卡的取消路径钉子：收起待确认卡（取消按钮路径）不得向 tokio 发
  任何命令——待确认阶段没有在途任务，取消无可取消；迟到的同编号产物也
  无人采纳（卡不持代数会话）。通道三零流量是发送前确认的安全属性之一：
  取材产物未经用户确认就离开 App 侧的通路必须不存在。

- language_violations_default_to_an_empty_list_on_the_wire（降级批注方案
  §6）：清单逐条往返锁定序列化形态；剥列反序列化得空清单，锁定旧条目走
  界面泛化回落、不破缓存。
