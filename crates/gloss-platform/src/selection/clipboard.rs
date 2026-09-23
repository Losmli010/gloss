//! 剪贴板兜底读取（模拟复制）：`ClipboardFallbackReader`。
//!
//! AX 读不到选区时的兜底通道：快照剪贴板全部条目与 flavor 原始字节 →
//! 注入 Cmd+C → 轮询确认目标应用写入 → 读回 → 按快照恢复。快照拿不全
//! 即整体放弃、不注入；未确认时重注入一轮再等一个完整超时窗口。纯时序
//! 逻辑在 [`wait_for_write`]（单测覆盖），系统 API 交互归 `imp` 模块。

use std::time::{Duration, Instant};

/// 写入确认的轮询上限：超过即认为目标应用未响应复制。
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
    use std::time::Instant;

    use arboard::Clipboard;
    use rdev::{EventType, Key};

    use crate::ffi::cf::{
        self, CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef, CFDataCreate, CFDataGetBytePtr,
        CFDataGetLength, CFDataRef, CFIndex, CFRelease, CFRetain, CFStringRef, CFTypeRef, CfGuard,
        kCFAllocatorDefault,
    };
    use crate::ffi::pasteboard::{
        CLIPBOARD_NAME, K_PASTEBOARD_MODIFIED, PasteboardClear, PasteboardCopyItemFlavorData,
        PasteboardCopyItemFlavors, PasteboardCreate, PasteboardGetItemCount,
        PasteboardGetItemIdentifier, PasteboardItemID, PasteboardPutItemFlavor, PasteboardRef,
        PasteboardSynchronize,
    };

    use gloss_core::log::{debug, error, thread};
    use gloss_core::model::GlossError;

    use super::{WRITE_CONFIRM_TIMEOUT, WRITE_POLL_INTERVAL, wait_for_write};

    /// 复制快捷键的修饰键：Cmd（rdev 映射 Meta）。
    const COPY_MODIFIER: Key = Key::MetaLeft;

    /// 打开的系统粘贴板：引用与名称字符串一并用守卫释放。
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
        let name = cf::cf_string(CLIPBOARD_NAME)?;
        let mut raw: PasteboardRef = std::ptr::null_mut();
        // SAFETY: `name` 是有效 CFString 引用，出参指向栈上变量；失败时不
        // 写入出参。
        let status = unsafe { PasteboardCreate(name.string_ref(), &mut raw) };
        if status != 0 || raw.is_null() {
            return None;
        }
        Some(OpenPasteboard { raw, _name: name })
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

    /// 快照的单个 flavor：flavor 名（Retain 自持，守卫释放）与原始字节。
    struct SavedFlavor {
        name: CfGuard,
        data: Vec<u8>,
    }

    /// 快照的单个条目：标识符原样保留，恢复时按它把 flavor 归并回同一条目。
    struct SavedItem {
        id: PasteboardItemID,
        flavors: Vec<SavedFlavor>,
    }

    /// 原剪贴板的完整快照：空 Vec 即原剪贴板为空（恢复 = 清空后不写入）。
    struct SavedContent(Vec<SavedItem>);

    /// 待执行的恢复：`read()` 的一切退出路径（正常返回、`?` 上抛、panic
    /// 展开）都经 Drop 收口，未显式跳过即按快照恢复原剪贴板。
    struct PendingRestore {
        saved: SavedContent,
        skip: bool,
    }

    impl Drop for PendingRestore {
        fn drop(&mut self) {
            if !self.skip {
                restore_and_log(&self.saved);
            }
        }
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
        /// 调用方保证：在平台事件线程上调用（恢复同线程）。
        /// 阻塞语义：确认目标应用写入最长轮询两轮 [`WRITE_CONFIRM_TIMEOUT`]
        /// （首发未确认时重注入一轮），期间事件线程被占用、其余命令与事件
        /// 排队（事件源侧缓冲），调用方需自行处理在途重复触发。
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
            // 快照失败直接上抛：尚未注入，剪贴板未被触碰。
            let Some(saved) = snapshot_pasteboard() else {
                debug!(
                    thread = thread::EVENT,
                    "original clipboard content cannot be fully preserved, fallback declined"
                );
                return Err(GlossError::SelectionUnavailable);
            };
            // 恢复统一交给守卫收口（见 `PendingRestore`）：除显式跳过外，
            // 一切退出路径都在 drop 时按原内容恢复。
            let mut pending = PendingRestore { saved, skip: false };
            let outcome = self.attempt(&mut clipboard);
            if let Ok(acquired) = &outcome {
                // 窗口期内剪贴板可能已被再次改动：现值仍是我们刚读到的
                // 文本才恢复。
                let current_is_ours =
                    matches!(clipboard.get_text(), Ok(current) if &current == acquired);
                if !current_is_ours {
                    debug!(
                        thread = thread::EVENT,
                        "clipboard changed after read, restore skipped"
                    );
                    pending.skip = true;
                }
            }
            drop(pending);
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
            let baseline = monitor.baseline();
            let mut deadline = Instant::now() + WRITE_CONFIRM_TIMEOUT;
            let mut confirmed = wait_for_write(baseline, || Some(monitor.generation()), deadline);
            if !confirmed {
                debug!(
                    thread = thread::EVENT,
                    "copy write not confirmed, retrying injection once"
                );
                inject_copy_key()?;
                deadline = Instant::now() + WRITE_CONFIRM_TIMEOUT;
                confirmed = wait_for_write(baseline, || Some(monitor.generation()), deadline);
            }
            if !confirmed {
                debug!(
                    thread = thread::EVENT,
                    "copy write not confirmed within timeout"
                );
                return Err(GlossError::SelectionUnavailable);
            }
            // 读到空/读失败不等于失败，继续轮询到 deadline 再收口。
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

    /// 快照原剪贴板全部条目与 flavor 数据；任何一步拿不全即整体失败（调用
    /// 方据此放弃注入，剪贴板保持原样）。
    fn snapshot_pasteboard() -> Option<SavedContent> {
        let Some(pb) = open_pasteboard() else {
            debug!(
                thread = thread::EVENT,
                "pasteboard unavailable, fallback declined"
            );
            return None;
        };
        // SAFETY: `raw` 是有效 +1 引用，存活至本结构销毁；顺带消费既有
        // modified 标志，避免把快照前的历史改动算作目标应用的写入。
        unsafe { PasteboardSynchronize(pb.raw) };
        let mut count: usize = 0;
        // SAFETY: `raw` 是有效 +1 引用，出参指向栈上变量。
        let status = unsafe { PasteboardGetItemCount(pb.raw, &mut count) };
        if status != 0 {
            debug!(
                thread = thread::EVENT,
                status, "item count query failed, fallback declined"
            );
            return None;
        }
        let Ok(count) = CFIndex::try_from(count) else {
            debug!(
                thread = thread::EVENT,
                items = count,
                "item count out of range, fallback declined"
            );
            return None;
        };
        let mut items = Vec::new();
        for index in 1..=count {
            let mut id: PasteboardItemID = std::ptr::null_mut();
            // SAFETY: `raw` 是有效 +1 引用，出参指向刚声明的栈上变量。
            let status = unsafe { PasteboardGetItemIdentifier(pb.raw, index, &mut id) };
            if status != 0 {
                debug!(
                    thread = thread::EVENT,
                    status, "item identifier query failed, fallback declined"
                );
                return None;
            }
            let mut flavors_ref: CFArrayRef = std::ptr::null();
            // SAFETY: `raw` 与 `id` 均为有效引用，出参指向栈上变量；失败时
            // 不写入出参。
            let status = unsafe { PasteboardCopyItemFlavors(pb.raw, id, &mut flavors_ref) };
            if status != 0 {
                debug!(
                    thread = thread::EVENT,
                    status, "flavor list query failed, fallback declined"
                );
                return None;
            }
            let _flavors = CfGuard::new(flavors_ref as CFTypeRef);
            // SAFETY: `flavors_ref` 是刚拷贝的有效 CFArray 引用。
            let flavor_count = unsafe { CFArrayGetCount(flavors_ref) };
            let Ok(flavor_count) = usize::try_from(flavor_count) else {
                debug!(
                    thread = thread::EVENT,
                    "flavor count out of range, fallback declined"
                );
                return None;
            };
            let mut flavors = Vec::with_capacity(flavor_count);
            for flavor_index in 0..flavor_count {
                // SAFETY: 索引在 GetCount 范围内；返回的元素引用由数组持有，
                // 存入快照前先 Retain 自持。
                let flavor = unsafe { CFArrayGetValueAtIndex(flavors_ref, flavor_index as CFIndex) }
                    as CFStringRef;
                // SAFETY: `flavor` 是有效 CF 对象引用，Retain 后交由守卫
                // 恰好释放一次。
                let name = CfGuard::new(unsafe { CFRetain(flavor as CFTypeRef) });
                let mut data: CFDataRef = std::ptr::null();
                // SAFETY: `raw`/`id`/`flavor` 均为有效引用，出参指向栈上
                // 变量；数据未物化（promised）时返回失败。
                let status = unsafe { PasteboardCopyItemFlavorData(pb.raw, id, flavor, &mut data) };
                if status != 0 {
                    debug!(
                        thread = thread::EVENT,
                        status, "flavor data unavailable (promised?), fallback declined"
                    );
                    return None;
                }
                let _data = CfGuard::new(data as CFTypeRef);
                // SAFETY: `data` 是刚拷贝的有效 CFData 引用，长度与字节
                // 指针来自同一对象。
                let (len, bytes_ptr) = unsafe { (CFDataGetLength(data), CFDataGetBytePtr(data)) };
                let Ok(len) = usize::try_from(len) else {
                    debug!(
                        thread = thread::EVENT,
                        "flavor data length out of range, fallback declined"
                    );
                    return None;
                };
                let bytes = if len == 0 {
                    Vec::new()
                } else {
                    // SAFETY: `bytes_ptr` 非 NULL（长度非零的 CFData 保证），
                    // 可读字节数恰为 `len`；复制进快照以脱离 CF 生命周期。
                    let bytes = unsafe { std::slice::from_raw_parts(bytes_ptr, len) };
                    bytes.to_vec()
                };
                flavors.push(SavedFlavor { name, data: bytes });
            }
            items.push(SavedItem { id, flavors });
        }
        Some(SavedContent(items))
    }

    /// 把快照原样写回：先为全部 flavor 预建 CFData（任一失败即整体放弃，
    /// 此时粘贴板尚未被触碰），全部就绪后才 Clear + 逐条 Put。失败留
    /// debug 痕并返回 `false`（恢复是 best-effort，不决定主结果）。
    fn restore_pasteboard(saved: &SavedContent) -> bool {
        let Some(pb) = open_pasteboard() else {
            debug!(thread = thread::EVENT, "pasteboard unavailable for restore");
            return false;
        };
        // 阶段一：预建全部 flavor 数据；任一失败即放弃，已建引用由 staged
        // 里各守卫随 Vec 一起释放。
        let mut staged: Vec<(PasteboardItemID, CFStringRef, CfGuard)> = Vec::new();
        for item in &saved.0 {
            for flavor in &item.flavors {
                // SAFETY: `data` 字节缓冲在本次调用内存活且非悬空，长度即
                // 缓冲长度；分配失败返回 NULL，随后判别。
                let data = unsafe {
                    CFDataCreate(
                        kCFAllocatorDefault,
                        flavor.data.as_ptr(),
                        flavor.data.len() as CFIndex,
                    )
                };
                if data.is_null() {
                    debug!(
                        thread = thread::EVENT,
                        "flavor data allocation failed, restore declined"
                    );
                    return false;
                }
                staged.push((
                    item.id,
                    flavor.name.string_ref(),
                    CfGuard::new(data as CFTypeRef),
                ));
            }
        }
        // 阶段二：数据全部就绪才清板写入。
        // SAFETY: `raw` 是有效 +1 引用，存活至本结构销毁。
        let status = unsafe { PasteboardClear(pb.raw) };
        if status != 0 {
            debug!(thread = thread::EVENT, status, "pasteboard clear failed");
            return false;
        }
        for (id, name, data) in &staged {
            // SAFETY: `raw`/`id`/`name`/`data` 均为阶段一备齐的有效引用；
            // Put 不接管 `data` 引用，由 staged 的守卫释放。
            let status = unsafe { PasteboardPutItemFlavor(pb.raw, *id, *name, data.data_ref(), 0) };
            if status != 0 {
                debug!(
                    thread = thread::EVENT,
                    status, "flavor write failed, restore incomplete"
                );
                return false;
            }
        }
        true
    }

    /// 恢复原剪贴板内容；失败留痕但不影响调用方的主结果。
    fn restore_and_log(saved: &SavedContent) {
        if !restore_pasteboard(saved) {
            error!(thread = thread::EVENT, "failed to restore clipboard");
        }
    }

    /// 注入复制快捷键：修饰键按下 → C 按下/释放 → 修饰键释放。任何一步
    /// 失败即放弃；若修饰键已按下而后续步骤失败，补发配对释放。
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
                    // 错误路径上的补发释放是尽力而为：释放失败已由紧随的
                    // SelectionUnavailable 收口，无更优处理。
                    #[allow(clippy::let_underscore_must_use)]
                    let _ = rdev::simulate(&EventType::KeyRelease(COPY_MODIFIER));
                }
                return Err(GlossError::SelectionUnavailable);
            }
        }
        Ok(())
    }
}

pub use imp::ClipboardFallbackReader;

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::wait_for_write;

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
#[allow(clippy::let_underscore_must_use)]
mod live_tests {
    use std::ffi::{CStr, c_void};
    use std::sync::Mutex;

    use arboard::Clipboard;

    use crate::ffi::cf::{self, CFDataGetBytePtr, CFDataGetLength, CFDataRef, CFIndex, CfGuard};
    use crate::ffi::pasteboard::{
        CLIPBOARD_NAME, PasteboardClear, PasteboardCopyItemFlavorData, PasteboardCreate,
        PasteboardGetItemIdentifier, PasteboardItemID, PasteboardPutItemFlavor, PasteboardRef,
        PasteboardSynchronize,
    };

    use super::imp::ClipboardFallbackReader;

    static CLIPBOARD_LIVE_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn fallback_read_restores_original_clipboard() {
        let _clipboard = CLIPBOARD_LIVE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut clipboard = Clipboard::new().expect("clipboard should be available");
        let preset = format!("gloss-restore-{}", std::process::id());
        clipboard.set_text(preset.clone()).expect("preset text");

        let mut reader = ClipboardFallbackReader::new();
        let _ = reader.read();

        let restored = clipboard.get_text().expect("clipboard should be readable");
        assert!(
            restored == preset,
            "original clipboard content must survive (got {} bytes)",
            restored.len()
        );
    }

    #[test]
    fn fallback_read_restores_multiflavor_clipboard() {
        let _clipboard = CLIPBOARD_LIVE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let preset_text = "gloss-multiflavor-preset";
        let custom_flavor: &CStr = c"org.gloss.test.multiflavor";
        let payload = b"\x00\x01\xfe\xff preset payload";
        assert!(
            preset_items(&[(preset_text, custom_flavor, payload)]),
            "preset write should succeed"
        );

        let mut reader = ClipboardFallbackReader::new();
        let _ = reader.read();

        let mut clipboard = Clipboard::new().expect("clipboard should be available");
        let text = clipboard
            .get_text()
            .expect("text flavor should be readable");
        assert!(
            text == preset_text,
            "text flavor must survive verbatim (got {} bytes)",
            text.len()
        );
        assert_eq!(
            read_flavor_data(1, custom_flavor).as_deref(),
            Some(&payload[..]),
            "custom flavor must survive byte-for-byte"
        );
    }

    #[test]
    fn fallback_read_restores_empty_clipboard() {
        let _clipboard = CLIPBOARD_LIVE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(clear_pasteboard_for_test(), "preset clear should succeed");

        let mut reader = ClipboardFallbackReader::new();
        let _ = reader.read();

        let mut clipboard = Clipboard::new().expect("clipboard should be available");
        assert!(
            matches!(
                clipboard.get_text(),
                Err(arboard::Error::ContentNotAvailable)
            ),
            "clipboard must still be empty after the fallback round trip"
        );
    }

    #[test]
    fn fallback_read_restores_multi_item_clipboard() {
        let _clipboard = CLIPBOARD_LIVE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let flavor_x: &CStr = c"org.gloss.test.item-x";
        let flavor_y: &CStr = c"org.gloss.test.item-y";
        assert!(
            preset_items(&[
                ("gloss-item-one", flavor_x, b"payload x".as_slice()),
                ("gloss-item-two", flavor_y, b"payload y".as_slice()),
            ]),
            "preset write should succeed"
        );

        let mut reader = ClipboardFallbackReader::new();
        let _ = reader.read();

        assert_eq!(
            read_flavor_data(1, c"public.utf8-plain-text").as_deref(),
            Some(&b"gloss-item-one"[..]),
            "item 1 text flavor must merge back into item 1"
        );
        assert_eq!(
            read_flavor_data(1, flavor_x).as_deref(),
            Some(&b"payload x"[..]),
            "item 1 custom flavor must merge back into item 1"
        );
        assert_eq!(
            read_flavor_data(2, c"public.utf8-plain-text").as_deref(),
            Some(&b"gloss-item-two"[..]),
            "item 2 text flavor must merge back into item 2"
        );
        assert_eq!(
            read_flavor_data(2, flavor_y).as_deref(),
            Some(&b"payload y"[..]),
            "item 2 custom flavor must merge back into item 2"
        );
    }

    fn preset_items(items: &[(&str, &CStr, &[u8])]) -> bool {
        let Some(name_ref) = cf::cf_string(CLIPBOARD_NAME) else {
            return false;
        };
        let mut pb: PasteboardRef = std::ptr::null_mut();
        // SAFETY: `name_ref` 是有效 CFString 引用，出参指向栈上变量。
        let status = unsafe { PasteboardCreate(name_ref.string_ref(), &mut pb) };
        if status != 0 || pb.is_null() {
            return false;
        }
        // SAFETY: `pb` 是 PasteboardCreate 返回的 +1 引用，交给守卫释放。
        let _pb = CfGuard::new(pb as *const c_void);

        // SAFETY: `pb` 是有效 +1 引用。
        let status = unsafe { PasteboardClear(pb) };
        if status != 0 {
            return false;
        }
        for (index, (text, flavor, data)) in items.iter().enumerate() {
            let item_id = (index + 1) as PasteboardItemID;
            if !put_text_and_flavor_for_test(pb, item_id, text, flavor, data) {
                return false;
            }
        }
        true
    }

    fn put_text_and_flavor_for_test(
        pb: PasteboardRef,
        item_id: PasteboardItemID,
        text: &str,
        flavor: &CStr,
        data: &[u8],
    ) -> bool {
        let (Some(text_ref), Some(flavor_ref), Some(text_data), Some(flavor_data)) = (
            cf::cf_string(c"public.utf8-plain-text"),
            cf::cf_string(flavor),
            cf::cf_data(text.as_bytes()),
            cf::cf_data(data),
        ) else {
            return false;
        };
        // SAFETY: `pb` 与四个引用的守卫均存活到函数末尾，Put 不接管引用。
        let status = unsafe {
            PasteboardPutItemFlavor(pb, item_id, text_ref.string_ref(), text_data.data_ref(), 0)
        };
        if status != 0 {
            return false;
        }
        // SAFETY: 同上，引用仍由守卫持有。
        let status = unsafe {
            PasteboardPutItemFlavor(
                pb,
                item_id,
                flavor_ref.string_ref(),
                flavor_data.data_ref(),
                0,
            )
        };
        status == 0
    }

    fn clear_pasteboard_for_test() -> bool {
        let Some(name_ref) = cf::cf_string(CLIPBOARD_NAME) else {
            return false;
        };
        let mut pb: PasteboardRef = std::ptr::null_mut();
        // SAFETY: `name_ref` 是有效 CFString 引用，出参指向栈上变量。
        let status = unsafe { PasteboardCreate(name_ref.string_ref(), &mut pb) };
        if status != 0 || pb.is_null() {
            return false;
        }
        // SAFETY: `pb` 是 PasteboardCreate 返回的 +1 引用，交给守卫释放。
        let _pb = CfGuard::new(pb as *const _);
        // SAFETY: `pb` 是有效 +1 引用。
        let status = unsafe { PasteboardClear(pb) };
        status == 0
    }

    fn read_flavor_data(item_index: CFIndex, flavor: &CStr) -> Option<Vec<u8>> {
        let name_ref = cf::cf_string(CLIPBOARD_NAME)?;
        let mut pb: PasteboardRef = std::ptr::null_mut();
        // SAFETY: `name_ref` 是有效 CFString 引用，出参指向栈上变量。
        let status = unsafe { PasteboardCreate(name_ref.string_ref(), &mut pb) };
        if status != 0 || pb.is_null() {
            return None;
        }
        // SAFETY: `pb` 是 PasteboardCreate 返回的 +1 引用，交给守卫释放。
        let _pb = CfGuard::new(pb as *const _);
        // SAFETY: `pb` 是有效 +1 引用；本地引用可能滞后于全局板，先同步。
        unsafe { PasteboardSynchronize(pb) };
        let mut id: PasteboardItemID = std::ptr::null_mut();
        // SAFETY: `pb` 是有效 +1 引用，出参指向栈上变量。
        let status = unsafe { PasteboardGetItemIdentifier(pb, item_index, &mut id) };
        if status != 0 {
            return None;
        }
        let flavor_ref = cf::cf_string(flavor)?;
        let mut data: CFDataRef = std::ptr::null();
        // SAFETY: `pb`/`id` 与 `flavor_ref` 均为有效引用，出参指向栈上变量。
        let status =
            unsafe { PasteboardCopyItemFlavorData(pb, id, flavor_ref.string_ref(), &mut data) };
        if status != 0 || data.is_null() {
            return None;
        }
        // SAFETY: `data` 是刚拷贝的有效 CFData 引用，长度与指针来自同一对象。
        let (len, bytes_ptr) = unsafe { (CFDataGetLength(data), CFDataGetBytePtr(data)) };
        let Ok(len) = usize::try_from(len) else {
            return None;
        };
        if len == 0 {
            return Some(Vec::new());
        }
        // SAFETY: `bytes_ptr` 非 NULL（长度非零的 CFData 保证），可读字节
        // 数恰为 `len`。
        let bytes = unsafe { std::slice::from_raw_parts(bytes_ptr, len) };
        Some(bytes.to_vec())
    }

    #[test]
    #[ignore = "需授权真机：先把运行测试的终端 App 加入 系统设置→隐私与                安全性→辅助功能（未授权时由 live_test_support 快速失败）"]
    fn reads_live_selection_via_simulated_copy() {
        let _clipboard = CLIPBOARD_LIVE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        crate::live_test_support::require_accessibility("clipboard_simulated_copy");
        let mut reader = ClipboardFallbackReader::new();
        let text = reader
            .read()
            .expect("fallback should read the live selection");
        println!("selected: {text}");
        assert!(!text.is_empty());
    }

    #[test]
    #[ignore = "需授权真机：先把运行测试的终端 App 加入 系统设置→隐私与                安全性→辅助功能，并在前台应用里选中文字"]
    fn reads_live_selection_from_rich_clipboard_via_simulated_copy() {
        let _clipboard = CLIPBOARD_LIVE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        crate::live_test_support::require_accessibility("clipboard_rich_clipboard_fallback");
        let preset_text = "gloss-rich-fallback-preset";
        let custom_flavor: &CStr = c"org.gloss.test.richfallback";
        let payload = b"rich fallback payload".as_slice();
        assert!(
            preset_items(&[(preset_text, custom_flavor, payload)]),
            "preset write should succeed"
        );

        let mut reader = ClipboardFallbackReader::new();
        let text = reader
            .read()
            .expect("rich clipboard must not decline the fallback");
        assert!(!text.is_empty(), "fallback should read the live selection");

        let mut clipboard = Clipboard::new().expect("clipboard should be available");
        let restored = clipboard
            .get_text()
            .expect("text flavor should be readable");
        assert!(
            restored == preset_text,
            "text flavor must be restored verbatim (got {} bytes)",
            restored.len()
        );
        assert_eq!(
            read_flavor_data(1, custom_flavor).as_deref(),
            Some(payload),
            "custom flavor must be restored byte-for-byte"
        );
    }
}
