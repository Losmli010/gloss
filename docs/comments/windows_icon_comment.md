# windows_icon 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## MacAppIcon 镜像桩（2026-11-05，Windows 移植 P0）

- 同名镜像端口类型 `MacAppIcon`（实现 `AppIcon`）：命名沿用 macOS 侧是过渡——P1 装配收敛（D3）后消费方经 `Platform` 句柄取端口，类型名将随 P2 真实现一并正名（Windows 无对应概念，实作为无操作或任务栏覆盖）。
