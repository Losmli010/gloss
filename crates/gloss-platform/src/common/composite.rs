//! 组合取材通道的平台中立共享形状：主路加剪贴板兜底的组合判定、让渡调度、
//! 激活闸门、读等待预算、产物归一化与“产物先行、收尾随后”的读取装配只写
//! 一份，两侧平台目录共用。本模块零目标条件编译、零外部函数接口（FFI）、
//! 零不安全块，不触任何系统接口——平台差异只经注入闭包进入：主路读取、
//! 剪贴板兜底与宿主激活闭包由绑定侧提供，激活闸门可缺省。
//!
//! 降级边界：主路报空选区分两路。带激活闸门（系统（macOS））时先按几何节
//! 奏让渡补读（见让渡调度）——释放瞬间读到的空多半是还没写好；预算耗尽
//! 仍空与“读不到”一样先过激活闸门：对无障碍树默认不建的宿主家族
//! （Chromium）注入开关按需开树，树一回答文本直接采纳，免注入免剪贴板副
//! 作用；激活落空才落剪贴板兜底。兜底的确认写入能分辨“确实没选”与“读
//! 了但拿不到”——注入复制快捷键后剪贴板没有真实写入就不采纳，误滑不会
//! 把陈旧剪贴板内容当成选区。无激活闸门（系统（Windows）没有需要唤醒的
//! 无障碍树）时让渡耗尽与“读不到”直接落兜底——文本模式缺席时等再久也
//! 读不到，注入才是解法。带闸门时权限被拒（辅助功能授权缺失）不兜底：
//! 兜底依赖按键注入，未授权时注入会被系统静默忽略，白等超时只会拖慢失败
//! 路径；权限语义原样上抛，由上层做权限引导。
//!
//! 时延边界：让渡与激活补读的睡眠段共享一份墙钟预算（读等待预算，自进入
//! 读取起算），预算耗尽即停睡转进——把无响应宿主上的等待钉在预算内；读
//! 调用本身受各自的平台约束（系统（macOS）辅助功能消息超时、系统
//! （Windows）组件对象模型（COM）跨进程调用耗时），预算只收窄睡眠段。
//!
//! 归一化边界：主路与剪贴板兜底两条通道的产物都在读取入口修剪两端空白，
//! 判空用修剪后的值——纯空白选区（误滑）按既有空选区语义上抛；下游全链
//! 路（内容闸门、原文视图、提示词、缓存键）只见同一份已修剪文本。
//!
//! 产物与收尾分离：兜底轮的恢复写盘与写盘归因随读取结果拆分移交恢复收尾
//! 结构（核心层文本收尾契约），调用方在取材产物上抛（弹窗链路启动）之后
//! 收口——恢复不挡在弹窗前。

use std::time::Duration;
use std::time::Instant;

use gloss_core::log::{info, thread};
use gloss_core::model::GlossError;
use gloss_core::ports::{TextRead, TextSettle};

const READ_WAIT_BUDGET: Duration = Duration::from_millis(700);

const EMPTY_SETTLE_SCHEDULE_MS: [u64; 3] = [25, 75, 150];

const ACTIVATION_SETTLE_DELAY: Duration = Duration::from_millis(250);

const ACTIVATION_SETTLE_BUDGET: usize = 5;

fn settle_delay(attempt: usize) -> Option<Duration> {
    EMPTY_SETTLE_SCHEDULE_MS
        .get(attempt)
        .map(|ms| Duration::from_millis(*ms))
}

fn activation_delay(attempt: usize) -> Option<Duration> {
    (attempt < ACTIVATION_SETTLE_BUDGET).then_some(ACTIVATION_SETTLE_DELAY)
}

fn give_up_tail(with_gate: bool) -> &'static str {
    if with_gate {
        "trying to wake the accessibility tree"
    } else {
        "falling back to the clipboard"
    }
}

struct Pacing<S, A, E> {
    settle: S,
    activation: A,
    waits_exhausted: E,
}

impl<S, A, E> Pacing<S, A, E>
where
    S: Fn(usize) -> Option<Duration>,
    A: Fn(usize) -> Option<Duration>,
    E: Fn() -> bool,
{
    fn new(settle: S, activation: A, waits_exhausted: E) -> Self {
        Self {
            settle,
            activation,
            waits_exhausted,
        }
    }
}

fn combine(
    mut main: impl FnMut() -> Result<String, GlossError>,
    responds_to_wake: bool,
    wake: Option<&mut dyn FnMut() -> bool>,
    fallback: impl FnOnce() -> Result<String, GlossError>,
    pacing: Pacing<
        impl Fn(usize) -> Option<Duration>,
        impl Fn(usize) -> Option<Duration>,
        impl Fn() -> bool,
    >,
) -> Result<String, GlossError> {
    let mut attempts = 0;
    loop {
        match main() {
            Ok(text) => return Ok(text),
            Err(err @ GlossError::AccessibilityDenied) if wake.is_some() => return Err(err),
            Err(GlossError::SelectionEmpty) => {
                if wake.is_some() && responds_to_wake && attempts == 0 {
                    return wake_then(main, wake, fallback, &pacing);
                }
                let Some(delay) = (pacing.settle)(attempts) else {
                    info!(
                        thread = thread::EVENT,
                        "selection still empty after settle budget, {}",
                        give_up_tail(wake.is_some())
                    );
                    return wake_then(main, wake, fallback, &pacing);
                };
                if (pacing.waits_exhausted)() {
                    info!(
                        thread = thread::EVENT,
                        "selection wait budget exhausted, {}",
                        give_up_tail(wake.is_some())
                    );
                    return wake_then(main, wake, fallback, &pacing);
                }
                attempts += 1;
                std::thread::sleep(delay);
            }
            Err(_) => return wake_then(main, wake, fallback, &pacing),
        }
    }
}

fn wake_then(
    mut main: impl FnMut() -> Result<String, GlossError>,
    wake: Option<&mut dyn FnMut() -> bool>,
    fallback: impl FnOnce() -> Result<String, GlossError>,
    pacing: &Pacing<
        impl Fn(usize) -> Option<Duration>,
        impl Fn(usize) -> Option<Duration>,
        impl Fn() -> bool,
    >,
) -> Result<String, GlossError> {
    let Some(wake) = wake else {
        return fallback();
    };
    if !wake() {
        return fallback();
    }
    let mut attempts = 0;
    loop {
        match main() {
            Ok(text) => return Ok(text),
            Err(err @ GlossError::AccessibilityDenied) => return Err(err),
            Err(GlossError::SelectionEmpty) => {
                let Some(delay) = (pacing.activation)(attempts) else {
                    info!(
                        thread = thread::EVENT,
                        "accessibility tree still silent after activation budget, falling back to the clipboard"
                    );
                    return fallback();
                };
                if (pacing.waits_exhausted)() {
                    info!(
                        thread = thread::EVENT,
                        "accessibility wait budget exhausted, falling back to the clipboard"
                    );
                    return fallback();
                }
                attempts += 1;
                std::thread::sleep(delay);
            }
            Err(_) => return fallback(),
        }
    }
}

fn normalize(raw: String) -> Result<String, GlossError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(GlossError::SelectionEmpty);
    }
    if trimmed.len() == raw.len() {
        return Ok(raw);
    }
    Ok(trimmed.to_owned())
}

pub(crate) fn acquire(
    main: impl FnMut() -> Result<String, GlossError>,
    responds_to_wake: bool,
    wake: Option<&mut dyn FnMut() -> bool>,
    fallback: impl FnOnce() -> (Result<String, GlossError>, TextSettle),
) -> TextRead {
    let started = Instant::now();
    let mut settle = TextSettle::idle();
    let outcome = combine(
        main,
        responds_to_wake,
        wake,
        || {
            let (outcome, round) = fallback();
            settle = round;
            outcome
        },
        Pacing::new(settle_delay, activation_delay, || {
            started.elapsed() >= READ_WAIT_BUDGET
        }),
    )
    .and_then(normalize);
    TextRead::new(outcome, settle)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use gloss_core::model::GlossError;
    use gloss_core::ports::TextSettle;

    use super::{
        ACTIVATION_SETTLE_BUDGET, ACTIVATION_SETTLE_DELAY, EMPTY_SETTLE_SCHEDULE_MS, Pacing,
        acquire, activation_delay, combine, normalize, settle_delay,
    };

    fn main_ok(text: &str) -> impl FnMut() -> Result<String, GlossError> {
        let text = text.to_owned();
        move || Ok(text.clone())
    }

    fn never_wake() -> bool {
        false
    }

    fn budget_never_exhausted() -> bool {
        false
    }

    fn no_delay(attempt: usize) -> Option<Duration> {
        settle_delay(attempt).map(|_| Duration::ZERO)
    }

    fn no_wake_delay(attempt: usize) -> Option<Duration> {
        activation_delay(attempt).map(|_| Duration::ZERO)
    }

    fn deferred_settle() -> (TextSettle, Arc<AtomicBool>) {
        let finished = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&finished);
        (
            TextSettle::deferred(move || flag.store(true, Ordering::Relaxed)),
            finished,
        )
    }

    #[test]
    fn fallback_is_lazy_on_main_success() {
        let mut clipboard_called = false;
        let outcome = combine(
            main_ok("from main"),
            false,
            None,
            || {
                clipboard_called = true;
                Ok::<String, GlossError>("from clipboard".into())
            },
            Pacing::new(no_delay, no_wake_delay, budget_never_exhausted),
        );
        assert_eq!(outcome, Ok("from main".into()));
        assert!(
            !clipboard_called,
            "main success must not touch the clipboard"
        );
    }

    #[test]
    fn without_a_gate_empty_selection_settles_then_falls_back() {
        let mut reads = 0;
        let mut fallback_calls = 0;
        let outcome = combine(
            || {
                reads += 1;
                Err::<String, GlossError>(GlossError::SelectionEmpty)
            },
            false,
            None,
            || {
                fallback_calls += 1;
                Ok::<String, GlossError>("from clipboard".into())
            },
            Pacing::new(no_delay, no_wake_delay, budget_never_exhausted),
        );
        assert_eq!(outcome, Ok("from clipboard".into()));
        assert_eq!(
            reads,
            EMPTY_SETTLE_SCHEDULE_MS.len() + 1,
            "the main read is retried through the whole settle schedule first"
        );
        assert_eq!(
            fallback_calls, 1,
            "the fallback runs once, only after the schedule is exhausted"
        );
    }

    #[test]
    fn without_a_gate_a_settled_reread_adopts_the_late_selection() {
        let mut reads = 0;
        let outcome = combine(
            || {
                reads += 1;
                if reads == 1 {
                    Err(GlossError::SelectionEmpty)
                } else {
                    Ok("the selection showed up".to_owned())
                }
            },
            false,
            None,
            || panic!("a settled selection must not fall back to the clipboard"),
            Pacing::new(no_delay, no_wake_delay, budget_never_exhausted),
        );
        assert_eq!(outcome, Ok("the selection showed up".to_owned()));
        assert_eq!(reads, 2);
    }

    #[test]
    fn without_a_gate_an_unavailable_main_falls_back_directly() {
        let main = || Err::<String, GlossError>(GlossError::SelectionUnavailable);
        assert_eq!(
            combine(
                main,
                false,
                None,
                || Ok::<String, GlossError>("from clipboard".into()),
                Pacing::new(no_delay, no_wake_delay, budget_never_exhausted),
            ),
            Ok("from clipboard".into())
        );
        assert_eq!(
            combine(
                main,
                false,
                None,
                || Err(GlossError::SelectionUnavailable),
                Pacing::new(no_delay, no_wake_delay, budget_never_exhausted),
            ),
            Err(GlossError::SelectionUnavailable)
        );
    }

    #[test]
    fn without_a_gate_denied_is_just_another_failure() {
        let main = || Err::<String, GlossError>(GlossError::AccessibilityDenied);
        assert_eq!(
            combine(
                main,
                false,
                None,
                || Ok::<String, GlossError>("from clipboard".into()),
                Pacing::new(no_delay, no_wake_delay, budget_never_exhausted),
            ),
            Ok("from clipboard".into())
        );
        assert_eq!(
            combine(
                main,
                false,
                None,
                || Err(GlossError::SelectionUnavailable),
                Pacing::new(no_delay, no_wake_delay, budget_never_exhausted),
            ),
            Err(GlossError::SelectionUnavailable)
        );
    }

    #[test]
    fn without_a_gate_the_wait_budget_ends_the_settling_straight_to_the_fallback() {
        let mut reads = 0;
        let outcome = combine(
            || {
                reads += 1;
                Err::<String, GlossError>(GlossError::SelectionEmpty)
            },
            false,
            None,
            || Ok::<String, GlossError>("from clipboard".into()),
            Pacing::new(no_delay, no_wake_delay, || true),
        );
        assert_eq!(outcome, Ok("from clipboard".into()));
        assert_eq!(
            reads, 1,
            "one initial read before the budget cut (no activation gate on this mode)"
        );
    }

    #[test]
    fn permission_denied_skips_fallback() {
        let mut clipboard_called = false;
        let mut wake = never_wake;
        let wake_ref: &mut dyn FnMut() -> bool = &mut wake;
        let outcome = combine(
            || Err::<String, GlossError>(GlossError::AccessibilityDenied),
            false,
            Some(wake_ref),
            || {
                clipboard_called = true;
                Ok::<String, GlossError>("from clipboard".into())
            },
            Pacing::new(no_delay, no_wake_delay, budget_never_exhausted),
        );
        assert_eq!(outcome, Err(GlossError::AccessibilityDenied));
        assert!(
            !clipboard_called,
            "permission denied must not fall back to the clipboard"
        );
    }

    #[test]
    fn empty_selection_settles_then_falls_back_to_the_clipboard() {
        let mut reads = 0;
        let mut wake_calls = 0;
        let mut wake = || {
            wake_calls += 1;
            false
        };
        let mut fallback_calls = 0;
        let outcome = {
            let wake_ref: &mut dyn FnMut() -> bool = &mut wake;
            combine(
                || {
                    reads += 1;
                    Err::<String, GlossError>(GlossError::SelectionEmpty)
                },
                false,
                Some(wake_ref),
                || {
                    fallback_calls += 1;
                    Ok::<String, GlossError>("from clipboard".into())
                },
                Pacing::new(no_delay, no_wake_delay, budget_never_exhausted),
            )
        };
        assert_eq!(outcome, Ok("from clipboard".into()));
        assert_eq!(
            reads,
            EMPTY_SETTLE_SCHEDULE_MS.len() + 1,
            "the main read is retried through the whole settle schedule first"
        );
        assert_eq!(
            fallback_calls, 1,
            "the fallback runs once, after the schedule and the declined gate"
        );
        assert_eq!(
            wake_calls, 1,
            "the gate is consulted once between the schedule and the fallback"
        );
    }

    #[test]
    fn empty_then_ready_main_read_adopts_the_late_selection() {
        let mut reads = 0;
        let mut wake = never_wake;
        let outcome = {
            let wake_ref: &mut dyn FnMut() -> bool = &mut wake;
            combine(
                || {
                    reads += 1;
                    if reads == 1 {
                        Err(GlossError::SelectionEmpty)
                    } else {
                        Ok("the selection showed up".to_owned())
                    }
                },
                false,
                Some(wake_ref),
                || panic!("a settled selection must not fall back to the clipboard"),
                Pacing::new(no_delay, no_wake_delay, budget_never_exhausted),
            )
        };
        assert_eq!(outcome, Ok("the selection showed up".to_owned()));
        assert_eq!(reads, 2);
    }

    #[test]
    fn unavailable_main_consults_the_gate_once_then_falls_back() {
        let main = || Err::<String, GlossError>(GlossError::SelectionUnavailable);
        let mut wake_calls = 0;
        let mut wake = || {
            wake_calls += 1;
            false
        };
        let outcome = {
            let wake_ref: &mut dyn FnMut() -> bool = &mut wake;
            combine(
                main,
                false,
                Some(wake_ref),
                || Ok::<String, GlossError>("from clipboard".into()),
                Pacing::new(no_delay, no_wake_delay, budget_never_exhausted),
            )
        };
        assert_eq!(outcome, Ok("from clipboard".into()));
        assert_eq!(wake_calls, 1, "the gate is consulted once per give-up");
        let mut wake = never_wake;
        let outcome = {
            let wake_ref: &mut dyn FnMut() -> bool = &mut wake;
            combine(
                main,
                false,
                Some(wake_ref),
                || Err(GlossError::SelectionUnavailable),
                Pacing::new(no_delay, no_wake_delay, budget_never_exhausted),
            )
        };
        assert_eq!(outcome, Err(GlossError::SelectionUnavailable));
    }

    #[test]
    fn success_and_denial_never_consult_the_gate() {
        let mut wake_calls = 0;
        let mut wake = || {
            wake_calls += 1;
            false
        };
        let outcome = {
            let wake_ref: &mut dyn FnMut() -> bool = &mut wake;
            combine(
                main_ok("from main"),
                false,
                Some(wake_ref),
                || Ok::<String, GlossError>("from clipboard".into()),
                Pacing::new(no_delay, no_wake_delay, budget_never_exhausted),
            )
        };
        assert_eq!(outcome, Ok("from main".into()));
        let outcome = {
            let wake_ref: &mut dyn FnMut() -> bool = &mut wake;
            combine(
                || Err::<String, GlossError>(GlossError::AccessibilityDenied),
                false,
                Some(wake_ref),
                || Ok::<String, GlossError>("from clipboard".into()),
                Pacing::new(no_delay, no_wake_delay, budget_never_exhausted),
            )
        };
        assert_eq!(outcome, Err(GlossError::AccessibilityDenied));
        assert_eq!(
            wake_calls, 0,
            "activation is only consulted when the main channel gives up"
        );
    }

    #[test]
    fn a_declined_wake_goes_straight_to_the_clipboard() {
        let mut reads = 0;
        let mut wake = never_wake;
        let outcome = {
            let wake_ref: &mut dyn FnMut() -> bool = &mut wake;
            combine(
                || {
                    reads += 1;
                    Err::<String, GlossError>(GlossError::SelectionEmpty)
                },
                false,
                Some(wake_ref),
                || Ok::<String, GlossError>("from clipboard".into()),
                Pacing::new(no_delay, no_wake_delay, budget_never_exhausted),
            )
        };
        assert_eq!(outcome, Ok("from clipboard".into()));
        assert_eq!(
            reads,
            EMPTY_SETTLE_SCHEDULE_MS.len() + 1,
            "the settle schedule runs before the gate is consulted"
        );
    }

    #[test]
    fn a_fresh_wake_re_reads_and_adopts_the_late_selection() {
        let mut reads = 0;
        let mut wake = || true;
        let outcome = {
            let wake_ref: &mut dyn FnMut() -> bool = &mut wake;
            combine(
                || {
                    reads += 1;
                    if reads == 1 {
                        Err(GlossError::SelectionEmpty)
                    } else {
                        Ok("the woken tree answered".to_owned())
                    }
                },
                false,
                Some(wake_ref),
                || panic!("a freshly woken tree must not fall back while it may still answer"),
                Pacing::new(no_delay, no_wake_delay, budget_never_exhausted),
            )
        };
        assert_eq!(outcome, Ok("the woken tree answered".to_owned()));
        assert_eq!(reads, 2);
    }

    #[test]
    fn a_woken_tree_that_stays_silent_falls_back_after_the_budget() {
        let mut reads = 0;
        let mut wake = || true;
        let outcome = {
            let wake_ref: &mut dyn FnMut() -> bool = &mut wake;
            combine(
                || {
                    reads += 1;
                    Err::<String, GlossError>(GlossError::SelectionEmpty)
                },
                false,
                Some(wake_ref),
                || Ok::<String, GlossError>("from clipboard".into()),
                Pacing::new(no_delay, no_wake_delay, budget_never_exhausted),
            )
        };
        assert_eq!(outcome, Ok("from clipboard".into()));
        assert_eq!(
            reads,
            EMPTY_SETTLE_SCHEDULE_MS.len() + 1 + ACTIVATION_SETTLE_BUDGET + 1,
            "the settle schedule and then the activation budget each fully run"
        );
    }

    #[test]
    fn a_woken_tree_that_errors_does_not_burn_the_activation_budget() {
        let mut reads = 0;
        let mut wake = || true;
        let outcome = {
            let wake_ref: &mut dyn FnMut() -> bool = &mut wake;
            combine(
                || {
                    reads += 1;
                    Err::<String, GlossError>(GlossError::SelectionUnavailable)
                },
                false,
                Some(wake_ref),
                || Ok::<String, GlossError>("from clipboard".into()),
                Pacing::new(no_delay, no_wake_delay, budget_never_exhausted),
            )
        };
        assert_eq!(outcome, Ok("from clipboard".into()));
        assert_eq!(reads, 2, "the initial read plus one activation re-read");
    }

    #[test]
    fn a_woken_tree_that_loses_permission_propagates_the_denial() {
        let mut reads = 0;
        let mut wake = || true;
        let outcome = {
            let wake_ref: &mut dyn FnMut() -> bool = &mut wake;
            combine(
                || {
                    reads += 1;
                    if reads <= EMPTY_SETTLE_SCHEDULE_MS.len() + 1 {
                        Err(GlossError::SelectionEmpty)
                    } else {
                        Err(GlossError::AccessibilityDenied)
                    }
                },
                false,
                Some(wake_ref),
                || {
                    panic!(
                        "a permission loss during activation must not fall back to the clipboard"
                    )
                },
                Pacing::new(no_delay, no_wake_delay, budget_never_exhausted),
            )
        };
        assert_eq!(outcome, Err(GlossError::AccessibilityDenied));
        assert_eq!(
            reads,
            EMPTY_SETTLE_SCHEDULE_MS.len() + 2,
            "the settle schedule runs out, then the activation re-read hits the denial"
        );
    }

    #[test]
    fn a_rescued_host_skips_the_settle_schedule() {
        let mut reads = 0;
        let mut wake = || true;
        let outcome = {
            let wake_ref: &mut dyn FnMut() -> bool = &mut wake;
            combine(
                || {
                    reads += 1;
                    if reads == 1 {
                        Err(GlossError::SelectionEmpty)
                    } else {
                        Ok("rescued".to_owned())
                    }
                },
                true,
                Some(wake_ref),
                || panic!("a woken tree that answers must not reach the clipboard"),
                Pacing::new(no_delay, no_wake_delay, budget_never_exhausted),
            )
        };
        assert_eq!(outcome, Ok("rescued".into()));
        assert_eq!(
            reads, 2,
            "the first empty read jumps straight to the activation re-read"
        );
    }

    #[test]
    fn a_rescued_host_with_a_declined_wake_falls_back_without_settling() {
        let mut reads = 0;
        let mut wake = never_wake;
        let outcome = {
            let wake_ref: &mut dyn FnMut() -> bool = &mut wake;
            combine(
                || {
                    reads += 1;
                    Err::<String, GlossError>(GlossError::SelectionEmpty)
                },
                true,
                Some(wake_ref),
                || Ok::<String, GlossError>("from clipboard".into()),
                Pacing::new(no_delay, no_wake_delay, budget_never_exhausted),
            )
        };
        assert_eq!(outcome, Ok("from clipboard".into()));
        assert_eq!(
            reads, 1,
            "the rescued host skips the settle schedule even when the wake declines"
        );
    }

    #[test]
    fn the_wait_budget_ends_the_settling_early() {
        let mut reads = 0;
        let mut wake = || true;
        let outcome = {
            let wake_ref: &mut dyn FnMut() -> bool = &mut wake;
            combine(
                || {
                    reads += 1;
                    if reads == 1 {
                        Err(GlossError::SelectionEmpty)
                    } else {
                        Ok("woken".to_owned())
                    }
                },
                false,
                Some(wake_ref),
                || panic!("the budget cut goes to the wake gate, not the clipboard"),
                Pacing::new(no_delay, no_wake_delay, || true),
            )
        };
        assert_eq!(outcome, Ok("woken".into()));
        assert_eq!(
            reads, 2,
            "the schedule is cut at the first wait, straight to the wake gate"
        );
    }

    #[test]
    fn the_wait_budget_ends_the_activation_re_reads() {
        let mut reads = 0;
        let mut wake = || true;
        let outcome = {
            let wake_ref: &mut dyn FnMut() -> bool = &mut wake;
            combine(
                || {
                    reads += 1;
                    Err::<String, GlossError>(GlossError::SelectionEmpty)
                },
                false,
                Some(wake_ref),
                || Ok("from clipboard".to_owned()),
                Pacing::new(no_delay, no_wake_delay, || true),
            )
        };
        assert_eq!(outcome, Ok("from clipboard".into()));
        assert_eq!(
            reads, 2,
            "one initial read plus one activation re-read before the fallback"
        );
    }

    #[test]
    fn activation_budget_is_bounded_and_positive() {
        assert_eq!(activation_delay(0), Some(ACTIVATION_SETTLE_DELAY));
        assert_eq!(
            activation_delay(ACTIVATION_SETTLE_BUDGET - 1),
            Some(ACTIVATION_SETTLE_DELAY)
        );
        assert_eq!(activation_delay(ACTIVATION_SETTLE_BUDGET), None);
        assert_eq!(activation_delay(usize::MAX), None);
        assert!(ACTIVATION_SETTLE_DELAY > Duration::ZERO);
    }

    #[test]
    fn settle_schedule_is_bounded_and_positive() {
        for (attempt, expected) in EMPTY_SETTLE_SCHEDULE_MS.iter().enumerate() {
            assert_eq!(
                settle_delay(attempt),
                Some(Duration::from_millis(*expected)),
                "attempt {attempt} follows the geometric schedule"
            );
        }
        assert_eq!(
            settle_delay(EMPTY_SETTLE_SCHEDULE_MS.len()),
            None,
            "the schedule exhausts into the continuation gate"
        );
        assert_eq!(settle_delay(usize::MAX), None);
        assert!(
            EMPTY_SETTLE_SCHEDULE_MS.iter().all(|ms| *ms > 0),
            "every settle wait is positive"
        );
    }

    #[test]
    fn acquired_text_is_trimmed_at_the_boundary() {
        assert_eq!(
            normalize("  hello world \n\t".to_owned()),
            Ok("hello world".to_owned())
        );
        assert_eq!(normalize("hello".to_owned()), Ok("hello".to_owned()));
    }

    #[test]
    fn acquired_text_keeps_interior_whitespace_verbatim() {
        assert_eq!(
            normalize("  a\n b\tc ".to_owned()),
            Ok("a\n b\tc".to_owned())
        );
    }

    #[test]
    fn pure_whitespace_selection_maps_to_selection_empty() {
        for raw in ["   ", "\n\t \n", "\u{3000}", ""] {
            assert_eq!(
                normalize(raw.to_owned()),
                Err(GlossError::SelectionEmpty),
                "a whitespace-only selection must take the existing empty-selection semantics"
            );
        }
    }

    #[test]
    fn the_fallback_rounds_settle_rides_out_for_a_later_finish() {
        let (settle, finished) = deferred_settle();
        let read = acquire(
            || Err::<String, GlossError>(GlossError::SelectionUnavailable),
            false,
            None,
            || (Ok::<String, GlossError>("from clipboard".into()), settle),
        );
        let (outcome, settle) = read.into_parts();
        assert_eq!(outcome, Ok("from clipboard".into()));
        assert!(
            !finished.load(Ordering::Relaxed),
            "the settle must wait for the caller to finish it"
        );
        settle.finish();
        assert!(
            finished.load(Ordering::Relaxed),
            "the fallback round's restore plan must ride on the read outcome"
        );
    }

    #[test]
    fn a_failed_fallback_round_still_hands_back_its_settle() {
        let (settle, finished) = deferred_settle();
        let read = acquire(
            || Err::<String, GlossError>(GlossError::SelectionUnavailable),
            false,
            None,
            || {
                (
                    Err::<String, GlossError>(GlossError::SelectionUnavailable),
                    settle,
                )
            },
        );
        let (outcome, settle) = read.into_parts();
        assert_eq!(outcome, Err(GlossError::SelectionUnavailable));
        settle.finish();
        assert!(
            finished.load(Ordering::Relaxed),
            "a failed fallback round still rides its restore plan out"
        );
    }

    #[test]
    fn a_main_only_read_carries_an_idle_settle() {
        let read = acquire(main_ok("already there"), false, None, || {
            panic!("the fallback must not run when the main channel answers")
        });
        let (outcome, settle) = read.into_parts();
        assert_eq!(outcome, Ok("already there".into()));
        settle.finish();
    }

    #[test]
    fn the_read_entry_normalizes_the_combined_outcome() {
        let read = acquire(
            || Ok::<String, GlossError>("  padded  ".to_owned()),
            false,
            None,
            || panic!("a successful main read must not reach the clipboard"),
        );
        assert_eq!(
            read.into_parts().0,
            Ok("padded".to_owned()),
            "the read entry trims both ends before handing the outcome out"
        );
        let read = acquire(
            || Ok::<String, GlossError>("   ".to_owned()),
            false,
            None,
            || panic!("a whitespace-only read must not reach the clipboard"),
        );
        assert_eq!(
            read.into_parts().0,
            Err(GlossError::SelectionEmpty),
            "a whitespace-only selection takes the existing empty-selection semantics"
        );
    }

    #[test]
    fn the_rescued_flag_reaches_the_gate_through_the_read_entry() {
        let mut reads = 0;
        let mut wake_calls = 0;
        let mut wake = || {
            wake_calls += 1;
            true
        };
        let read = {
            let wake_ref: &mut dyn FnMut() -> bool = &mut wake;
            acquire(
                || {
                    reads += 1;
                    if reads == 1 {
                        Err(GlossError::SelectionEmpty)
                    } else {
                        Ok("rescued".to_owned())
                    }
                },
                true,
                Some(wake_ref),
                || panic!("a rescued host that answers must not reach the clipboard"),
            )
        };
        let (outcome, _) = read.into_parts();
        assert_eq!(outcome, Ok("rescued".into()));
        assert_eq!(
            reads, 2,
            "the rescued flag skips the settle schedule at the read entry"
        );
        assert_eq!(wake_calls, 1, "the gate is consulted exactly once");
    }
}
