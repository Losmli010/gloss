# windows_clipboard_image 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## 取材读取器桩（2026-11-05，Windows 移植 P0）

- `read` 一律返回 `PlatformUnsupported`：图像格式（CF_DIB/CF_DIBV5/PNG）解码与 `image` 库特性面（Windows 加 bmp/dib）随 P2 剪贴板取材落地。

- 2026-11-05（P1）类型改名 `SystemImageReader` 并实现核心层图片取材端口的桩（与系统（macOS）实现同名镜像）：仍一律报告未支持。
