//! 剪贴板兜底读取（模拟复制）：`ClipboardFallbackReader`。
//!
//! AX 读不到选区时的兜底通道：保存剪贴板原内容 → 注入复制快捷键
//! （Cmd+C）→ 轮询确认目标应用写入 → 读回文本 → 恢复原内容。注入会把
//! 快捷键投给用户的前台应用（可能误拷非文本对象），因此只有能完整保全
//! 原内容时才注入：空剪贴板（恢复 = 清空）或只含纯文本 flavor（恢复 =
//! 写回）才继续；富文本（text+HTML/RTF 混合，set_text 会降级）与文件/
//! 图像等无法保全的内容直接放弃兜底。
//!
//! 恢复时机（08 §7.1）：目标应用写入粘贴板是异步的，恢复过早会被应用的
//! 写入覆盖掉原文——以「写入确认（kPasteboardModified）或 2s 超时」为
//! 界，确认后也持续读到 deadline（避开 clear→setData 的半写入间隙），
//! 之后才恢复；读取成功且窗口期内剪贴板未被再次改动才恢复，读取失败按
//! 原内容恢复（best-effort），恢复失败留痕但不吞掉主结果。整条流程必须
//! 运行在平台事件线程（调用方保证亲和性），恢复操作同线程执行。
//!
//! 「轮询等待写入完成」的时序逻辑是纯函数（见 `wait_for_write`），单测
//! 覆盖；注入与确认信号依赖系统 API，归 `imp` 模块。

use std::time::{Duration, Instant};

/// 写入确认的轮询上限（08 §7.1）：超过即认为目标应用未响应复制。
const WRITE_CONFIRM_TIMEOUT: Duration = Duration::from_secs(2);
/// 写入确认的轮询节奏：远低于可感知延迟，高于常见调度抖动。
const WRITE_POLL_INTERVAL: Duration = Duration::from_millis(30);

/// 轮询等待剪贴板变化（纯逻辑，单测覆盖）：probe 返回当前代数（None
/// 表示本轮读取失败，继续等），与 baseline 不同即认为目标应用已完成写
/// 入；到 deadline 仍未变化返回 `false`，调用方据此走超时恢复路径。
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

mod imp {
    use std::ffi::{CStr, c_void};
    use std::time::Instant;

    use arboard::Clipboard;
    use core_foundation_sys::base::{CFRelease, CFTypeRef, kCFAllocatorDefault};
    use core_foundation_sys::string::{
        CFStringCreateWithCString, CFStringRef, kCFStringEncodingUTF8,
    };
    use rdev::{EventType, Key};

    use gloss_core::log::{debug, error, thread};
    use gloss_core::model::GlossError;

    use super::{WRITE_CONFIRM_TIMEOUT, WRITE_POLL_INTERVAL, wait_for_write};

    /// 复制快捷键的修饰键：Cmd（rdev 映射 Meta）。
    const COPY_MODIFIER: Key = Key::MetaLeft;

    // ---- Pasteboard C API（写入确认信号与内容可保全性判定）----

    /// 系统剪贴板的注册名（kPasteboardClipboard 的字符串值）。
    const PASTEBOARD_NAME: &CStr = c"com.apple.pasteboard.clipboard";

    /// kPasteboardModified：自上次经本地引用访问以来全局粘贴板已被修改；
    /// 标志在 Synchronize 调用时被消费，探针侧需闩锁。
    const K_PASTEBOARD_MODIFIED: u32 = 1 << 0;

    /// 粘贴板句柄：CF 不透明类型，Create 返回 +1 引用。
    type PasteboardRef = *mut c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        /// 创建指向指定名称全局粘贴板的本地引用（+1），失败返回非零状态码。
        fn PasteboardCreate(name: CFStringRef, out: *mut PasteboardRef) -> i32;
        /// 与全局粘贴板同步，返回标志集（含 kPasteboardModified）。
        fn PasteboardSynchronize(pasteboard: PasteboardRef) -> u32;
        /// 返回粘贴板条目数，失败返回非零状态码。出参是 ItemCount
        /// （MacTypes.h 的 unsigned long，Darwin LP64 下 8 字节）。
        fn PasteboardGetItemCount(pasteboard: PasteboardRef, out_count: *mut usize) -> i32;
    }

    /// CF 对象守卫：出作用域即 CFRelease，杜绝错误路径上的手工释放遗漏。
    struct CfGuard(CFTypeRef);

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
    struct OpenPasteboard {
        raw: PasteboardRef,
        _name: CfGuard,
    }

    impl Drop for OpenPasteboard {
        fn drop(&mut self) {
            // SAFETY: `raw` 是 PasteboardCreate 返回的 +1 引用（非空已在
            // 创建时判别），此处是唯一释放点，恰好归还一次。
            unsafe { CFRelease(self.raw as CFTypeRef) };
        }
    }

    /// 打开系统剪贴板的本地引用。
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
    struct ChangeMonitor {
        pb: OpenPasteboard,
        latched: bool,
    }

    impl ChangeMonitor {
        /// 建立基线：同步一次消费既有 modified 标志。
        fn new() -> Option<Self> {
            let pb = open_pasteboard()?;
            // SAFETY: `raw` 是有效 +1 引用，存活至本结构销毁。
            unsafe { PasteboardSynchronize(pb.raw) };
            Some(Self { pb, latched: false })
        }

        /// 以闩锁后的 0/1 表达代数，基线恒为 0。
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
        /// 阻塞语义：确认目标应用写入最长轮询 [`WRITE_CONFIRM_TIMEOUT`]，
        /// 期间事件线程被占用、其余命令与事件排队（事件源侧缓冲），调用方
        /// 需自行处理在途重复触发。
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
            match &outcome {
                Ok(acquired) => {
                    // 恢复前的廉价校验：窗口期内用户可能自己复制了新内容，
                    // 当前内容仍是我们刚读到的选中文本才恢复——无条件恢复
                    // 会把用户的新拷贝清掉。
                    let current_is_ours =
                        matches!(clipboard.get_text(), Ok(current) if &current == acquired);
                    if current_is_ours {
                        restore_and_log(&mut clipboard, saved);
                    } else {
                        debug!(
                            thread = thread::EVENT,
                            "clipboard changed after read, restore skipped"
                        );
                    }
                }
                // 读取失败路径无从区分「应用的写入」与「用户的新拷贝」，
                // 按原内容恢复（best-effort，窗口期极短）。
                Err(_) => restore_and_log(&mut clipboard, saved),
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
            // 写入确认后读取：应用写粘贴板是 clear → setData 序列，轮询可能
            // 落在「已清空、未写入」的间隙——读到空/读失败不等于失败，继续
            // 轮询到 deadline 再收口，避免恢复原文后被应用的迟到写入覆盖。
            loop {
                if let Ok(text) = clipboard.get_text()
                    && !text.is_empty()
                {
                    return Ok(text);
                }
                if Instant::now() >= deadline {
                    debug!(
                        thread = thread::EVENT,
                        "copy confirmed but no text readable within timeout"
                    );
                    return Err(GlossError::SelectionUnavailable);
                }
                std::thread::sleep(WRITE_POLL_INTERVAL);
            }
        }
    }

    /// 恢复原剪贴板内容；失败留痕但不影响调用方的主结果。
    fn restore_and_log(clipboard: &mut Clipboard, saved: SavedContent) {
        if let Err(err) = restore(clipboard, saved) {
            error!(
                thread = thread::EVENT,
                error = %err,
                "failed to restore clipboard"
            );
        }
    }

    /// 保存原内容并判定是否允许注入：能保全才注入。
    fn save_original(clipboard: &mut Clipboard) -> Result<SavedContent, GlossError> {
        match clipboard.get_text() {
            Ok(text) if clipboard_is_text_only() => Ok(SavedContent::Text(text)),
            Ok(_) => {
                // 有纯文本 flavor 但还携带 HTML/RTF 等其它 flavor（浏览器、
                // Office 复制的典型形态）：set_text 恢复会把富文本降级成
                // 纯文本，违反「完整保全」不变量，放弃兜底。
                debug!(
                    thread = thread::EVENT,
                    "clipboard holds rich text content, fallback declined"
                );
                Err(GlossError::SelectionUnavailable)
            }
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

    /// 剪贴板是否只含纯文本 flavor（无 HTML/RTF 等富文本伴随格式）：
    /// 判不了（系统查询失败）一律按富文本处理，宁可放弃兜底也不冒
    /// 无法恢复的风险。
    fn clipboard_is_text_only() -> bool {
        /// kPasteboardIsTextOnly：全部条目都只含 string flavor（Pasteboard.h）。
        const K_PASTEBOARD_IS_TEXT_ONLY: u32 = 1 << 3;
        let Some(pb) = open_pasteboard() else {
            return false;
        };
        // SAFETY: `raw` 是有效 +1 引用，存活至本结构销毁。
        let flags = unsafe { PasteboardSynchronize(pb.raw) };
        flags & K_PASTEBOARD_IS_TEXT_ONLY != 0
    }

    /// 剪贴板是否为空：判不了（系统查询失败）一律按非空处理，宁可放弃
    /// 兜底也不冒无法恢复的风险。
    fn is_clipboard_empty() -> bool {
        let Some(pb) = open_pasteboard() else {
            return false;
        };
        let mut count: usize = 0;
        // SAFETY: `raw` 是有效 +1 引用，出参指向栈上变量。
        let status = unsafe { PasteboardGetItemCount(pb.raw, &mut count) };
        status == 0 && count == 0
    }

    /// 注入复制快捷键：修饰键按下 → C 按下/释放 → 修饰键释放。任何一步
    /// 失败即放弃；若修饰键已按下而后续步骤失败，补发配对释放，避免前台
    /// 应用停留在孤立的按下态（菜单栏高亮、快捷键半生效）。
    fn inject_copy_key() -> Result<(), GlossError> {
        let sequence = [
            EventType::KeyPress(COPY_MODIFIER),
            EventType::KeyPress(Key::KeyC),
            EventType::KeyRelease(Key::KeyC),
            EventType::KeyRelease(COPY_MODIFIER),
        ];
        for (index, event) in sequence.iter().enumerate() {
            if let Err(err) = rdev::simulate(event) {
                debug!(
                    thread = thread::EVENT,
                    error = ?err,
                    "copy key injection failed"
                );
                if index > 0 {
                    let _ = rdev::simulate(&EventType::KeyRelease(COPY_MODIFIER));
                }
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

#[cfg(test)]
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
    #[ignore = "需授权真机：先把运行测试的终端 App 加入 系统设置→隐私与                安全性→辅助功能（未授权时由 live_test_support 快速失败）"]
    fn reads_live_selection_via_simulated_copy() {
        crate::live_test_support::require_accessibility("clipboard_simulated_copy");
        let mut reader = ClipboardFallbackReader::new();
        let text = reader
            .read()
            .expect("fallback should read the live selection");
        println!("selected: {text}");
        assert!(!text.is_empty());
    }
}
