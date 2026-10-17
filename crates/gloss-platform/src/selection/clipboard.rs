//! 剪贴板兜底读取（模拟复制）：`ClipboardFallbackReader`。
//!
//! AX 读不到选区时的兜底通道：快照剪贴板全部条目与 flavor 原始字节 →
//! 注入 Cmd+C → 轮询确认目标应用写入 → 读回 → 读回成功且现值未变才按
//! 快照恢复（失败轮一律不恢复——板上要么未变，要么是本轮之外的新写入
//! 或注入副产物，都不是可覆盖的内容）。快照按条目与 flavor 尽力保全：
//! 读不出数据的 flavor（promised 数据等）单独跳过——读不出即无法如实
//! 恢复，整轮放弃只会让兜底在有此类 flavor 的板上整批失效。两类边界仍
//! 整体放弃、不注入：结构读不出（板、条目、flavor 名单）；板上条目的
//! flavor 全部读不出——注入后原内容无法如实写回，恢复轮会清空剪贴板。
//! 未确认时重注入一轮再等一个完整超时窗口。产物与收尾分离：恢复决策与
//! 写盘归因装在 [`FallbackSettle`] 里，调用方在取材产物上抛之后收口
//! （[`FallbackSettle::finish`]）——恢复写盘不挡在弹窗链路上。写盘轮次
//! 经取材轮次闩锁向哨兵自报计数归因（见 clipboard 模块的
//! [`crate::clipboard::RoundWrites`]）。纯时序逻辑在 [`wait_for_write`]
//! （单测覆盖），系统 API 交互归 `imp` 模块。

use std::time::{Duration, Instant};

/// 写入确认的轮询上限：超过即认为目标应用未响应复制。兜底是取材的末段
/// 通道，窗口约束的只是无响应宿主上的最坏路径（两轮最坏 1.6s）；常规
/// 确认在数十毫秒内完成。
const WRITE_CONFIRM_TIMEOUT: Duration = Duration::from_millis(800);
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

    use crate::clipboard::{RoundWrites, WatchRoundFlag, pasteboard_change_count};
    use crate::ffi::cf::{
        self, CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef, CFDataCreate, CFDataGetBytePtr,
        CFDataGetLength, CFDataRef, CFIndex, CFRelease, CFRetain, CFStringRef, CFTypeRef, CfGuard,
        kCFAllocatorDefault,
    };
    use crate::ffi::event;
    use crate::ffi::pasteboard::{
        CLIPBOARD_NAME, K_PASTEBOARD_MODIFIED, PasteboardClear, PasteboardCopyItemFlavorData,
        PasteboardCopyItemFlavors, PasteboardCreate, PasteboardGetItemCount,
        PasteboardGetItemIdentifier, PasteboardItemID, PasteboardPutItemFlavor, PasteboardRef,
        PasteboardSynchronize,
    };

    use gloss_core::log::{debug, error, thread};
    use gloss_core::model::GlossError;

    use super::{WRITE_CONFIRM_TIMEOUT, WRITE_POLL_INTERVAL, wait_for_write};

    /// 剪贴板兜底轮的收尾：恢复决策在读取时定下，[`FallbackSettle::finish`]
    /// 在取材产物上抛之后执行恢复（或跳过）并置位写盘归因——恢复写盘不
    /// 挡在产物交付前。finish 前的 panic 展开由内部 [`PendingRestore`] 的
    /// Drop 兜底按快照恢复（受控路径都已显式收口，Drop 只为展开兜底）。
    pub struct FallbackSettle {
        /// 恢复职责：`None` = 本轮未注入（板未被触碰），收口无事可做。
        pending: Option<PendingRestore>,
        /// 取材轮次闩锁句柄：收口时置位写盘归因。
        round: WatchRoundFlag,
        /// 写盘前的 changeCount 读数（归因基线）。
        pre: u64,
        /// 已确认的注入复制计数（0/1，Ok 蕴含写入确认）；恢复与否在收口时补计。
        acquired: u64,
        /// 收口决策：成功轮且现值仍是读回文本才恢复。
        restore: bool,
    }

    impl FallbackSettle {
        /// 未注入轮（读板失败）：板未被触碰，无恢复亦无归因。
        fn idle(round: WatchRoundFlag) -> Self {
            Self {
                pending: None,
                round,
                pre: 0,
                acquired: 0,
                restore: false,
            }
        }

        /// 定下收口决策：`restore` 为真时收口按快照恢复原剪贴板。
        fn planned(
            pending: PendingRestore,
            round: WatchRoundFlag,
            pre: u64,
            acquired: bool,
            restore: bool,
        ) -> Self {
            Self {
                pending: Some(pending),
                round,
                pre,
                acquired: u64::from(acquired),
                restore,
            }
        }

        /// 收口：按读取时定下的决策恢复（或跳过）原剪贴板并置位写盘归因。
        /// 在取材产物上抛之后、与读取同一事件线程上调用。
        pub fn finish(mut self) {
            let Some(pending) = self.pending.as_mut() else {
                return;
            };
            // 自报写盘数只计可归因的写入：文本读回成功才证明注入复制确实
            // 发生（Ok 蕴含写入确认）；「确认了写入却读不出文本」的写入
            // 无法与轮次内外来复制区分，一律记外来、由哨兵的 has_image
            // 兜底判定，宁可多触发不可误吞用户复制。
            let restored = if self.restore {
                pending.restore_now()
            } else {
                pending.skip_restore();
                false
            };
            self.round.enter(RoundWrites {
                pre: self.pre,
                ours: self.acquired + u64::from(restored),
            });
        }
    }

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

    /// 待执行的恢复：受控路径经 [`PendingRestore::restore_now`] /
    /// [`PendingRestore::skip_restore`] 显式收口，panic 展开由 Drop 兜底
    /// 按快照恢复原剪贴板。
    struct PendingRestore {
        saved: SavedContent,
        skip: bool,
    }

    impl PendingRestore {
        /// 按快照恢复原剪贴板并取消 Drop 兜底；返回是否写回成功（失败
        /// 已留 error 痕，不影响调用方的主结果）。
        fn restore_now(&mut self) -> bool {
            self.skip = true;
            let ok = restore_pasteboard(&self.saved);
            if !ok {
                error!(thread = thread::EVENT, "failed to restore clipboard");
            }
            ok
        }

        /// 跳过恢复（Drop 兜底一并取消）。
        fn skip_restore(&mut self) {
            self.skip = true;
        }
    }

    impl Drop for PendingRestore {
        fn drop(&mut self) {
            // 只有 panic 展开会走到这里：受控路径都已显式收口。
            if !self.skip {
                restore_and_log(&self.saved);
            }
        }
    }

    /// 剪贴板兜底读取实现：持取材轮次闩锁句柄，写盘轮次置位计数归因
    /// （哨兵消费时据此区分自身写盘与用户的真实复制）。
    #[derive(Debug, Default, Clone)]
    pub struct ClipboardFallbackReader {
        round: WatchRoundFlag,
    }

    impl ClipboardFallbackReader {
        /// 创建兜底读取器：传入与哨兵源共享的取材轮次闩锁句柄。
        pub fn new(round: WatchRoundFlag) -> Self {
            Self { round }
        }

        /// 兜底读取前台应用的选中文本（模拟复制路径）。
        ///
        /// 产物与收尾分离：返回值第二项（[`FallbackSettle`]）持有恢复决策与
        /// 写盘归因，调用方在产物上抛之后 `finish` 收口——恢复写盘不挡在产物
        /// 交付前。调用方保证：在平台事件线程上调用（收尾同线程）。
        /// 阻塞语义：确认目标应用写入最长轮询两轮 [`WRITE_CONFIRM_TIMEOUT`]
        /// （首发未确认时重注入一轮），期间事件线程被占用、其余命令与事件
        /// 排队（事件源侧缓冲），调用方需自行处理在途重复触发。
        pub fn read(&mut self) -> (Result<String, GlossError>, FallbackSettle) {
            let mut clipboard = match Clipboard::new() {
                Ok(clipboard) => clipboard,
                Err(err) => {
                    debug!(
                        thread = thread::EVENT,
                        error = %err,
                        "clipboard unavailable, fallback read skipped"
                    );
                    return (
                        Err(GlossError::SelectionUnavailable),
                        FallbackSettle::idle(self.round.clone()),
                    );
                }
            };
            // 归因起点先于快照记：快照要拷贝全部 flavor 字节（大图可达数
            // 十毫秒），其间落进的外来写入不得算进本轮自身写盘的基线。
            let pre_count = pasteboard_change_count().unwrap_or(0);
            // 快照失败直接上抛：尚未注入，剪贴板未被触碰（无写盘即无需
            // 置位闩锁）。
            let Some(saved) = snapshot_pasteboard() else {
                debug!(
                    thread = thread::EVENT,
                    "original clipboard content cannot be fully preserved, fallback declined"
                );
                return (
                    Err(GlossError::SelectionUnavailable),
                    FallbackSettle::idle(self.round.clone()),
                );
            };
            // 恢复收口：受控路径经 FallbackSettle::finish 恢复或跳过，panic
            // 展开由 PendingRestore 的 Drop 兜底。
            let pending = PendingRestore { saved, skip: false };
            let outcome = self.attempt(&mut clipboard);
            let settle = match &outcome {
                Ok(acquired) => {
                    // 窗口期内剪贴板可能已被再次改动：现值仍是我们刚读到的
                    // 文本才恢复。
                    let current_is_ours =
                        matches!(clipboard.get_text(), Ok(current) if &current == acquired);
                    if current_is_ours {
                        FallbackSettle::planned(pending, self.round.clone(), pre_count, true, true)
                    } else {
                        debug!(
                            thread = thread::EVENT,
                            "clipboard changed after read, restore skipped"
                        );
                        FallbackSettle::planned(pending, self.round.clone(), pre_count, true, false)
                    }
                }
                // 失败轮一律不恢复，板上内容三态皆不覆盖：未变（无需恢复）、
                // 本轮之外的新写入（目标应用迟到的复制、用户的复制）、注入
                // 副产物（确认了写入却读不出文本——前台选中的可能是图）。
                // 第三态留在板上并按外来写入放行：它正是前台应用对注入
                // ⌘C 的响应，是图即触发图片解释，好过静默清掉。
                Err(_) => {
                    FallbackSettle::planned(pending, self.round.clone(), pre_count, false, false)
                }
            };
            (outcome, settle)
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

    /// 快照原剪贴板全部条目与 flavor 数据。两类失败整体返回 `None`（调用
    /// 方据此放弃注入，剪贴板保持原样）：结构读不出（板、条目数、条目
    /// 标识、flavor 名单）；板上条目的 flavor 全部读不出。单个 flavor
    /// 读不出数据（promised 等）只跳过该 flavor，读得出的部分照常保全
    /// （见模块注释的边界说明）。
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
                    // 单个 flavor 读不出（promised 数据等）跳过而非整轮
                    // 放弃：读不出意味着我们同样无法如实恢复它，整轮放弃
                    // 只会让兜底通道在含 promises 的板上整批失效；可读
                    // flavor 照常保全，读不出的在恢复时缺席。
                    debug!(
                        thread = thread::EVENT,
                        status,
                        "flavor data unavailable (promised?), flavor left out of the snapshot"
                    );
                    continue;
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
            // 全部 flavor 都读不出的条目没有可恢复的内容，跳过——恢复时
            // 写一个空条目毫无意义。
            if flavors.is_empty() {
                debug!(
                    thread = thread::EVENT,
                    "no restorable flavor on the pasteboard item, item skipped"
                );
                continue;
            }
            items.push(SavedItem { id, flavors });
        }
        // 板上确有条目却无一 flavor 可恢复（全部读不出）：注入后原内容
        // 无法如实写回，恢复轮会把剪贴板清空——按整体放弃收口，剪贴板
        // 保持原样；空 items 在这里与「原剪贴板为空」的合法空快照严格
        // 区分（后者 count == 0）。
        if count > 0 && items.is_empty() {
            debug!(
                thread = thread::EVENT,
                "no flavor on the board is restorable, fallback declined"
            );
            return None;
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

    /// 失败点之前已按下、需要补发释放的键（逆序）。`index` 是失败事件的
    /// 序号：只有 `sequence[..index]` 里 `key_down` 的事件才是按下态。
    /// 已成功释放的键可能再收一次 key-up（重复释放幂等，无副作用）。
    pub(super) fn keys_to_release(sequence: &[(u16, bool, u64)], index: usize) -> Vec<u16> {
        sequence[..index]
            .iter()
            .rev()
            .filter(|(_, key_down, _)| *key_down)
            .map(|(key, _, _)| *key)
            .collect()
    }

    /// 注入复制快捷键：修饰键按下 → C 按下/释放 → 修饰键释放。C 的按下
    /// 与释放自带 Command 修饰位（应用按事件自带的修饰位识别快捷键，裸 C
    /// 会被当成普通字符打进焦点处）；修饰键按下同样带位，释放不带——
    /// 修饰键事件的修饰位表达事件之后的状态，带位释放会向下游宣告
    /// 「Command 仍按着」。任何一步失败即放弃；失败点之前按下的键全部
    /// 逆序补发释放（只放修饰键会在 C 键释放失败时留下按下态）。
    fn inject_copy_key() -> Result<(), GlossError> {
        let Some(source) = event::new_hid_source() else {
            debug!(
                thread = thread::EVENT,
                "copy key injection failed: no event source"
            );
            return Err(GlossError::SelectionUnavailable);
        };
        let cmd = event::K_CG_EVENT_FLAG_MASK_COMMAND;
        let sequence = [
            (event::VK_COMMAND, true, cmd),
            (event::VK_ANSI_C, true, cmd),
            (event::VK_ANSI_C, false, cmd),
            (event::VK_COMMAND, false, 0),
        ];
        for (index, &(virtual_key, key_down, flags)) in sequence.iter().enumerate() {
            if !event::post_keyboard_event(&source, virtual_key, key_down, flags) {
                debug!(
                    thread = thread::EVENT,
                    virtual_key, key_down, "copy key injection failed"
                );
                // 错误路径上的补发是尽力而为，释放失败已由紧随的
                // SelectionUnavailable 收口。
                for pressed in keys_to_release(&sequence, index) {
                    let _ = event::post_keyboard_event(&source, pressed, false, 0);
                }
                return Err(GlossError::SelectionUnavailable);
            }
        }
        Ok(())
    }
}

pub use imp::{ClipboardFallbackReader, FallbackSettle};

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::imp::keys_to_release;
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

    #[test]
    fn injection_failure_releases_every_pressed_key_in_reverse() {
        // 序列与 inject_copy_key 同形：修饰键按下 → C 按下 → C 释放 →
        // 修饰键释放；四个失败点各验一次。
        let sequence = [
            (1u16, true, 0u64),
            (2, true, 0),
            (2, false, 0),
            (1, false, 0),
        ];
        assert_eq!(
            keys_to_release(&sequence, 0),
            Vec::<u16>::new(),
            "第一个事件失败时还没有键按下"
        );
        assert_eq!(
            keys_to_release(&sequence, 1),
            vec![1],
            "C 按下失败：只需补放修饰键"
        );
        assert_eq!(
            keys_to_release(&sequence, 2),
            vec![2, 1],
            "C 释放失败：先补 C 再补修饰键（只放修饰键会留下 C 按下态）"
        );
        assert_eq!(
            keys_to_release(&sequence, 3),
            vec![2, 1],
            "修饰键释放失败：C 已释放，重复 key-up 幂等"
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
    use crate::clipboard::{WatchRoundFlag, pasteboard_change_count};

    static CLIPBOARD_LIVE_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn fallback_read_restores_original_clipboard() {
        let _clipboard = CLIPBOARD_LIVE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut clipboard = Clipboard::new().expect("clipboard should be available");
        let preset = format!("gloss-restore-{}", std::process::id());
        clipboard.set_text(preset.clone()).expect("preset text");

        let round = WatchRoundFlag::default();
        let mut reader = ClipboardFallbackReader::new(round.clone());
        let (_, settle) = reader.read();
        settle.finish();

        let restored = clipboard.get_text().expect("clipboard should be readable");
        assert!(
            restored == preset,
            "original clipboard content must survive (got {} bytes)",
            restored.len()
        );
        let writes = round.peek().expect("the round must report its writes");
        assert!(
            matches!(writes.ours, 0 | 2),
            "ours must be 0 (failed round) or 2 (acquired + restored), got {}",
            writes.ours
        );
        assert!(
            pasteboard_change_count().is_none_or(|current| current >= writes.pre),
            "the round pre count must not exceed the current count"
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

        let mut reader = ClipboardFallbackReader::new(WatchRoundFlag::default());
        let (_, settle) = reader.read();
        settle.finish();

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

        let mut reader = ClipboardFallbackReader::new(WatchRoundFlag::default());
        let (_, settle) = reader.read();
        settle.finish();

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

        let mut reader = ClipboardFallbackReader::new(WatchRoundFlag::default());
        let (_, settle) = reader.read();
        settle.finish();

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
        let mut reader = ClipboardFallbackReader::new(WatchRoundFlag::default());
        let (outcome, settle) = reader.read();
        settle.finish();
        let text = outcome.expect("fallback should read the live selection");
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

        let mut reader = ClipboardFallbackReader::new(WatchRoundFlag::default());
        let (outcome, settle) = reader.read();
        settle.finish();
        let text = outcome.expect("rich clipboard must not decline the fallback");
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
