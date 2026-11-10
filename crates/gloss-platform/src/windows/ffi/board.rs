//! 剪贴板查询面：序列号、格式清单、图像格式存在性与图像数据读写。
//!
//! 序列号随每次剪贴板更新单调递增（清板与每次放回各记一次跳变）：以基线
//! 差判定“目标应用确实写入了”，注入被静默丢弃（UIPI）或目标未响应时序
//! 列号不动，兜底轮按未确认处理，剪贴板监视（哨兵）以同一序列号做变更轮
//! 询，写回因此按真实跳变数自报、供轮次写盘归因对齐哨兵口径。格式清单只
//! 读计数、不取数据，供快照区分空板与非文本内容。图像面只做存在性查询与
//! 数据读写：存在性是不开板的纯元数据查询（哨兵的类型判定），读取在开板
//! 窗口内取全局内存对象并拷贝（字节上限在锁页与拷贝之前判定；多格式读取
//! 收在一次开板窗口内，快照要的是同一代板内容），写入服务快照恢复——开
//! 板窗口内清板后按捕获的原格式原字节逐个放回。

use std::sync::OnceLock;

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, CountClipboardFormats, EmptyClipboard, GetClipboardData,
    GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
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

pub(crate) fn write_formats(writes: &[(u32, &[u8])]) -> (bool, u64) {
    if writes.is_empty() {
        return (true, 0);
    }
    // SAFETY: a null window handle associates the open with this thread and
    // the clipboard is closed on every path below.
    if unsafe { OpenClipboard(None) }.is_err() {
        return (false, 0);
    }
    let outcome = write_open_formats(writes);
    // SAFETY: matches the successful open above; failures are tolerated since
    // the open is thread-associated and nothing further touches the board.
    if let Err(err) = unsafe { CloseClipboard() } {
        debug!(thread = thread::EVENT, error = %err, "close clipboard failed");
    }
    outcome
}

fn write_open_formats(writes: &[(u32, &[u8])]) -> (bool, u64) {
    let mut jumps = 0u64;
    // SAFETY: the clipboard is open on this thread; clearing the board is the
    // restore contract (the captured snapshot replaces the content wholesale).
    if unsafe { EmptyClipboard() }.is_err() {
        return (false, jumps);
    }
    jumps += 1;
    for (format, bytes) in writes {
        let Some(handle) = movable_handle(bytes) else {
            debug!(thread = thread::EVENT, "snapshot handle allocation failed");
            return (false, jumps);
        };
        // SAFETY: the clipboard is open on this thread; on success the board
        // adopts `handle` and frees it itself, on failure it is freed below.
        if unsafe { SetClipboardData(*format, Some(HANDLE(handle.0))) }.is_err() {
            // SAFETY: the failed handle was never adopted by the board.
            if let Err(err) = unsafe { GlobalFree(Some(handle)) } {
                debug!(thread = thread::EVENT, error = %err, "free clipboard data failed");
            }
            debug!(thread = thread::EVENT, format, "snapshot write failed");
            return (false, jumps);
        }
        jumps += 1;
    }
    (true, jumps)
}

pub(crate) fn read_formats(formats: &[u32], max_bytes: usize) -> Vec<Option<Vec<u8>>> {
    // SAFETY: a null window handle associates the open with this thread and
    // the clipboard is closed on every path below.
    if unsafe { OpenClipboard(None) }.is_err() {
        return formats.iter().map(|_| None).collect();
    }
    let slots = formats
        .iter()
        .map(|&format| match read_open_format(format, max_bytes) {
            FormatData::Bytes(bytes) => Some(bytes),
            FormatData::Absent | FormatData::OverLimit => None,
        })
        .collect();
    // SAFETY: matches the successful open above; failures are tolerated since
    // the open is thread-associated and nothing further touches the board.
    if let Err(err) = unsafe { CloseClipboard() } {
        debug!(thread = thread::EVENT, error = %err, "close clipboard failed");
    }
    slots
}

fn movable_handle(bytes: &[u8]) -> Option<HGLOBAL> {
    // SAFETY: a fresh moveable allocation sized to the bytes; the returned
    // handle stays owned by this function until handed to the caller.
    let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }.ok()?;
    // SAFETY: `handle` is the live allocation from above; a null lock declines
    // and frees it. The pointer stays within this scope and the bytes are
    // copied before the unlock below.
    let base = unsafe { GlobalLock(handle) }.cast::<u8>();
    if base.is_null() {
        // SAFETY: the allocation was never adopted by the board.
        if let Err(err) = unsafe { GlobalFree(Some(handle)) } {
            debug!(thread = thread::EVENT, error = %err, "free clipboard data failed");
        }
        return None;
    }
    // SAFETY: the allocation holds at least `bytes.len()` bytes and the lock
    // above pinned it for this copy.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), base, bytes.len()) };
    // SAFETY: exactly one unlock pairs the lock above; a false return carrying
    // NO_ERROR is success (the read path tolerates it the same way).
    if let Err(err) = unsafe { GlobalUnlock(handle) }
        && err.code().is_err()
    {
        debug!(thread = thread::EVENT, error = %err, "unlock clipboard data failed");
    }
    Some(handle)
}
