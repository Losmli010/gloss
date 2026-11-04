# windows_composite 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## 组合读取器桩（2026-11-05，Windows 移植 P0）

- `read` 一律返回 `PlatformUnsupported`、无还原收尾（settle 空实现）：UIA 主路 + SendInput 模拟复制兜底（保留读后还原语义）随 P2 ① 落地；组装段的读取调用面零改动编译。
