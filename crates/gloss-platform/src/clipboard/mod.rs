//! 剪贴板图片的触发哨兵与取材读取：`ClipboardWatchSource` + `ClipboardImageReader`。
//!
//! 双栈分工：哨兵只读 `NSPasteboard.changeCount` 与 `types`（纯元数据，廉价，
//! 不触发 `macOS` 15+ 的「允许粘贴」授权流程），回答「要不要触发」；内容读取
//! 直读同一粘贴板的图像 `flavor` 数据（`dataForType:`，`TIFF`/`PNG`），经 `image` 包
//! 解码为 `RGBA`，回答「是什么图」。① 观察与 ② 取材之间剪贴板可能被
//! 覆盖：哨兵在发①时记下 `changeCount`，读取器比对现值——不一致即竞态，
//! 静默丢弃。两侧同在平台事件线程上运行（同线程顺序消费，见 `events` 模块
//! 文档），原子量只为跨闭包共享。
//!
//! 日志红线：图像只记字节数与像素尺寸，内容与字节永不落日志、不落盘、
//! 不进错误消息。

mod image;
mod watch;

pub use image::ClipboardImageReader;
pub use watch::{
    ClipboardWatchSource, PasteboardImageObserved, PasteboardObserver, RoundWrites, WatchRoundFlag,
};

use objc2::rc::Retained;
use objc2_app_kit::{NSPasteboard, NSPasteboardTypePNG, NSPasteboardTypeTIFF};
use objc2_foundation::NSData;

pub(crate) fn pasteboard_change_count() -> Option<u64> {
    let pasteboard = NSPasteboard::generalPasteboard();
    u64::try_from(pasteboard.changeCount()).ok()
}

pub(crate) fn pasteboard_has_image() -> bool {
    let Some(types) = NSPasteboard::generalPasteboard().types() else {
        return false;
    };
    // SAFETY: `NSPasteboardTypePNG`/`NSPasteboardTypeTIFF` 是 AppKit 导出的
    // 常量字符串 extern static，进程启动即初始化、存活期内恒有效。
    types.containsObject(unsafe { NSPasteboardTypePNG })
        || types.containsObject(unsafe { NSPasteboardTypeTIFF })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageFlavor {
    Tiff,
    Png,
}

pub(crate) fn pasteboard_image_flavor() -> Option<(ImageFlavor, Retained<NSData>)> {
    let pasteboard = NSPasteboard::generalPasteboard();
    // SAFETY: AppKit 导出的常量字符串 extern static，进程启动即初始化、
    // 存活期内恒有效。
    let candidates = [
        (ImageFlavor::Tiff, unsafe { NSPasteboardTypeTIFF }),
        (ImageFlavor::Png, unsafe { NSPasteboardTypePNG }),
    ];
    for (flavor, flavor_type) in candidates {
        if let Some(data) = pasteboard.dataForType(flavor_type) {
            return Some((flavor, data));
        }
    }
    None
}
