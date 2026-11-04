# windows_events_mod 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## 事件线程桩（2026-11-05，Windows 移植 P0）

- `EventSource`/`EventSources`/`EventSink`/`EventThread` 是平台无关形状，与 macOS 实现同名同签名；`spawn` 不拉起事件线程、记一条警告痕即返回空句柄——Win32 消息泵宿主（§3.3）随 P2 落地，桩期如实报告事件线程缺席。
