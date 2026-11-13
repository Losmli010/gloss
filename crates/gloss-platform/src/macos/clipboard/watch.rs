//! 剪贴板图片哨兵源的平台绑定：把共享哨兵形状（平台中立共享层）绑定到
//! 系统（macOS）原语。
//!
//! 共享形状（基线武装、查询节流与取材轮次的计数归因）住平台中立共享层
//! （common）；本模块只提供“读变更计数”与“是否有图”两个原语闭包——
//! 变更计数与类型清单都是纯元数据查询，不触碰内容、不触发系统（macOS）
//! 15 及以上的“允许粘贴”授权流程——并把哨兵实现核心层剪贴板监视端口
//! （轮询产出契约观察事件）。构造时读一次变更计数记基线：启动时的既有
//! 剪贴板内容不触发。真机人工测试（写真实剪贴板观察哨兵与取材读取器）
//! 留在平台侧。

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use gloss_core::ports::{ClipboardImageObserved, ClipboardWatcher};

pub use crate::common::clipboard_watch::{
    ClipboardWatchSource, PasteboardImageObserved, PasteboardObserver, RoundWrites, WatchRoundFlag,
};

use super::{pasteboard_change_count, pasteboard_has_image};

impl ClipboardWatchSource {
    pub fn new(enabled: Arc<AtomicBool>) -> Self {
        Self::arm(enabled, pasteboard_change_count)
    }
}

impl ClipboardWatcher for ClipboardWatchSource {
    fn poll(&mut self) -> Vec<ClipboardImageObserved> {
        self.poll_at(
            Instant::now(),
            pasteboard_change_count,
            pasteboard_has_image,
        )
        .into_iter()
        .map(|_| ClipboardImageObserved)
        .collect()
    }
}

#[cfg(test)]
#[allow(clippy::let_underscore_must_use)]
mod live_tests {
    use std::io::Cursor;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use arboard::Clipboard;
    use gloss_core::ports::ClipboardImageReader;
    use objc2_app_kit::{
        NSPasteboard, NSPasteboardType, NSPasteboardTypePNG, NSPasteboardTypeTIFF,
    };
    use objc2_foundation::NSData;

    use super::*;
    use crate::clipboard::SystemImageReader;

    struct RestoreOnDrop {
        text: Option<String>,
    }

    impl Drop for RestoreOnDrop {
        fn drop(&mut self) {
            let Some(text) = self.text.take() else {
                return;
            };
            if let Ok(mut clipboard) = Clipboard::new() {
                let _ = clipboard.set_text(text);
            }
        }
    }

    fn write_flavor(flavor_type: &'static NSPasteboardType, bytes: &[u8]) {
        let board = NSPasteboard::generalPasteboard();
        board.clearContents();
        let data = NSData::with_bytes(bytes);
        assert!(
            board.setData_forType(Some(&data), flavor_type),
            "flavor write must succeed"
        );
    }

    fn encoded_2x2(format: image::ImageFormat) -> Vec<u8> {
        let rgba = vec![
            255u8, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 0,
        ];
        let canvas = image::RgbaImage::from_raw(2, 2, rgba).expect("canvas");
        let mut bytes = Cursor::new(Vec::new());
        canvas.write_to(&mut bytes, format).expect("encode");
        bytes.into_inner()
    }

    #[test]
    #[ignore = "需图形会话：写真实剪贴板并观察哨兵与取材读取器"]
    fn watch_and_reader_carry_a_real_image_copy() {
        let text_before = Clipboard::new()
            .ok()
            .and_then(|mut clipboard| clipboard.get_text().ok());
        let _restore = RestoreOnDrop { text: text_before };

        let (flavor, flavor_type, format) = (
            crate::clipboard::ImageFlavor::Tiff,
            // SAFETY: constant string extern static exported by AppKit, valid
            // for the entire lifetime of the process.
            unsafe { NSPasteboardTypeTIFF },
            image::ImageFormat::Tiff,
        );
        let mut watch = ClipboardWatchSource::new(Arc::new(AtomicBool::new(true)));
        write_flavor(flavor_type, &encoded_2x2(format));
        let events = watch.poll();
        assert_eq!(events.len(), 1, "the image write must be observed once");
        assert!(
            watch.poll().is_empty(),
            "no further observation without a change"
        );
        let mut reader = SystemImageReader::new(watch.observer());
        let png = reader
            .read()
            .expect("read must succeed")
            .expect("the observed image must still be on the board");
        let decoded = image::load_from_memory(&png).expect("valid png");
        assert_eq!((decoded.width(), decoded.height()), (2, 2), "{flavor:?}");

        let (flavor, flavor_type, format) = (
            crate::clipboard::ImageFlavor::Png,
            // SAFETY: constant string extern static exported by AppKit, valid
            // for the entire lifetime of the process.
            unsafe { NSPasteboardTypePNG },
            image::ImageFormat::Png,
        );
        let mut watch = ClipboardWatchSource::new(Arc::new(AtomicBool::new(true)));
        write_flavor(flavor_type, &encoded_2x2(format));
        let events = watch.poll();
        assert_eq!(
            events.len(),
            1,
            "a PNG-only board must be observed exactly once"
        );
        let mut reader = SystemImageReader::new(watch.observer());
        let png = reader
            .read()
            .expect("read must succeed")
            .expect("the observed PNG must still be on the board");
        let decoded = image::load_from_memory(&png).expect("valid png");
        assert_eq!((decoded.width(), decoded.height()), (2, 2), "{flavor:?}");
    }
}
