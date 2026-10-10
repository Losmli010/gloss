//! 剪贴板图片的触发哨兵与取材读取：`ClipboardWatchSource` + `ClipboardImageReader`。
//!
//! 双栈分工：哨兵只读 `NSPasteboard.changeCount` 与 `types`（纯元数据，廉价，
//! 不触发 macOS 15+ 的「允许粘贴」授权流程），回答「要不要触发」；内容读取
//! 走 arboard 的 `get_image`（RGBA 位图），回答「是什么图」。① 观察与 ② 取材
//! 之间剪贴板可能被覆盖：哨兵在发①时记下 changeCount，读取器比对现值——
//! 不一致即竞态，静默丢弃。两侧同在平台事件线程上运行（同线程顺序消费，
//! 见 `events` 模块文档），原子量只为跨闭包共享。
//!
//! 日志红线：图像只记字节数与像素尺寸，内容与字节永不落日志、不落盘、
//! 不进错误消息。

mod image;
mod watch;

pub use image::ClipboardImageReader;
pub use watch::{ClipboardWatchSource, PasteboardImageObserved, PasteboardObserver};

use objc2_app_kit::{NSPasteboard, NSPasteboardTypePNG, NSPasteboardTypeTIFF};

/// 读系统剪贴板的 changeCount（纯元数据查询，不触碰内容）；读不到返回
/// `None`（generalPasteboard 恒返回有效板，负值不合法——两者都按不可观察
/// 处理，调用方保持静默）。
pub(crate) fn pasteboard_change_count() -> Option<u64> {
    let pasteboard = NSPasteboard::generalPasteboard();
    u64::try_from(pasteboard.changeCount()).ok()
}

/// 剪贴板当前内容是否含图片类型（types 查询同样只碰元数据）。TIFF 覆盖
/// 截图（⌘⇧4+Ctrl）与多数应用的「复制图像」，PNG 覆盖浏览器/文件复制。
pub(crate) fn pasteboard_has_image() -> bool {
    let Some(types) = NSPasteboard::generalPasteboard().types() else {
        return false;
    };
    // SAFETY: `NSPasteboardTypePNG`/`NSPasteboardTypeTIFF` 是 AppKit 导出的
    // 常量字符串 extern static，进程启动即初始化、存活期内恒有效。
    types.containsObject(unsafe { NSPasteboardTypePNG })
        || types.containsObject(unsafe { NSPasteboardTypeTIFF })
}
