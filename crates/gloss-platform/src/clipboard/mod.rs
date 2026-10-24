//! 剪贴板图片的触发哨兵与取材读取：触发哨兵加内容读取器。
//!
//! 双栈分工：哨兵只读系统粘贴板的变更计数与类型清单——纯元数据，廉价，
//! 不触发系统（macOS）15 及以上的“允许粘贴”授权流程——回答“要不要触发”；
//! 内容读取直读同一粘贴板的图像数据风格（flavor），按类型取数据，
//! 类型为标记图像文件格式（TIFF）或便携式网络图形（PNG），经图像解码库（image）
//! 解码为红绿蓝透明像素，回答“是什么图”。1 观察与 2 取材之间剪贴板可能被
//! 覆盖：哨兵在发1时记下变更计数，读取器比对现值——不一致即竞态，
//! 静默丢弃。两侧同在平台事件线程上运行，同线程顺序消费，见事件线程模块
//! 文档；原子量只为跨闭包共享。
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
    // SAFETY: `NSPasteboardTypePNG`/`NSPasteboardTypeTIFF` are constant string
    // extern statics exported by AppKit, initialized at process start and valid
    // for the entire lifetime of the process.
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
    // SAFETY: constant string extern statics exported by AppKit, initialized at
    // process start and valid for the entire lifetime of the process.
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
