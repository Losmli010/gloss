//! 剪贴板图片哨兵源的平台绑定：把共享哨兵形状（平台中立共享层）绑定到
//! 系统（Windows）原语。
//!
//! 共享形状（基线武装、查询节流与取材轮次的计数归因）住平台中立共享层
//! （common）；本模块只提供“读变更计数”与“是否有图”两个原语闭包——
//! 剪贴板序列号与图像格式存在性都是不开板的纯元数据查询，不取数据、
//! 无副作用（系统（Windows）无“允许粘贴”授权流程）——并把哨兵实现核心
//! 层剪贴板监视端口（轮询产出契约观察事件）。构造时读一次序列号记基线：
//! 启动时的既有剪贴板内容不触发。真机人工测试（写真实剪贴板观察哨兵与
//! 取材读取器）留在平台侧。

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use gloss_core::ports::{ClipboardImageObserved, ClipboardWatcher};

pub use crate::common::clipboard_watch::{
    ClipboardWatchSource, PasteboardImageObserved, PasteboardObserver, RoundWrites, WatchRoundFlag,
};

use super::{clipboard_has_image, clipboard_sequence_number};

impl ClipboardWatchSource {
    pub fn new(enabled: Arc<AtomicBool>) -> Self {
        Self::arm(enabled, clipboard_sequence_number)
    }
}

impl ClipboardWatcher for ClipboardWatchSource {
    fn poll(&mut self) -> Vec<ClipboardImageObserved> {
        self.poll_at(
            Instant::now(),
            clipboard_sequence_number,
            clipboard_has_image,
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
    use std::slice;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use arboard::Clipboard;
    use gloss_core::ports::ClipboardImageReader;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};

    use super::*;
    use crate::clipboard::SystemImageReader;
    use crate::ffi::board;

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

    fn encoded_2x2(format: image::ImageFormat) -> Vec<u8> {
        let rgba = vec![
            255u8, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 0,
        ];
        let canvas = image::RgbaImage::from_raw(2, 2, rgba).expect("canvas");
        let mut bytes = Cursor::new(Vec::new());
        canvas.write_to(&mut bytes, format).expect("encode");
        bytes.into_inner()
    }

    fn dib_2x2() -> Vec<u8> {
        let bmp = encoded_2x2(image::ImageFormat::Bmp);
        assert_eq!(&bmp[..2], b"BM", "the bmp file header must be present");
        bmp[14..].to_vec()
    }

    fn write_format(format: u32, bytes: &[u8]) {
        // SAFETY: a movable global allocation sized to the payload; the handle
        // is either taken over by the board below or leaked in this test.
        let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }.expect("alloc");
        // SAFETY: the fresh allocation is valid and pinned by the lock; the
        // bytes are copied out before the unlock.
        let base = unsafe { GlobalLock(handle) };
        assert!(!base.is_null(), "global lock must succeed");
        // SAFETY: the locked range is exactly `bytes.len()` long and writable.
        unsafe { slice::from_raw_parts_mut(base.cast::<u8>(), bytes.len()) }.copy_from_slice(bytes);
        // SAFETY: exactly one unlock pairs the lock above; the final unlock's
        // false return with NO_ERROR is tolerated.
        let _ = unsafe { GlobalUnlock(handle) };
        // SAFETY: a null window handle associates the open with this thread
        // and the clipboard is closed on every path below.
        unsafe { OpenClipboard(None) }.expect("open");
        // SAFETY: the clipboard is open on this thread; emptying discards the
        // previous content and makes this process the owner.
        unsafe { EmptyClipboard() }.expect("empty");
        // SAFETY: the clipboard is open and the handle carries the bytes
        // written above; ownership transfers to the board on success, and a
        // null return leaves the handle leaked in this test process.
        let set = unsafe { SetClipboardData(format, Some(HANDLE(handle.0))) };
        // SAFETY: matches the open above; the close runs on the failure path
        // too, so a failed write never leaves the board open in this process.
        unsafe { CloseClipboard() }.expect("close");
        set.expect("set");
    }

    #[test]
    #[ignore = "需图形会话：写真实剪贴板并观察哨兵与取材读取器"]
    fn watch_and_reader_carry_a_real_image_copy_on_windows() {
        let text_before = Clipboard::new()
            .ok()
            .and_then(|mut clipboard| clipboard.get_text().ok());
        let _restore = RestoreOnDrop { text: text_before };

        let mut watch = ClipboardWatchSource::new(Arc::new(AtomicBool::new(true)));
        write_format(board::CF_DIB, &dib_2x2());
        let events = watch.poll();
        assert_eq!(
            events.len(),
            1,
            "the image write must be observed once (CF_DIB)"
        );
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
        assert_eq!((decoded.width(), decoded.height()), (2, 2), "CF_DIB");

        let png_format = board::png_format().expect("the PNG format must register");
        let mut watch = ClipboardWatchSource::new(Arc::new(AtomicBool::new(true)));
        write_format(png_format, &encoded_2x2(image::ImageFormat::Png));
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
        assert_eq!((decoded.width(), decoded.height()), (2, 2), "PNG format");
    }
}
