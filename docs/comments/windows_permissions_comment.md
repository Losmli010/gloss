# windows_permissions 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## 权限预检桩（2026-11-05，Windows 移植 P0）

- 预检恒放行、引导为无操作：Windows 无系统级授权门，UIA 不索取权限（移植方案能力映射的定案）；与 macOS 的“缺授权即拒读”不同，该目标的阻塞点在取材桩（`PlatformUnsupported`），不在权限。
- `AccessibilityWatch` 保留同名占位、轮询恒不放行：真实现落地时决定保留或移除。
