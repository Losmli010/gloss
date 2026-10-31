# actions.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## GlossApp::handle_overlay_action

- 执行浮层一帧上交的动作（错误映射的壳侧半边 + 头部动作区 + 页脚动作区）。
- 失败卡的“打开设置”与头部齿轮、托盘走同一个入口。

## OverlayAction::Confirm（2026-10-27，发送前确认方案）

- 待确认卡主按钮“注疏”的上交：`confirm_pending()` 取回下发请求后
  `send_run`——下发请求只在确认点击的这一刻组装（陈旧点击守卫在
  `confirm_pending` 内部，壳层不多拦一道）。守卫返回 `None` 时什么都不做
  （无卡、卡已被顶掉或已消费的确认静默丢弃，通道三零流量）。

## OverlayAction::CopyAnnotation（2026-10-27）

- 完成态卡页脚“复制注疏”的上交：`copy_annotation()` 从 Outcome 视图取
  `annotation_text()`（注 + 换行拼接疏条目，原文侧信息不进剪贴板），经
  `gloss_platform::clipboard::copy_text` 写系统剪贴板。失败只记 warn 降级
  （浮层不弹错——复制是顺手功能，不是任务出口）；成功与否的界面反馈由
  页脚的“已复制”翻面承担（`RenderState.copied_at`，见 footer 记录）。
- 为什么复制动作由壳执行而不是浮层内直接写剪贴板：与 `OpenSettings` 同一
  理由——浮层只渲染、不副作用，全部平台触点收在壳层。
