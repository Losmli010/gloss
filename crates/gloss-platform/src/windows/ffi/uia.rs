//! 用户界面自动化（UIA）文本选区读取：焦点元素的文本模式（TextPattern）选区。
//!
//! 只读焦点元素的选中范围文本，是向目标应用发起的同步跨进程调用，必须
//! 运行在套间初始化过的平台事件线程（线程约束见组件对象模型模块头）。
//! 应用不支持文本模式（自绘界面）或调用失败以错误码上抛；无选中范围与
//! 空串都按空选区交调用方解读；文本超出上限时不取全文、以空值交回
//! ——调用方按读取不可用处理（与系统（macOS）读取器的超限语义同构）。

use windows::Win32::UI::Accessibility::{
    IUIAutomation, IUIAutomationTextPattern, UIA_TextPatternId,
};
use windows::core::Interface;

pub(crate) fn selected_text(
    automation: &IUIAutomation,
    max_bytes: usize,
) -> Result<Option<String>, i32> {
    // SAFETY: `automation` is a valid interface reference created on this
    // thread's STA apartment; the returned element reference is checked below.
    let focused = unsafe { automation.GetFocusedElement() }.map_err(|err| err.code().0)?;
    // SAFETY: `focused` is a valid element reference just returned; the
    // pattern id is the system TextPattern constant.
    let pattern = unsafe { focused.GetCurrentPattern(UIA_TextPatternId) }
        .ok()
        .and_then(|raw| raw.cast::<IUIAutomationTextPattern>().ok());
    let Some(pattern) = pattern else {
        return Err(UIA_TEXT_PATTERN_UNSUPPORTED);
    };
    // SAFETY: `pattern` is a valid interface reference; the call returns the
    // selected ranges owned by the callee and released with the reference.
    let ranges = unsafe { pattern.GetSelection() }.map_err(|err| err.code().0)?;
    // SAFETY: `ranges` is a valid array reference just returned above.
    let count = unsafe { ranges.Length() }.map_err(|err| err.code().0)?;
    if count <= 0 {
        return Ok(Some(String::new()));
    }
    // SAFETY: the index is within the `Length` range reported above.
    let range = unsafe { ranges.GetElement(0) }.map_err(|err| err.code().0)?;
    // SAFETY: `range` is a valid reference just returned; a negative length
    // asks for as much text as the provider holds.
    let text = unsafe { range.GetText(-1) }.map_err(|err| err.code().0)?;
    let wide: &[u16] = &text;
    if wide.len() * 2 > max_bytes {
        return Ok(None);
    }
    Ok(Some(String::from_utf16_lossy(wide)))
}

const UIA_TEXT_PATTERN_UNSUPPORTED: i32 = -1;
