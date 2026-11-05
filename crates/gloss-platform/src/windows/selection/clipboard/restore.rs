//! 快照写回（恢复序列）：文本写回或清板，单次失败短退避后重试。
//!
//! 系统（Windows）的恢复走跨平台剪贴板库：文本快照原样写回，空板快照
//! 清板；剪贴板被其它应用短时占有时单次写回会失败，按系统（macOS）恢复
//! 序列的同款节奏重试（每次取全新剪贴板句柄）。

use std::time::Duration;

use arboard::Clipboard;

use gloss_core::log::{debug, error, thread};

use super::snapshot::SavedContent;

const RESTORE_ATTEMPTS: usize = 6;

const RESTORE_RETRY_DELAY: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RestoreStage {
    Clipboard,
    Set,
    Clear,
}

pub(super) struct RestoreOutcome {
    pub(super) ok: bool,
    pub(super) writes: u64,
    pub(super) stage: RestoreStage,
}

pub(super) fn restore_board(saved: &SavedContent) -> RestoreOutcome {
    restore_with_retry(RESTORE_ATTEMPTS, RESTORE_RETRY_DELAY, || {
        restore_once(saved)
    })
}

pub(super) fn restore_and_log(saved: &SavedContent) {
    let outcome = restore_board(saved);
    if !outcome.ok {
        error!(
            thread = thread::EVENT,
            stage = ?outcome.stage,
            writes = outcome.writes,
            "failed to restore clipboard"
        );
    }
}

pub(super) fn restore_with_retry(
    attempts: usize,
    delay: Duration,
    mut once: impl FnMut() -> RestoreOutcome,
) -> RestoreOutcome {
    let mut writes = 0;
    let mut stage = RestoreStage::Clipboard;
    for attempt in 0..attempts {
        let outcome = once();
        writes += outcome.writes;
        if outcome.ok {
            return RestoreOutcome {
                ok: true,
                writes,
                stage: RestoreStage::Clipboard,
            };
        }
        stage = outcome.stage;
        if attempt + 1 < attempts {
            std::thread::sleep(delay);
        }
    }
    RestoreOutcome {
        ok: false,
        writes,
        stage,
    }
}

fn restore_once(saved: &SavedContent) -> RestoreOutcome {
    let fail = |stage: RestoreStage, writes: u64| {
        debug!(thread = thread::EVENT, ?stage, "restore attempt failed");
        RestoreOutcome {
            ok: false,
            writes,
            stage,
        }
    };
    let Ok(mut clipboard) = Clipboard::new() else {
        return fail(RestoreStage::Clipboard, 0);
    };
    match saved {
        SavedContent::Text(text) => match clipboard.set_text(text.clone()) {
            Ok(()) => RestoreOutcome {
                ok: true,
                writes: 1,
                stage: RestoreStage::Clipboard,
            },
            Err(_) => fail(RestoreStage::Set, 0),
        },
        SavedContent::Empty => match clipboard.clear() {
            Ok(()) => RestoreOutcome {
                ok: true,
                writes: 1,
                stage: RestoreStage::Clipboard,
            },
            Err(_) => fail(RestoreStage::Clear, 0),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{RestoreOutcome, RestoreStage, restore_with_retry};

    fn outcome(ok: bool, writes: u64) -> RestoreOutcome {
        RestoreOutcome {
            ok,
            writes,
            stage: RestoreStage::Clipboard,
        }
    }

    #[test]
    fn board_retry_stops_at_the_first_success() {
        let calls = std::cell::Cell::new(0u32);
        let result = restore_with_retry(3, Duration::ZERO, || {
            calls.set(calls.get() + 1);
            outcome(true, 1)
        });
        assert!(result.ok);
        assert_eq!(result.writes, 1);
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn board_retry_accumulates_the_writes_of_every_attempt_until_success() {
        let calls = std::cell::Cell::new(0u32);
        let result = restore_with_retry(3, Duration::ZERO, || {
            calls.set(calls.get() + 1);
            match calls.get() {
                1 => outcome(false, 0),
                2 => outcome(false, 0),
                _ => outcome(true, 1),
            }
        });
        assert!(result.ok);
        assert_eq!(result.writes, 1);
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn board_retry_exhaustion_carries_the_last_stage_and_total_writes() {
        let calls = std::cell::Cell::new(0u32);
        let result = restore_with_retry(2, Duration::ZERO, || {
            calls.set(calls.get() + 1);
            match calls.get() {
                1 => RestoreOutcome {
                    ok: false,
                    writes: 0,
                    stage: RestoreStage::Set,
                },
                _ => RestoreOutcome {
                    ok: false,
                    writes: 0,
                    stage: RestoreStage::Clear,
                },
            }
        });
        assert!(!result.ok);
        assert_eq!(result.writes, 0);
        assert_eq!(result.stage, RestoreStage::Clear);
    }
}
