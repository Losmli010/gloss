# foreground.rs 决策记录（系统（Windows）外部函数接口）

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## foreground_image_name（2026-11-06，Windows 移植 P2①）

- 前台窗口进程映像路径查询：`GetForegroundWindow` → `GetWindowThreadProcessId`
  → `OpenProcess`（仅查询受限权）→ `QueryFullProcessImageNameW`。只取映像
  路径与进程标识，不读窗口标题、不碰用户内容（探针红线：纯查询）。
- 每一步失败都返回空（安全桌面（UAC）、锁屏、会话切换、无前台窗口）——
  闸门按“无事实”放行，探针读不到不该升级成拦截（与系统（macOS）前台应用
  取不到时的同一降级路径）。
- 句柄在早退路径之前关闭（查询结果先落栈再关句柄），不留泄漏路径。

## IMAGE_PATH_CHARS

- 映像路径缓冲按一千零二十四个宽字符取上限：常规安装路径（含长路径与深
  嵌套）都装得下；超长路径按查询失败处理（返回空），不为极端路径扩面。
