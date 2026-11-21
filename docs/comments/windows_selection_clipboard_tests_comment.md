# clipboard（系统（Windows）剪贴板兜底读取器）测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md“注释纪律”节），测试的验收意图与辅助契约记录在此。

## 测试目标与层

- 第一层（L1）：写入确认的纯时序函数（`wait_for_write`）的替身单测随方案
  A2（2026-11-17）上移共享层，见 fallback_tests 记录；补放列表的纯函数在
  外部函数接口（FFI）注入模块（见 inject_tests 记录）。
- 人工测试（L4，`--ignored`，须在真机（Windows）上跑）：真实注入链路的
  恢复保真（`fallback_read_restores_original_clipboard_on_windows`）与
  未收口的 `Drop` 恢复（`fallback_read_unwind_restores_the_clipboard_
  without_finish_on_windows`）；全系列经模块内互斥锁串行，勿并行触发
  其它剪贴板测试。

## 恢复序列计量单测（2026-11-21）

- sequence_jumps_measures_paired_readings_and_declines_missing_ones：纯函数
  五例组合钉契约——成对读数返回实测差（回退饱和为零，序列号回绕的静默边
  界）、任一读数缺席返回空（不猜数）；成功轮空缺按下界两跳计、失败轮记零
  的取舍在调用方，边界缘由见 windows_selection_clipboard 记录。

## 辅助契约

- 确认窗口与轮询间隔的超时路径必须及时返回（不悬挂）。
- `L4` 恢复测试的前提是前台有可复制的选中文本；未确认轮不注入即不恢复
  （板上外来写入原样保留）。
