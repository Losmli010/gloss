//! 快照写回（恢复序列）：预建全部 flavor 数据 → 清板 → 逐条写回，单次
//! 序列失败短退避后重试（每次开全新粘贴板引用）。

use std::time::Duration;

use crate::ffi::cf::{CFDataCreate, CFIndex, CFStringRef, CFTypeRef, CfGuard, kCFAllocatorDefault};
use crate::ffi::pasteboard::{PasteboardClear, PasteboardItemID, PasteboardPutItemFlavor};

use gloss_core::log::{debug, error, thread};

use super::snapshot::{SavedContent, open_pasteboard_reported};

/// 恢复写盘的尝试次数与退避：CI 实测高负载虚拟机上粘贴板服务会出现
/// ≥100ms 级的持续不可用（3×50ms 重试未成功清板一次，板上永久留下注入
/// 副本、用户原内容丢失），短窗口熬不过去；最坏退避 5×200ms ≈ 1s，低于
/// 事件线程既有的最坏占用（写入确认两轮 1.6s），且只在已失败的路径上
/// 付出。
const RESTORE_ATTEMPTS: usize = 6;
const RESTORE_RETRY_DELAY: Duration = Duration::from_millis(200);

/// 恢复序列的失败环节：随 error! 进日志（生产可定位），测试二进制无
/// subscriber，live 测试经 [`gloss_core::log::capture`] 取同一条进断言
/// 消息。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RestoreStage {
    /// 打不开粘贴板（名称构造失败，或 PasteboardCreate 返回非零）。
    Open,
    /// 预建 flavor 的 CFDataCreate 返回空。
    Alloc,
    /// PasteboardClear 返回非零。
    Clear,
    /// PasteboardPutItemFlavor 返回非零。
    Put,
}

/// 恢复写盘的结果：`ok` 表示快照已完整写回（重试耗尽也不影响调用方
/// 主结果）；`clears` 是实际发生的清板次数——Clear 是恢复序列里唯一
/// 推进 changeCount 的调用（Put 实测不推进），哨兵的写盘归因必须按它
/// 计数，半程失败后的重试成功会清两次板，固定记 1 会把差额误判成用户
/// 的外来复制；`stage`/`status` 仅在 `ok` 为假时有意义（系统调用无
/// 状态码的场合记 0，名称构造失败记 -1）。
pub(super) struct RestoreOutcome {
    pub(super) ok: bool,
    pub(super) clears: u64,
    pub(super) stage: RestoreStage,
    pub(super) status: i32,
}

/// 把快照原样写回：见 [`restore_with_retry`] 与 [`restore_pasteboard_once`]。
pub(super) fn restore_pasteboard(saved: &SavedContent) -> RestoreOutcome {
    restore_with_retry(RESTORE_ATTEMPTS, RESTORE_RETRY_DELAY, || {
        restore_pasteboard_once(saved)
    })
}

/// 恢复原剪贴板内容；失败留痕（环节与状态码，供定位粘贴板服务的
/// 持续不可用）但不影响调用方的主结果。
pub(super) fn restore_and_log(saved: &SavedContent) {
    let outcome = restore_pasteboard(saved);
    if !outcome.ok {
        error!(
            thread = thread::EVENT,
            stage = ?outcome.stage,
            status = outcome.status,
            clears = outcome.clears,
            "failed to restore clipboard"
        );
    }
}

/// 带重试的恢复：单次序列失败短退避后重试（每次开全新粘贴板引用，
/// 上一次 Clear 成功而 Put 失败的半程态由本次 Clear 重置），成功即
/// 止；`clears` 累加全部尝试的实际清板数，含失败尝试。退避经参数
/// 注入以便单测零延时驱动。
pub(super) fn restore_with_retry(
    attempts: usize,
    delay: Duration,
    mut once: impl FnMut() -> RestoreOutcome,
) -> RestoreOutcome {
    let mut clears = 0;
    let (mut stage, mut status) = (RestoreStage::Open, 0);
    for attempt in 0..attempts {
        let outcome = once();
        clears += outcome.clears;
        if outcome.ok {
            return RestoreOutcome {
                ok: true,
                clears,
                stage: RestoreStage::Open,
                status: 0,
            };
        }
        stage = outcome.stage;
        status = outcome.status;
        if attempt + 1 < attempts {
            std::thread::sleep(delay);
        }
    }
    RestoreOutcome {
        ok: false,
        clears,
        stage,
        status,
    }
}

/// 单次恢复序列；失败的具体环节由各分支留 debug 痕。
fn restore_pasteboard_once(saved: &SavedContent) -> RestoreOutcome {
    let fail = |stage: RestoreStage, status: i32, clears: u64| {
        debug!(
            thread = thread::EVENT,
            ?stage,
            status,
            "restore attempt failed"
        );
        RestoreOutcome {
            ok: false,
            clears,
            stage,
            status,
        }
    };
    let pb = match open_pasteboard_reported() {
        Ok(pb) => pb,
        Err(status) => return fail(RestoreStage::Open, status, 0),
    };
    // 阶段一：预建全部 flavor 数据；任一失败即放弃，已建引用由 staged
    // 里各守卫随 Vec 一起释放。
    let Some(staged) = stage_flavor_data(saved) else {
        return fail(RestoreStage::Alloc, 0, 0);
    };
    // 阶段二：数据全部就绪才清板写入。
    // SAFETY: `raw` 是有效 +1 引用，存活至本结构销毁。
    let status = unsafe { PasteboardClear(pb.raw) };
    if status != 0 {
        return fail(RestoreStage::Clear, status, 0);
    }
    for (id, name, data) in &staged {
        // SAFETY: `raw`/`id`/`name`/`data` 均为阶段一备齐的有效引用；
        // Put 不接管 `data` 引用，由 staged 的守卫释放。
        let status = unsafe { PasteboardPutItemFlavor(pb.raw, *id, *name, data.data_ref(), 0) };
        if status != 0 {
            return fail(RestoreStage::Put, status, 1);
        }
    }
    RestoreOutcome {
        ok: true,
        clears: 1,
        stage: RestoreStage::Open,
        status: 0,
    }
}

/// 阶段一：预建全部 flavor 的 CFData（引用由守卫随 Vec 持有）；任一
/// 分配失败返回 `None`。
fn stage_flavor_data(
    saved: &SavedContent,
) -> Option<Vec<(PasteboardItemID, CFStringRef, CfGuard)>> {
    let mut staged = Vec::new();
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
                return None;
            }
            staged.push((
                item.id,
                flavor.name.string_ref(),
                CfGuard::new(data as CFTypeRef),
            ));
        }
    }
    Some(staged)
}
