//! 剪贴板查询面：序列号、格式清单、图像格式存在性与图像数据读取。
//!
//! 序列号随每次剪贴板更新单调递增：以基线差判定“目标应用确实写入了”，
//! 注入被静默丢弃（UIPI）或目标未响应时序列号不动，兜底轮按未确认处理，
//! 剪贴板监视（哨兵）以同一序列号做变更轮询。格式清单只读计数、不取数据，
//! 供快照区分空板与非文本内容。图像面只做存在性查询与数据读取：存在性是
//! 不开板的纯元数据查询（哨兵的类型判定），读取在开板窗口内取全局内存对象
//! 并拷贝（字节上限在锁页与拷贝之前判定）。

use std::sync::OnceLock;

use windows::Win32::Foundation::HGLOBAL;
use windows::Win32::System::DataExchange::{
    CloseClipboard, CountClipboardFormats, GetClipboardData, GetClipboardSequenceNumber,
    IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW,
};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::core::w;

use gloss_core::log::{debug, thread};

pub(crate) const CF_DIB: u32 = 8;

pub(crate) const CF_DIBV5: u32 = 17;

pub(crate) fn sequence_number() -> Option<u64> {
    // SAFETY: a pure query with no preconditions; a zero return means the
    // sequence number is unavailable.
    let sequence = unsafe { GetClipboardSequenceNumber() };
    (sequence != 0).then_some(u64::from(sequence))
}

pub(crate) fn format_count() -> Option<u32> {
    // SAFETY: a null window handle associates the open with this thread and
    // the clipboard is closed on every path below.
    unsafe { OpenClipboard(None) }.ok()?;
    // SAFETY: read-only query while the clipboard is open on this thread.
    let count = unsafe { CountClipboardFormats() } as u32;
    // SAFETY: matches the successful open above; failures are tolerated since
    // the open is thread-associated and nothing further touches the board.
    if let Err(err) = unsafe { CloseClipboard() } {
        debug!(thread = thread::EVENT, error = %err, "close clipboard failed");
    }
    Some(count)
}

pub(crate) fn png_format() -> Option<u32> {
    static PNG_FORMAT: OnceLock<Option<u32>> = OnceLock::new();
    *PNG_FORMAT.get_or_init(|| {
        // SAFETY: the name is a compile-time literal wide string valid for
        // the entire call; registration has no other preconditions.
        let id = unsafe { RegisterClipboardFormatW(w!("PNG")) };
        (id != 0).then_some(id)
    })
}

pub(crate) fn has_image() -> bool {
    png_format().is_some_and(is_format_available)
        || is_format_available(CF_DIBV5)
        || is_format_available(CF_DIB)
}

fn is_format_available(format: u32) -> bool {
    // SAFETY: a pure availability query; the absent format maps to an error
    // and no board state is held across the call.
    unsafe { IsClipboardFormatAvailable(format) }.is_ok()
}

pub(crate) enum FormatData {
    Absent,
    OverLimit,
    Bytes(Vec<u8>),
}

pub(crate) fn read_format(format: u32, max_bytes: usize) -> FormatData {
    // SAFETY: a null window handle associates the open with this thread and
    // the clipboard is closed on every return path below.
    if unsafe { OpenClipboard(None) }.is_err() {
        return FormatData::Absent;
    }
    let outcome = read_open_format(format, max_bytes);
    // SAFETY: matches the successful open above; failures are tolerated since
    // the open is thread-associated and nothing further touches the board.
    if let Err(err) = unsafe { CloseClipboard() } {
        debug!(thread = thread::EVENT, error = %err, "close clipboard failed");
    }
    outcome
}

fn read_open_format(format: u32, max_bytes: usize) -> FormatData {
    // SAFETY: the clipboard is open on this thread; an absent format maps to
    // an error and the returned handle is owned by the board, not freed here.
    let data = match unsafe { GetClipboardData(format) } {
        Ok(data) => data,
        Err(_) => return FormatData::Absent,
    };
    let source = HGLOBAL(data.0);
    // SAFETY: `source` is the global memory object the board just handed out
    // and stays valid for the lock-unlock pair below.
    let size = unsafe { GlobalSize(source) };
    if size > max_bytes {
        return FormatData::OverLimit;
    }
    // SAFETY: `source` is valid and pinned by the lock; a null lock declines
    // the read. The pointer stays within this scope and the bytes are copied
    // out before the unlock below.
    let bytes = unsafe {
        let base = GlobalLock(source).cast::<u8>();
        if base.is_null() {
            return FormatData::Absent;
        }
        std::slice::from_raw_parts(base, size).to_vec()
    };
    // SAFETY: exactly one unlock pairs the lock above; the final unlock
    // reports success through a false return carrying NO_ERROR (a zero code).
    if let Err(err) = unsafe { GlobalUnlock(source) }
        && err.code().is_err()
    {
        debug!(thread = thread::EVENT, error = %err, "unlock clipboard data failed");
    }
    FormatData::Bytes(bytes)
}
