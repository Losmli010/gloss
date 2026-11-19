//! 剪贴板：图像格式的存在性判定、数据读取与快照恢复的查询面。
//!
//! 双栈分工对齐系统（macOS）实现：文本写入住平台中立共享层
//! （common::clipboard）的跨平台兜底库路径；图像不走兜底库——其读图无尺
//! 寸设界且不认位图（DIB）家族，取材经外部函数接口（FFI）模块直读剪贴
//! 板接口（Win32）：便携式网络图形（PNG）注册格式、版本 5 位图
//! （CF_DIBV5）与位图（CF_DIB）按优先序取首个在板格式的数据字节（字节
//! 上限在锁页与拷贝之前判定）；剪贴板兜底读取的快照恢复另走同面：图像
//! 多格式收在一次开板窗口内逐个捕获原始字节（同一代板内容），恢复时开
//! 板清板后原格式原字节放回（图像内容不经解码重编码，写回字节即捕获字
//! 节；写回按剪贴板序列号的真实跳变数自报，供轮次写盘归因对齐哨兵口
//! 径）。哨兵与取材读取器共用序列号与图像存在性两个纯查询（序列号是
//! 系统（macOS）变更计数的对应物）；两侧同在平台事件线程上运行，同线
//! 程顺序消费。
//!
//! 日志红线：图像只记字节数与像素尺寸，内容与字节永不落日志、不落盘、
//! 不进错误消息。

mod image;
mod watch;

pub use image::SystemImageReader;
pub use watch::{
    ClipboardWatchSource, PasteboardImageObserved, PasteboardObserver, RoundWrites, WatchRoundFlag,
};

use crate::ffi::board::{self, FormatData};

pub(crate) fn clipboard_sequence_number() -> Option<u64> {
    crate::ffi::board::sequence_number()
}

pub(crate) fn clipboard_format_count() -> Option<u32> {
    crate::ffi::board::format_count()
}

pub(crate) fn clipboard_has_image() -> bool {
    board::has_image()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FlavorCapture {
    pub(crate) format: u32,
    pub(crate) bytes: Vec<u8>,
}

pub(crate) fn clipboard_capture_image_flavors(max_bytes: usize) -> Vec<FlavorCapture> {
    let candidates = [
        board::png_format(),
        Some(board::CF_DIBV5),
        Some(board::CF_DIB),
    ];
    let ids = candidates.iter().flatten().copied().collect::<Vec<_>>();
    board::read_formats(&ids, max_bytes)
        .into_iter()
        .zip(ids)
        .filter_map(|(slot, format)| slot.map(|bytes| FlavorCapture { format, bytes }))
        .collect()
}

pub(crate) fn clipboard_write_flavors(flavors: &[FlavorCapture]) -> (bool, u64) {
    let writes = flavors
        .iter()
        .map(|flavor| (flavor.format, flavor.bytes.as_slice()))
        .collect::<Vec<_>>();
    board::write_formats(&writes)
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
