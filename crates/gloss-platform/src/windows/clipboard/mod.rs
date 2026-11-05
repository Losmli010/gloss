//! 剪贴板：文本写入端口的桩与图片哨兵、取材读取器的同名镜像，加序列号与
//! 格式清单的查询面。
//!
//! 文本复制在系统（macOS）实现里走跨平台兜底库；系统（Windows）的对应
//! 实现（剪贴板接口（Win32），图像取位图（CF_DIB／CF_DIBV5）与便携式网络
//! 图形（PNG））按移植方案后续阶段补齐，本桩先行占位。序列号与格式计数
//! 经外部函数接口（FFI）模块的纯查询承载，供选区兜底与后续阶段的哨兵共用。

use gloss_core::model::GlossError;
use gloss_core::ports::ClipboardWriter;

mod image;
mod watch;

pub use image::SystemImageReader;
pub use watch::{
    ClipboardWatchSource, PasteboardImageObserved, PasteboardObserver, RoundWrites, WatchRoundFlag,
};

pub struct SystemClipboardWriter;

impl ClipboardWriter for SystemClipboardWriter {
    fn copy_text(&self, _text: &str) -> Result<(), String> {
        Err(GlossError::PlatformUnsupported.to_string())
    }
}

pub(crate) fn clipboard_sequence_number() -> Option<u64> {
    crate::ffi::board::sequence_number()
}

pub(crate) fn clipboard_format_count() -> Option<u32> {
    crate::ffi::board::format_count()
}
