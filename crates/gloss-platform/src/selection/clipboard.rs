//! 剪贴板兜底读取（模拟复制）：`ClipboardFallbackReader`。
//!
//! AX 读不到选区时的兜底通道：保存剪贴板原内容 → 注入复制快捷键（macOS
//! Cmd+C / Windows Ctrl+C）→ 轮询确认目标应用写入 → 读回文本 → 恢复原
//! 内容。注入会把快捷键投给用户的前台应用（可能误拷非文本对象），因此
//! 只有能完整保全原内容时才注入：空剪贴板（恢复 = 清空）或纯文本（恢复
//! = 写回）才继续；文件/图像等 arboard 无法保全的内容直接放弃兜底。
//!
//! 恢复时机（08 §7.1）：目标应用写入粘贴板是异步的，恢复过早会被应用的
//! 写入覆盖掉原文——以「写入确认（macOS kPasteboardModified / Windows
//! 序列号变化）或 2s 超时」为界，之后才恢复；无论读取成败都必须恢复，
//! 恢复失败留痕但不吞掉主结果。整条流程必须运行在平台事件线程（调用方
//! 保证亲和性），恢复操作同线程执行。
//!
//! 平台门控与纯逻辑切分同 accessibility.rs / events/mouse.rs：注入与确认
//! 信号是平台专属（仅 macOS/Windows 提供），「轮询等待写入完成」的时序
//! 逻辑是纯函数，全平台单测。

use std::time::{Duration, Instant};

/// 写入确认的轮询上限（08 §7.1）：超过即认为目标应用未响应复制。
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
const WRITE_CONFIRM_TIMEOUT: Duration = Duration::from_secs(2);
/// 写入确认的轮询节奏：远低于可感知延迟，高于常见调度抖动。
const WRITE_POLL_INTERVAL: Duration = Duration::from_millis(30);

/// 轮询等待剪贴板变化（纯逻辑，全平台单测）：probe 返回当前代数（None
/// 表示本轮读取失败，继续等），与 baseline 不同即认为目标应用已完成写
/// 入；到 deadline 仍未变化返回 `false`，调用方据此走超时恢复路径。
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
fn wait_for_write(
    baseline: u64,
    mut probe: impl FnMut() -> Option<u64>,
    deadline: Instant,
) -> bool {
    loop {
        if probe().is_some_and(|current| current != baseline) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(WRITE_POLL_INTERVAL);
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod imp {
    use std::time::Instant;

    use arboard::Clipboard;
    use rdev::{EventType, Key};

    #[cfg(target_os = "macos")]
    use core_foundation_sys::base::{CFRelease, CFTypeRef, kCFAllocatorDefault};
    #[cfg(target_os = "macos")]
    use core_foundation_sys::string::{
        CFStringCreateWithCString, CFStringRef, kCFStringEncodingUTF8,
    };
    #[cfg(target_os = "macos")]
    use std::ffi::{CStr, c_void};

    use gloss_core::log::{debug, error, thread};
    use gloss_core::model::GlossError;

    use super::{WRITE_CONFIRM_TIMEOUT, wait_for_write};

    /// 复制快捷键的修饰键：macOS 为 Cmd（rdev 映射 Meta），Windows 为 Ctrl。
    #[cfg(target_os = "macos")]
    const COPY_MODIFIER: Key = Key::MetaLeft;
    #[cfg(target_os = "windows")]
    const COPY_MODIFIER: Key = Key::ControlLeft;

    // ---- macOS：Pasteboard C API（写入确认信号与内容可保全性判定）----

    /// 系统剪贴板的注册名（kPasteboardClipboard 的字符串值）。
    #[cfg(target_os = "macos")]
    const PASTEBOARD_NAME: &CStr = c"com.apple.pasteboard.clipboard";

    /// kPasteboardModified：自上次经本地引用访问以来全局粘贴板已被修改；
    /// 标志在 Synchronize 调用时被消费，探针侧需闩锁。
    #[cfg(target_os = "macos")]
    const K_PASTEBOARD_MODIFIED: u32 = 1 << 0;

    /// macOS 粘贴板句柄：CF 不透明类型，Create 返回 +1 引用。
    #[cfg(target_os = "macos")]
    type PasteboardRef = *mut c_void;

    #[cfg(target_os = "macos")]
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        /// 创建指向指定名称全局粘贴板的本地引用（+1），失败返回非零状态码。
        fn PasteboardCreate(name: CFStringRef, out: *mut PasteboardRef) -> i32;
        /// 与全局粘贴板同步，返回标志集（含 kPasteboardModified）。
        fn PasteboardSynchronize(pasteboard: PasteboardRef) -> u32;
        /// 返回粘贴板条目数，失败返回非零状态码。
        fn PasteboardGetItemCount(pasteboard: PasteboardRef, out_count: *mut u32) -> i32;
    }

    // ---- Windows：user32（同上）----

    /// CF_UNICODETEXT 格式 id。
    #[cfg(target_os = "windows")]
    const CF_UNICODETEXT: u32 = 13;

    #[cfg(target_os = "windows")]
    #[link(name = "user32")]
    unsafe extern "system" {
        /// 系统级剪贴板序号，每次内容变更递增。
        fn GetClipboardSequenceNumber() -> u32;
        /// 剪贴板是否存在指定格式（BOOL，非零为真）。
        fn IsClipboardFormatAvailable(format: u32) -> i32;
        /// 剪贴板当前格式总数，空剪贴板为 0。
        fn CountClipboardFormats() -> i32;
    }

    /// CF 对象守卫：出作用域即 CFRelease，杜绝错误路径上的手工释放遗漏。
    #[cfg(target_os = "macos")]
    struct CfGuard(CFTypeRef);

    #[cfg(target_os = "macos")]
    impl Drop for CfGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: `self.0` 是创建函数返回的 +1 引用（NULL 已判空），
                // 此处是唯一释放点，恰好归还一次。
                unsafe { CFRelease(self.0) };
            }
        }
    }

    /// 按名构造 CFString：属性/名称常量在现行系统已不导出数据符号（见
    /// accessibility.rs），改用稳定字符串值。返回 +1 引用，交给守卫释放。
    ///
    /// # Safety
    ///
    /// 分配失败返回 NULL；非 NULL 引用必须恰好释放一次。
    #[cfg(target_os = "macos")]
    unsafe fn create_cf_string(name: &CStr) -> Option<CFStringRef> {
        // SAFETY: `name` 是 NUL 结尾的有效 C 字符串，编码为受支持的 UTF-8；
        // 分配失败返回 NULL，由调用方判别。
        let s = unsafe {
            CFStringCreateWithCString(kCFAllocatorDefault, name.as_ptr(), kCFStringEncodingUTF8)
        };
        (!s.is_null()).then_some(s)
    }

    /// 打开的系统粘贴板：引用与名称字符串一并用守卫释放（Create 对名称的
    /// 所有权约定未文档化，保守保活到引用销毁）。
    #[cfg(target_os = "macos")]
    struct OpenPasteboard {
        raw: PasteboardRef,
        _name: CfGuard,
    }

    #[cfg(target_os = "macos")]
    impl Drop for OpenPasteboard {
        fn drop(&mut self) {
            // SAFETY: `raw` 是 PasteboardCreate 返回的 +1 引用（非空已在
            // 创建时判别），此处是唯一释放点，恰好归还一次。
            unsafe { CFRelease(self.raw as CFTypeRef) };
        }
    }

    /// 打开系统剪贴板的本地引用。
    #[cfg(target_os = "macos")]
    fn open_pasteboard() -> Option<OpenPasteboard> {
        // SAFETY: 入参是 NUL 结尾的字面量，满足 create_cf_string 的契约；
        // 返回的 +1 引用交由守卫恰好释放一次。
        let name = unsafe { create_cf_string(PASTEBOARD_NAME) }?;
        let name_guard = CfGuard(name as CFTypeRef);
        let mut raw: PasteboardRef = std::ptr::null_mut();
        // SAFETY: `name` 是有效 CFString 引用，出参指向栈上变量；失败时不
        // 写入出参。
        let status = unsafe { PasteboardCreate(name, &mut raw) };
        if status != 0 || raw.is_null() {
            return None;
        }
        Some(OpenPasteboard {
            raw,
            _name: name_guard,
        })
    }

    /// 写入确认信号源：注入前建立基线，之后轮询代数变化。
    #[cfg(target_os = "macos")]
    struct ChangeMonitor {
        pb: OpenPasteboard,
        latched: bool,
    }

    #[cfg(target_os = "windows")]
    struct ChangeMonitor {
        baseline_seq: u32,
    }

    #[cfg(target_os = "macos")]
    impl ChangeMonitor {
        /// 建立基线：同步一次消费既有 modified 标志。
        fn new() -> Option<Self> {
            let pb = open_pasteboard()?;
            // SAFETY: `raw` 是有效 +1 引用，存活至本结构销毁。
            unsafe { PasteboardSynchronize(pb.raw) };
            Some(Self { pb, latched: false })
        }

        /// macOS 侧以闩锁后的 0/1 表达代数，基线恒为 0。
        fn baseline(&self) -> u64 {
            0
        }

        /// 返回当前代数：任一次 Synchronize 报告 modified 即闩锁为 1
        /// （标志被消费，不能依赖后续调用，见 K_PASTEBOARD_MODIFIED）。
        fn generation(&mut self) -> u64 {
            // SAFETY: `raw` 是有效 +1 引用，存活至本结构销毁。
            let flags = unsafe { PasteboardSynchronize(self.pb.raw) };
            if flags & K_PASTEBOARD_MODIFIED != 0 {
                self.latched = true;
            }
            u64::from(self.latched)
        }
    }

    #[cfg(target_os = "windows")]
    impl ChangeMonitor {
        /// 建立基线：记录注入前的系统序号。
        fn new() -> Option<Self> {
            Some(Self {
                // SAFETY: 无前置条件的系统查询。
                baseline_seq: unsafe { GetClipboardSequenceNumber() },
            })
        }

        fn baseline(&self) -> u64 {
            u64::from(self.baseline_seq)
        }

        /// 返回当前系统序号（无状态计数器，无需闩锁）。
        fn generation(&mut self) -> u64 {
            // SAFETY: 无前置条件的系统查询。
            u64::from(unsafe { GetClipboardSequenceNumber() })
        }
    }

    /// 已保存的原剪贴板内容：文本写回，空则清空。
    enum SavedContent {
        Text(String),
        Empty,
    }

    /// 剪贴板兜底读取实现：无状态，可按需构造。
    #[derive(Debug, Default, Clone, Copy)]
    pub struct ClipboardFallbackReader;

    impl ClipboardFallbackReader {
        /// 创建兜底读取器。
        pub fn new() -> Self {
            Self
        }

        /// 兜底读取前台应用的选中文本（模拟复制路径）。
        ///
        /// 调用方保证：在平台事件线程上调用（08 §4.4 亲和性，恢复同线程）。
        pub fn read(&mut self) -> Result<String, GlossError> {
            let mut clipboard = match Clipboard::new() {
                Ok(clipboard) => clipboard,
                Err(err) => {
                    debug!(
                        thread = thread::EVENT,
                        error = %err,
                        "clipboard unavailable, fallback read skipped"
                    );
                    return Err(GlossError::SelectionUnavailable);
                }
            };
            // Err 直接上抛：未注入过，剪贴板未被触碰，无需恢复。
            let saved = save_original(&mut clipboard)?;
            let outcome = self.attempt(&mut clipboard);
            // 08 §7.1：无论读取成败都必须恢复原内容；恢复失败留痕但不吞掉
            // 主结果（此时用户剪贴板停留在选中文本上，属可诊断的系统异常）。
            if let Err(err) = restore(&mut clipboard, saved) {
                error!(
                    thread = thread::EVENT,
                    error = %err,
                    "failed to restore clipboard"
                );
            }
            outcome
        }

        /// 注入复制快捷键并等待写入确认，读回选中文本。不含恢复职责。
        fn attempt(&mut self, clipboard: &mut Clipboard) -> Result<String, GlossError> {
            let Some(mut monitor) = ChangeMonitor::new() else {
                debug!(
                    thread = thread::EVENT,
                    "change monitor unavailable, fallback declined"
                );
                return Err(GlossError::SelectionUnavailable);
            };
            inject_copy_key()?;
            let deadline = Instant::now() + WRITE_CONFIRM_TIMEOUT;
            let baseline = monitor.baseline();
            let confirmed = wait_for_write(baseline, || Some(monitor.generation()), deadline);
            if !confirmed {
                debug!(
                    thread = thread::EVENT,
                    "copy write not confirmed within timeout"
                );
                return Err(GlossError::SelectionUnavailable);
            }
            match clipboard.get_text() {
                Ok(text) if !text.is_empty() => Ok(text),
                // 应用确认写入了但内容非文本（如复制了文件），读取失败。
                Ok(_) => {
                    debug!(
                        thread = thread::EVENT,
                        "copy confirmed but clipboard holds no text"
                    );
                    Err(GlossError::SelectionUnavailable)
                }
                Err(err) => {
                    debug!(
                        thread = thread::EVENT,
                        error = %err,
                        "clipboard read after copy failed"
                    );
                    Err(GlossError::SelectionUnavailable)
                }
            }
        }
    }

    /// 保存原内容并判定是否允许注入：能保全才注入。
    fn save_original(clipboard: &mut Clipboard) -> Result<SavedContent, GlossError> {
        match clipboard.get_text() {
            Ok(text) => Ok(SavedContent::Text(text)),
            Err(arboard::Error::ContentNotAvailable) => {
                if is_clipboard_empty() {
                    Ok(SavedContent::Empty)
                } else {
                    // 文件/图像等 arboard 无法保全的内容：放弃兜底，否则
                    // 恢复阶段无法还原用户剪贴板。
                    debug!(
                        thread = thread::EVENT,
                        "clipboard holds non-text content, fallback declined"
                    );
                    Err(GlossError::SelectionUnavailable)
                }
            }
            Err(err) => {
                debug!(
                    thread = thread::EVENT,
                    error = %err,
                    "clipboard read failed, fallback declined"
                );
                Err(GlossError::SelectionUnavailable)
            }
        }
    }

    /// 剪贴板是否为空：判不了（系统查询失败）一律按非空处理，宁可放弃
    /// 兜底也不冒无法恢复的风险。
    #[cfg(target_os = "macos")]
    fn is_clipboard_empty() -> bool {
        let Some(pb) = open_pasteboard() else {
            return false;
        };
        let mut count: u32 = 0;
        // SAFETY: `raw` 是有效 +1 引用，出参指向栈上变量。
        let status = unsafe { PasteboardGetItemCount(pb.raw, &mut count) };
        status == 0 && count == 0
    }

    #[cfg(target_os = "windows")]
    fn is_clipboard_empty() -> bool {
        // SAFETY: 无前置条件的系统查询；无文本且格式总数为 0 才视为空。
        unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 && CountClipboardFormats() == 0 }
    }

    /// 注入复制快捷键：修饰键按下 → C 按下/释放 → 修饰键释放。任何一步
    /// 失败即放弃（未授权时系统会静默忽略注入事件，最终表现为超时）。
    fn inject_copy_key() -> Result<(), GlossError> {
        for event in [
            EventType::KeyPress(COPY_MODIFIER),
            EventType::KeyPress(Key::KeyC),
            EventType::KeyRelease(Key::KeyC),
            EventType::KeyRelease(COPY_MODIFIER),
        ] {
            if let Err(err) = rdev::simulate(&event) {
                debug!(
                    thread = thread::EVENT,
                    error = ?err,
                    "copy key injection failed"
                );
                return Err(GlossError::SelectionUnavailable);
            }
        }
        Ok(())
    }

    /// 恢复原剪贴板内容：文本写回，空则清空。
    fn restore(clipboard: &mut Clipboard, saved: SavedContent) -> Result<(), arboard::Error> {
        match saved {
            SavedContent::Text(text) => clipboard.set_text(text),
            SavedContent::Empty => clipboard.clear(),
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use imp::ClipboardFallbackReader;

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::wait_for_write;

    /// 代数变化即确认：前两轮与基线相同，第三轮变化。
    #[test]
    fn change_is_detected_when_generation_bumps() {
        let mut calls = 0;
        let deadline = Instant::now() + Duration::from_secs(5);
        let confirmed = wait_for_write(
            0,
            || {
                calls += 1;
                Some(u64::from(calls >= 3))
            },
            deadline,
        );
        assert!(confirmed);
        assert_eq!(calls, 3);
    }

    /// 超时路径不卡死：到 deadline 返回 false，总耗时不超过轮询上限。
    #[test]
    fn timeout_returns_false_without_hanging() {
        let start = Instant::now();
        let deadline = start + Duration::from_millis(60);
        assert!(!wait_for_write(0, || Some(0), deadline));
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "timeout path must return promptly"
        );
    }

    /// probe 持续不可用（None）同样只等到 deadline。
    #[test]
    fn unavailable_probe_keeps_polling_until_deadline() {
        let start = Instant::now();
        let deadline = start + Duration::from_millis(50);
        assert!(!wait_for_write(0, || None, deadline));
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "timeout path must return promptly"
        );
    }
}

#[cfg(all(target_os = "macos", test))]
mod live_tests {
    use arboard::Clipboard;

    use super::imp::ClipboardFallbackReader;

    /// 自动化验收（08 §7.1）：预置剪贴板内容 → 兜底读取（成败皆可，验收
    /// 点是恢复）→ 断言原内容完整恢复。
    ///
    /// 注意：本测试会向前台应用注入一次 Cmd+C。CI 无辅助功能授权时注入
    /// 被系统忽略，走 2s 超时路径；本机运行会短暂打断当前焦点应用。
    #[test]
    fn fallback_read_restores_original_clipboard() {
        let mut clipboard = Clipboard::new().expect("clipboard should be available");
        let preset = format!("gloss-restore-{}", std::process::id());
        clipboard.set_text(preset.clone()).expect("preset text");

        let mut reader = ClipboardFallbackReader::new();
        let _ = reader.read();

        let restored = clipboard.get_text().expect("clipboard should be readable");
        assert_eq!(restored, preset, "original clipboard content must survive");
    }

    /// 手动验收入口：在任意文本编辑器选中文字后运行
    /// `cargo test -p gloss-platform -- --ignored --nocapture`，断言模拟
    /// 复制兜底能读出选中文本。CI 无图形会话与授权，不参与常规测试。
    #[test]
    #[ignore = "requires a live GUI session, an active selection and accessibility permission"]
    fn reads_live_selection_via_simulated_copy() {
        let mut reader = ClipboardFallbackReader::new();
        let text = reader
            .read()
            .expect("fallback should read the live selection");
        println!("selected: {text}");
        assert!(!text.is_empty());
    }
}
