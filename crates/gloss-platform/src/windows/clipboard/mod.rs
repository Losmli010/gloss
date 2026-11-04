//! 剪贴板占位：文本复制的桩与图片哨兵、取材读取器的同名镜像。
//!
//! 文本复制在系统（macOS）实现里走跨平台兜底库；系统（Windows）的对应
//! 实现（剪贴板接口（Win32），图像取位图（CF_DIB／CF_DIBV5）与便携式网络
//! 图形（PNG））按移植方案后续阶段补齐，本桩先行占位。

use gloss_core::model::GlossError;

mod image;
mod watch;

pub use image::ClipboardImageReader;
pub use watch::{
    ClipboardWatchSource, PasteboardImageObserved, PasteboardObserver, RoundWrites, WatchRoundFlag,
};

pub fn copy_text(_text: &str) -> Result<(), String> {
    Err(GlossError::PlatformUnsupported.to_string())
}
