# windows_locale 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## system_locale 桩（2026-11-05，Windows 移植 P0）

- 恒回落 `Locale::En`：与 macOS 实现“读不到系统偏好即英文”的降级语义一致；`GetUserDefaultLocaleName` 读取随 P2 落地。
