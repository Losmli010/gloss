# windows_mouse 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## 鼠标手势源桩（2026-11-05，Windows 移植 P0）

- `MouseSource::spawn` 返回空源（手势事件不产生）与不复位的降级标志：低级鼠标钩子（`SetWindowsHookExW` + 消息泵宿主）随 P2 核心链路落地；线程命名口径 `gloss-mouse-tap` 到时沿用。
