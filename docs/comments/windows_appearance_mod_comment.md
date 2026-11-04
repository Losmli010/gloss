# windows_appearance_mod 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## 图标装配桩（2026-11-05，Windows 移植 P0）

- `install` 返回 `PlatformUnsupported`：任务栏图标来自打包资源（§2 映射），无 Dock 式运行时装配面；组装段的失败痕（warn）在桩期成立。
