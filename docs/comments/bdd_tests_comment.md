
## 2026-10-06 soak 条目改人工检查点（app_survives_eight_hour_mixed_load_soak）

评审发现原步骤的自动触发循环是假通过：`cargo test -- --ignored reads_live_selection_when_authorized --exact` 裸名匹配 0 个测试（真实全名含 `selection::accessibility::live_tests::` 前缀），且该测试是测试进程内的被动 AX 读取，即使修对名字也驱动不了在跑的 gloss 实例（不注入手势、不经过事件 tap）。仓库内没有事件注入助手（CGEvent 只出现在应用自身 tap 代码），无人值守的真实划词触发不可得，故改为「~2 小时间隔人工划词检查点」，自动部分只剩 RSS 采样循环。顺带修：日志计数 glob 限定 `gloss-*.jsonl`（prune 只管自家文件）；`just logs` 补 `--no-follow`（默认 follow 不退出）；场景断言删 phys_footprint（步骤只采 RSS，app-metrics 归档的是自 spawn 实例）。

## first_launch_guides_accessibility（原 …_then_input_monitoring，2026-10-09）

- 2026-10-09 用例更名与改写：输入监控引导移除后，首启只剩辅助功能一项系统引导，预期改为「只弹辅助功能对话框、不出现监控输入面板」；复位命令去掉 `tccutil reset ListenEvent`。soak 用例前置同步改为仅辅助功能。验收口径「keychain 授权框不抢在系统引导之前」不变（预热门控仍按启动时刻的辅助功能预检结论）。

## 评审收敛（2026-10-13，PR #178）

- **场景散文里的实现名残留**：injected_drag 条目的场景写「rdev 注入的
  拖拽序列」，注入实现已换 ffi/event.rs——`just test-bdd` 只校验测试名
  与命令级一致，抓不到散文里的机制名，本次由人工评审发现并修正（机制
  名从场景中删去，更新时间刷新）。
- **为什么不为散文机制名补门禁**（评审建议的「场景文本中 crate 名须仍
  在 Cargo.lock」评估后不采纳）：场景散文是自然语言，image/log/time/
  toml 等合法包名与普通英文词高度重叠，机械匹配必须配手工豁免清单，
  豁免清单自身会腐化成新的漂移源；清单的契约面是「测试名 ↔ 源码」双
  向一致（已门禁），散文里的机制名依赖「随代码同 PR 更新」纪律与人工
  评审兜底，本条即实例。
