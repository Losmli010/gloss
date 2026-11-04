# windows_clipboard_watch 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## 哨兵源桩（2026-11-05，Windows 移植 P0）

- 轮询恒空集：变更监视对应 Win32 `GetClipboardSequenceNumber` 轮询（保持 poll 形状），随 P2 落地；桩保留构造、观察句柄与轮次标记的同名镜像，组装段的取材管线可原样装配。
