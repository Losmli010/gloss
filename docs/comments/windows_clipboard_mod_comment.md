# windows_clipboard_mod 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## copy_text 桩（2026-11-05，Windows 移植 P0）

- 签名镜像 macOS 实现（`Result<(), String>`，错误串由 `PlatformUnsupported` 的 Display 而来），调用方（flow/actions.rs）零改动编译；Win32 剪贴板实现属 P2 能力面，不在本 PR 落。
- 子模块（image/watch）的镜像类型见各自记录。
