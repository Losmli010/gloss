# clipboard/mod.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关符号时同步更新本文件。

## `pasteboard_change_count` / `pasteboard_has_image` / `pasteboard_image_flavor`（2026-10-10）

- **为什么哨兵与取材的 NSPasteboard 探针集中在 mod.rs**：三个探针共用同一
  个「generalPasteboard + 类型常量」面，分散到 watch/image 各自文件会让
  「哨兵认什么、取材读什么」的对齐关系漂到两处——`ImageFlavor` 枚举的
  候选表（TIFF 先、PNG 后）与 `pasteboard_has_image` 的类型判定在这里
  同源，评审发现过的「哨兵认 PNG、读取只支持 TIFF」能力漂移在结构上
  不再可能（新增 flavor 时两处必同改，否则编译可见的枚举/常量缺口）。
- **为什么 `pasteboard_image_flavor` 的候选序是 TIFF 先、PNG 后**：截图
  （⌘⇧4+Ctrl）与多数 macOS 应用的「复制图像」写 TIFF，浏览器/文件复制
  常写 PNG，两类来源同时在场时 TIFF 是更「原生」的一份（PNG 常是同图
  的转码副本）；顺序只影响读到哪份编码数据，图像内容一致。
- **为什么返回 `Retained<NSData>` 而不是立刻拷成 `Vec<u8>`**：解码在调用方
  作用域内即刻发生（切片借用在 `read()` 内结束），拷贝是纯多余的一次
  大缓冲 memcpy；`as_bytes_unchecked` 的安全性由「NSData 活过整个借用期、
  期间无人改写粘贴板缓冲」保证——NSData 是不可变对象，粘贴板覆盖只会
  换引用不会原地改写。
- **`unsafe { NSPasteboardTypeTIFF/PNG }` 的缘由**：AppKit 导出的常量字符
  串 extern static，进程启动即初始化、存活期内恒有效；读取 extern static
  在 Rust 里是 unsafe 操作，事实上的前提只有「常量恒有效」一条。

## 符号契约（自代码注释迁入）

- `pasteboard_change_count`：读系统剪贴板的 changeCount（纯元数据查询，不触碰内容）；读不到返回 `None`（generalPasteboard 恒返回有效板，负值不合法——两者都按不可观察处理，调用方保持静默）。
- `pasteboard_has_image`：剪贴板当前内容是否含图片类型（types 查询同样只碰元数据）。TIFF 覆盖截图（⌘⇧4+Ctrl）与多数应用的「复制图像」，PNG 覆盖浏览器/文件复制。
- `ImageFlavor`：取材侧可读的图像 flavor：与 `pasteboard_has_image` 认的两种类型一一对应——哨兵认什么，取材就读什么，两侧能力不漂移。`Tiff` = public.tiff，`Png` = public.png。
- `pasteboard_image_flavor`：取剪贴板上第一份可读的图像 flavor 数据（TIFF 先于 PNG）；板上没有可读图像（被覆盖成文本/清空）时返回 `None`。取到即持 Retained 的 NSData，内容读取在调用方作用域内完成。
