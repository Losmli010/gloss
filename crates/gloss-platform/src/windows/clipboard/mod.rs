//! 剪贴板占位：文本写入端口的桩与图片哨兵、取材读取器的同名镜像。
//!
//! 文本复制在系统（macOS）实现里走跨平台兜底库；系统（Windows）的对应
//! 实现（剪贴板接口（Win32），图像取位图（CF_DIB／CF_DIBV5）与便携式网络
//! 图形（PNG））按移植方案后续阶段补齐，本桩先行占位。

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
