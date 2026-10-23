//! 原剪贴板的快照：打开系统粘贴板、逐条目保全全部 flavor 的原始字节。
//!
//! 快照的放弃边界（哪些失败整体放弃、哪些只跳过该 flavor）以
//! [`snapshot_pasteboard`] 的文档为准。

use crate::ffi::cf::{
    self, CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef, CFDataGetBytePtr, CFDataGetLength,
    CFDataRef, CFIndex, CFRelease, CFRetain, CFStringRef, CFTypeRef, CfGuard,
};
use crate::ffi::pasteboard::{
    CLIPBOARD_NAME, PasteboardCopyItemFlavorData, PasteboardCopyItemFlavors, PasteboardCreate,
    PasteboardGetItemCount, PasteboardGetItemIdentifier, PasteboardItemID, PasteboardRef,
    PasteboardSynchronize,
};

use gloss_core::log::{debug, error, thread};

use super::restore::{restore_and_log, restore_pasteboard};

/// 打开的系统粘贴板：引用与名称字符串一并用守卫释放。
pub(super) struct OpenPasteboard {
    pub(super) raw: PasteboardRef,
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
pub(super) fn open_pasteboard() -> Option<OpenPasteboard> {
    open_pasteboard_reported().ok()
}

/// 同 [`open_pasteboard`]，但带回失败状态码（名称构造失败记 -1）：
/// 恢复路径的诊断消息需要真实错误码定位粘贴板服务故障。
pub(super) fn open_pasteboard_reported() -> Result<OpenPasteboard, i32> {
    let Some(name) = cf::cf_string(CLIPBOARD_NAME) else {
        return Err(-1);
    };
    let mut raw: PasteboardRef = std::ptr::null_mut();
    // SAFETY: `name` 是有效 CFString 引用，出参指向栈上变量；失败时不
    // 写入出参。
    let status = unsafe { PasteboardCreate(name.string_ref(), &mut raw) };
    if status != 0 || raw.is_null() {
        return Err(status);
    }
    Ok(OpenPasteboard { raw, _name: name })
}

/// 快照的单个 flavor：flavor 名（Retain 自持，守卫释放）与原始字节。
pub(super) struct SavedFlavor {
    pub(super) name: CfGuard,
    pub(super) data: Vec<u8>,
}

/// 快照的单个条目：标识符原样保留，恢复时按它把 flavor 归并回同一条目。
pub(super) struct SavedItem {
    pub(super) id: PasteboardItemID,
    pub(super) flavors: Vec<SavedFlavor>,
}

/// 原剪贴板的完整快照：空 Vec 即原剪贴板为空（恢复 = 清空后不写入）。
pub(super) struct SavedContent(pub(super) Vec<SavedItem>);

/// 待执行的恢复：受控路径经 [`PendingRestore::restore_now`] /
/// [`PendingRestore::skip_restore`] 显式收口，panic 展开由 Drop 兜底
/// 按快照恢复原剪贴板。
pub(super) struct PendingRestore {
    saved: SavedContent,
    skip: bool,
}

impl PendingRestore {
    /// 待执行的恢复：`skip` 只由收口路径显式置位，构造即武装 Drop 兜底。
    pub(super) fn new(saved: SavedContent) -> Self {
        Self { saved, skip: false }
    }

    /// 按快照恢复原剪贴板并取消 Drop 兜底；返回实际清板数（供调用方
    /// 计入写盘归因），恢复失败已留 error 痕（环节与状态码），不影响
    /// 调用方的主结果。
    pub(super) fn restore_now(&mut self) -> u64 {
        self.skip = true;
        let outcome = restore_pasteboard(&self.saved);
        if !outcome.ok {
            error!(
                thread = thread::EVENT,
                stage = ?outcome.stage,
                status = outcome.status,
                clears = outcome.clears,
                "failed to restore clipboard"
            );
        }
        outcome.clears
    }

    /// 跳过恢复（Drop 兜底一并取消）。
    pub(super) fn skip_restore(&mut self) {
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

/// 快照原剪贴板全部条目与 flavor 数据。两类失败整体返回 `None`（调用
/// 方据此放弃注入，剪贴板保持原样）：结构读不出（板、条目数、条目
/// 标识、flavor 名单）；板上条目的 flavor 全部读不出。单个 flavor
/// 读不出数据（promised 等）只跳过该 flavor，读得出的部分照常保全
/// （见模块注释的边界说明）。
pub(super) fn snapshot_pasteboard() -> Option<SavedContent> {
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
    let items = snapshot_items(&pb, count)?;
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

/// 逐条目快照：条目标识与 flavor 名单任一结构读数失败即整体放弃
/// （返回 `None`）；全部 flavor 都读不出的条目跳过。
fn snapshot_items(pb: &OpenPasteboard, count: CFIndex) -> Option<Vec<SavedItem>> {
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
        let flavors = snapshot_flavors(pb, id, flavors_ref, flavor_count)?;
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
    Some(items)
}

/// 逐 flavor 快照：单个 flavor 读不出数据只跳过该 flavor，结构失败
/// 整体 `None`（边界见 [`snapshot_pasteboard`] 文档）。
fn snapshot_flavors(
    pb: &OpenPasteboard,
    id: PasteboardItemID,
    flavors_ref: CFArrayRef,
    flavor_count: usize,
) -> Option<Vec<SavedFlavor>> {
    let mut flavors = Vec::with_capacity(flavor_count);
    for flavor_index in 0..flavor_count {
        // SAFETY: 索引在 GetCount 范围内；返回的元素引用由数组持有，
        // 存入快照前先 Retain 自持。
        let flavor =
            unsafe { CFArrayGetValueAtIndex(flavors_ref, flavor_index as CFIndex) } as CFStringRef;
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
                status, "flavor data unavailable (promised?), flavor left out of the snapshot"
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
    Some(flavors)
}
