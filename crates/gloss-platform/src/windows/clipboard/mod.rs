//! 剪贴板：文本写入、图像格式的存在性判定与数据读取的查询面。
//!
//! 双栈分工对齐系统（macOS）实现：文本写入走跨平台兜底库（其系统
//! （Windows）面就是剪贴板接口（Win32）的文本格式封装，与恢复序列同一
//! 写入口径）；图像不走兜底库——其读图无尺寸设界且不认位图（DIB）家族，
//! 取材经外部函数接口（FFI）模块直读剪贴板接口（Win32）：便携式网络图形
//! （PNG）注册格式、版本 5 位图（CF_DIBV5）与位图（CF_DIB）按优先序取
//! 首个在板格式的数据字节（字节上限在锁页与拷贝之前判定）。哨兵与取材
//! 读取器共用序列号与图像存在性两个纯查询（序列号是系统（macOS）变更
//! 计数的对应物）；两侧同在平台事件线程上运行，同线程顺序消费。
//!
//! 日志红线：图像只记字节数与像素尺寸，内容与字节永不落日志、不落盘、
//! 不进错误消息。

mod image;
mod watch;

pub use image::SystemImageReader;
pub use watch::{
    ClipboardWatchSource, PasteboardImageObserved, PasteboardObserver, RoundWrites, WatchRoundFlag,
};

use gloss_core::ports::ClipboardWriter;

use crate::ffi::board::{self, FormatData};

pub struct SystemClipboardWriter;

impl ClipboardWriter for SystemClipboardWriter {
    fn copy_text(&self, text: &str) -> Result<(), String> {
        arboard::Clipboard::new()
            .and_then(|mut clipboard| clipboard.set_text(text.to_owned()))
            .map_err(|err| err.to_string())
    }
}

pub(crate) fn clipboard_sequence_number() -> Option<u64> {
    crate::ffi::board::sequence_number()
}

pub(crate) fn clipboard_format_count() -> Option<u32> {
    crate::ffi::board::format_count()
}

pub(crate) fn clipboard_has_image() -> bool {
    board::has_image()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageFlavor {
    Png,
    DibV5,
    Dib,
}

pub(crate) enum FlavorRead {
    Absent,
    OverLimit,
    Data(ImageFlavor, Vec<u8>),
}

pub(crate) fn clipboard_image_flavor(max_bytes: usize) -> FlavorRead {
    let png = board::png_format().map(|format| (ImageFlavor::Png, format));
    let candidates = [
        png,
        Some((ImageFlavor::DibV5, board::CF_DIBV5)),
        Some((ImageFlavor::Dib, board::CF_DIB)),
    ];
    for (flavor, format) in candidates.into_iter().flatten() {
        match board::read_format(format, max_bytes) {
            FormatData::Bytes(bytes) => return FlavorRead::Data(flavor, bytes),
            FormatData::OverLimit => return FlavorRead::OverLimit,
            FormatData::Absent => continue,
        }
    }
    FlavorRead::Absent
}
