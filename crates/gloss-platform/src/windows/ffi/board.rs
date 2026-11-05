//! 剪贴板序列号与格式清单：注入写入的确认探针与快照放弃判定。
//!
//! 序列号随每次剪贴板更新单调递增：以基线差判定“目标应用确实写入了”，
//! 注入被静默丢弃（UIPI）或目标未响应时序列号不动，兜底轮按未确认处理。
//! 格式清单只读计数、不取数据，供快照区分空板与非文本内容。

use windows::Win32::System::DataExchange::{
    CloseClipboard, CountClipboardFormats, GetClipboardSequenceNumber, OpenClipboard,
};

use gloss_core::log::{debug, thread};

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
