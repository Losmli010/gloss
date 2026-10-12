//! 选区读取的组合通道：AX 优先，读不到时先尝试激活宿主的无障碍树，仍
//! 拿不到再降级剪贴板兜底。
//!
//! 降级边界：AX 报权限缺失时不兜底——兜底依赖按键注入，未授权时注入会被
//! 系统静默忽略，白等超时只会拖慢失败路径；此时把权限语义原样上抛，由
//! 上层做权限引导。AX 报「选区为空」先短暂让渡补读（见 [`settle_delay`]）
//! ——释放瞬间读到的空多半是还没写好；预算耗尽仍空，与「读不到」一样
//! 先过激活闸门（见 [`wake_then`]）：对 Chromium 系宿主（无障碍树默认
//! 不建、AX 读选区恒失败的大头）注入开关按需开树，树一回答文本直接
//! 采纳，免注入免剪贴板副作用；激活落空才落剪贴板兜底。兜底的确认
//! 写入能分辨「确实没选」与「读了但拿不到」——注入 Cmd+C 后剪贴板没有
//! 真实写入就不采纳，误滑不会把陈旧剪贴板内容当成选区。
//!
//! 归一化边界：AX 与剪贴板兜底两条通道的产物都在入口（[`CompositeReader`]
//! 的读取方法）修剪两端空白，判空用修剪后的值——纯空白选区（误滑）按既有
//! `SelectionEmpty` 语义上抛；下游全链路（内容闸门、原文视图、prompt、
//! 缓存 key）只见同一份已修剪文本。

use std::time::Duration;

use gloss_core::log::{info, thread};
use gloss_core::model::GlossError;

/// 两次读取之间的让渡时长：鼠标释放到目标应用把选区写入 AX 属性之间有
/// 时序差（长选区、代码编辑器更慢），释放瞬间读到的「空」多半是还没写好
/// 而不是没有。让渡在事件线程上睡眠，与剪贴板兜底的阻塞语义同款（数十至
/// 数百毫秒，远小于兜底路径的最坏 4s）。
const EMPTY_SETTLE_DELAY: Duration = Duration::from_millis(120);

/// 让渡次数上限：最多 3 次让渡 × 120ms ≈ 360ms 睡眠预算（首读 + 3 次补读
/// 共 4 次读取，各次 AX 往返耗时另计），以「误滑收回前骨架多亮数百毫秒」
/// 换真实划词的判定率，超出即认定确实无选区。
const EMPTY_SETTLE_BUDGET: usize = 3;

/// 激活补读之间的等待时长：注入开关后宿主异步建无障碍树（数百毫秒起），
/// 补读之间让宿主建一会儿。等待在事件线程上睡眠，与让渡的阻塞语义同款。
const ACTIVATION_SETTLE_DELAY: Duration = Duration::from_millis(250);

/// 激活补读次数上限：最多 5 次 × 250ms ≈ 1.25s 建树等待（各次 AX 往返
/// 耗时另计，宿主不响应时受 1s 消息超时兜底）。只发生在「激活注入首次
/// 成功」的那一个手势，换的是此后所有手势免按键注入、免剪贴板副作用。
const ACTIVATION_SETTLE_BUDGET: usize = 5;

/// 第 `attempt` 次空读后的让渡时长；预算用尽返回 `None`（纯逻辑，单测覆盖）。
fn settle_delay(attempt: usize) -> Option<Duration> {
    (attempt < EMPTY_SETTLE_BUDGET).then_some(EMPTY_SETTLE_DELAY)
}

/// 第 `attempt` 次激活补读空结果后的等待时长；预算用尽返回 `None`
/// （纯逻辑，单测覆盖）。
fn activation_delay(attempt: usize) -> Option<Duration> {
    (attempt < ACTIVATION_SETTLE_BUDGET).then_some(ACTIVATION_SETTLE_DELAY)
}

/// 组合判定（纯逻辑，单测覆盖）：AX 成功直接采纳；权限缺失原样上抛且不
/// 评估激活与兜底（见模块注释）；空选区按 [`settle_delay`] 让渡后补读，
/// 预算耗尽仍空与「其余读不到的情形」一样先过激活闸门（[`wake_then`]），
/// 激活落空才落到兜底结果。`wake` 是惰性求值——激活含跨进程属性写入，
/// 未走到就不该有副作用；`fallback` 同理——兜底路径含按键注入。
/// `delay_for`/`wake_delay_for` 把让渡与激活的节奏参数化，测试零睡眠
/// 直达时序断言。
fn combine(
    mut ax: impl FnMut() -> Result<String, GlossError>,
    mut wake: impl FnMut() -> bool,
    fallback: impl FnOnce() -> Result<String, GlossError>,
    delay_for: impl Fn(usize) -> Option<Duration>,
    wake_delay_for: impl Fn(usize) -> Option<Duration>,
) -> Result<String, GlossError> {
    let mut attempts = 0;
    loop {
        match ax() {
            Ok(text) => return Ok(text),
            Err(err @ GlossError::AccessibilityDenied) => return Err(err),
            Err(GlossError::SelectionEmpty) => {
                let Some(delay) = delay_for(attempts) else {
                    info!(
                        thread = thread::EVENT,
                        "selection still empty after settle budget, trying to wake the accessibility tree"
                    );
                    return wake_then(ax, &mut wake, fallback, wake_delay_for);
                };
                attempts += 1;
                std::thread::sleep(delay);
            }
            Err(_) => return wake_then(ax, &mut wake, fallback, wake_delay_for),
        }
    }
}

/// 激活闸门（纯逻辑，单测覆盖）：`wake` 报「无需补读」（该宿主已评估
/// 过、不是 Chromium 家族或注入失败）时立即落兜底，不花任何等待；刚
/// 注入成功时按 [`activation_delay`] 的节奏补读，树一回答就采纳，预算
/// 耗尽仍空或读不到再落兜底——补读期间宿主建树是异步的，空结果多半是
/// 还没建好而不是没有。补读中权限缺失与让渡环同款原样上抛：注入会被
/// 系统静默忽略，兜底只会白等。
fn wake_then(
    mut ax: impl FnMut() -> Result<String, GlossError>,
    wake: &mut impl FnMut() -> bool,
    fallback: impl FnOnce() -> Result<String, GlossError>,
    wake_delay_for: impl Fn(usize) -> Option<Duration>,
) -> Result<String, GlossError> {
    if !wake() {
        return fallback();
    }
    let mut attempts = 0;
    loop {
        match ax() {
            Ok(text) => return Ok(text),
            Err(err @ GlossError::AccessibilityDenied) => return Err(err),
            Err(GlossError::SelectionEmpty) => {
                let Some(delay) = wake_delay_for(attempts) else {
                    info!(
                        thread = thread::EVENT,
                        "accessibility tree still silent after activation budget, falling back to the clipboard"
                    );
                    return fallback();
                };
                attempts += 1;
                std::thread::sleep(delay);
            }
            Err(_) => return fallback(),
        }
    }
}

/// 取材边界的归一化（纯逻辑，单测覆盖）：读到的文本修剪两端空白后放行，
/// 判空用修剪后的值——纯空白选区（误滑）按既有 [`GlossError::SelectionEmpty`]
/// 语义上抛。AX 与剪贴板兜底两条通道的产物都经此收口，让渡补读与兜底
/// 注入只服务「读不到」，不为已知纯空白的选区再花时序预算。
///
/// 两端本无空白时原缓冲原样归还，不复制整段（选区可达 8MB 上限）。
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

mod imp {
    use gloss_core::model::GlossError;

    use super::{activation_delay, combine, normalize, settle_delay};
    use crate::selection::accessibility::AccessibilityReader;
    use crate::selection::activation::AxActivator;
    use crate::selection::clipboard::ClipboardFallbackReader;

    /// 选区读取的组合实现：AX 优先，读不到时先尝试激活宿主的无障碍树，
    /// 仍拿不到再降级剪贴板兜底。激活器随读取器跨手势复用（会话内按
    /// pid 去重）。
    #[derive(Debug, Default)]
    pub struct CompositeReader {
        ax: AccessibilityReader,
        clipboard: ClipboardFallbackReader,
        activator: AxActivator,
    }

    impl CompositeReader {
        /// 创建组合读取器。
        pub fn new() -> Self {
            Self::default()
        }

        /// 读取前台应用的选中文本。
        ///
        /// 返回的文本已修剪两端空白；纯空白选区按
        /// [`GlossError::SelectionEmpty`] 报告。调用方保证：在平台事件线程
        /// 上调用。
        pub fn read(&mut self) -> Result<String, GlossError> {
            combine(
                || self.ax.read(),
                || self.activator.wake(),
                || self.clipboard.read(),
                settle_delay,
                activation_delay,
            )
            .and_then(normalize)
        }
    }
}

pub use imp::CompositeReader;

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gloss_core::model::GlossError;

    use super::{
        ACTIVATION_SETTLE_BUDGET, ACTIVATION_SETTLE_DELAY, EMPTY_SETTLE_BUDGET, EMPTY_SETTLE_DELAY,
        activation_delay, combine, normalize, settle_delay,
    };

    fn ax_ok(text: &str) -> impl FnMut() -> Result<String, GlossError> {
        let text = text.to_owned();
        move || Ok(text.clone())
    }

    fn clipboard_ok(text: &str) -> Result<String, GlossError> {
        Ok(text.to_owned())
    }

    fn never_wake() -> bool {
        false
    }

    fn no_delay(attempt: usize) -> Option<Duration> {
        settle_delay(attempt).map(|_| Duration::ZERO)
    }

    fn no_wake_delay(attempt: usize) -> Option<Duration> {
        activation_delay(attempt).map(|_| Duration::ZERO)
    }

    #[test]
    fn fallback_is_lazy_on_ax_success() {
        let mut clipboard_called = false;
        let outcome = combine(
            ax_ok("from ax"),
            never_wake,
            || {
                clipboard_called = true;
                Ok::<String, GlossError>("from clipboard".into())
            },
            no_delay,
            no_delay,
        );
        assert_eq!(outcome, Ok("from ax".into()));
        assert!(!clipboard_called, "ax success must not touch the clipboard");
    }

    #[test]
    fn permission_denied_skips_fallback() {
        let mut clipboard_called = false;
        let outcome = combine(
            || Err::<String, GlossError>(GlossError::AccessibilityDenied),
            never_wake,
            || {
                clipboard_called = true;
                Ok::<String, GlossError>("from clipboard".into())
            },
            no_delay,
            no_delay,
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
        let mut fallback_calls = 0;
        let outcome = combine(
            || {
                reads += 1;
                Err::<String, GlossError>(GlossError::SelectionEmpty)
            },
            never_wake,
            || {
                fallback_calls += 1;
                Ok::<String, GlossError>("from clipboard".into())
            },
            no_delay,
            no_delay,
        );
        assert_eq!(outcome, Ok("from clipboard".into()));
        assert_eq!(
            reads,
            EMPTY_SETTLE_BUDGET + 1,
            "the AX read is retried through the whole settle budget first"
        );
        assert_eq!(
            fallback_calls, 1,
            "the fallback runs once, only after the budget is exhausted"
        );
    }

    #[test]
    fn empty_then_ready_ax_read_adopts_the_late_selection() {
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
            never_wake,
            || panic!("a settled selection must not fall back to the clipboard"),
            no_delay,
            no_delay,
        );
        assert_eq!(outcome, Ok("the selection showed up".to_owned()));
        assert_eq!(reads, 2);
    }

    #[test]
    fn unavailable_ax_falls_back_to_clipboard() {
        let mut wake_calls = 0;
        let ax = || Err::<String, GlossError>(GlossError::SelectionUnavailable);
        assert_eq!(
            combine(
                ax,
                || {
                    wake_calls += 1;
                    false
                },
                || clipboard_ok("from clipboard"),
                no_delay,
                no_delay,
            ),
            Ok("from clipboard".into())
        );
        assert_eq!(wake_calls, 1, "the activator is consulted once per give-up");
        assert_eq!(
            combine(
                ax,
                never_wake,
                || Err(GlossError::SelectionUnavailable),
                no_delay,
                no_delay
            ),
            Err(GlossError::SelectionUnavailable)
        );
    }

    #[test]
    fn success_and_denial_never_consult_the_activator() {
        let mut wake_calls = 0;
        let mut wake = || {
            wake_calls += 1;
            false
        };
        let outcome = combine(
            ax_ok("from ax"),
            &mut wake,
            || Ok::<String, GlossError>("from clipboard".into()),
            no_delay,
            no_delay,
        );
        assert_eq!(outcome, Ok("from ax".into()));
        let outcome = combine(
            || Err::<String, GlossError>(GlossError::AccessibilityDenied),
            &mut wake,
            || Ok::<String, GlossError>("from clipboard".into()),
            no_delay,
            no_delay,
        );
        assert_eq!(outcome, Err(GlossError::AccessibilityDenied));
        assert_eq!(
            wake_calls, 0,
            "activation is only consulted when the ax channel gives up"
        );
    }

    #[test]
    fn a_declined_wake_goes_straight_to_the_clipboard() {
        let mut reads = 0;
        let outcome = combine(
            || {
                reads += 1;
                Err::<String, GlossError>(GlossError::SelectionEmpty)
            },
            never_wake,
            || Ok::<String, GlossError>("from clipboard".into()),
            no_delay,
            no_delay,
        );
        assert_eq!(outcome, Ok("from clipboard".into()));
        assert_eq!(
            reads,
            EMPTY_SETTLE_BUDGET + 1,
            "the settle budget runs before the activator is consulted"
        );
    }

    #[test]
    fn a_fresh_wake_re_reads_and_adopts_the_late_selection() {
        let mut reads = 0;
        let outcome = combine(
            || {
                reads += 1;
                if reads == 1 {
                    Err(GlossError::SelectionEmpty)
                } else {
                    Ok("the woken tree answered".to_owned())
                }
            },
            || true,
            || panic!("a freshly woken tree must not fall back while it may still answer"),
            no_delay,
            no_wake_delay,
        );
        assert_eq!(outcome, Ok("the woken tree answered".to_owned()));
        assert_eq!(reads, 2);
    }

    #[test]
    fn a_woken_tree_that_stays_silent_falls_back_after_the_budget() {
        let mut reads = 0;
        let outcome = combine(
            || {
                reads += 1;
                Err::<String, GlossError>(GlossError::SelectionEmpty)
            },
            || true,
            || Ok::<String, GlossError>("from clipboard".into()),
            no_delay,
            no_wake_delay,
        );
        assert_eq!(outcome, Ok("from clipboard".into()));
        assert_eq!(
            reads,
            EMPTY_SETTLE_BUDGET + 1 + ACTIVATION_SETTLE_BUDGET + 1,
            "the settle budget and then the activation budget each fully run"
        );
    }

    #[test]
    fn a_woken_tree_that_errors_does_not_burn_the_activation_budget() {
        let mut reads = 0;
        let outcome = combine(
            || {
                reads += 1;
                Err::<String, GlossError>(GlossError::SelectionUnavailable)
            },
            || true,
            || Ok::<String, GlossError>("from clipboard".into()),
            no_delay,
            no_wake_delay,
        );
        assert_eq!(outcome, Ok("from clipboard".into()));
        assert_eq!(reads, 2, "the initial read plus one activation re-read");
    }

    #[test]
    fn a_woken_tree_that_loses_permission_propagates_the_denial() {
        let mut reads = 0;
        let outcome = combine(
            || {
                reads += 1;
                if reads <= EMPTY_SETTLE_BUDGET + 1 {
                    Err(GlossError::SelectionEmpty)
                } else {
                    Err(GlossError::AccessibilityDenied)
                }
            },
            || true,
            || panic!("a permission loss during activation must not fall back to the clipboard"),
            no_delay,
            no_wake_delay,
        );
        assert_eq!(outcome, Err(GlossError::AccessibilityDenied));
        assert_eq!(
            reads,
            EMPTY_SETTLE_BUDGET + 2,
            "the settle budget runs out, then the activation re-read hits the denial"
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
    fn settle_budget_is_bounded_and_positive() {
        assert_eq!(settle_delay(0), Some(EMPTY_SETTLE_DELAY));
        assert_eq!(
            settle_delay(EMPTY_SETTLE_BUDGET - 1),
            Some(EMPTY_SETTLE_DELAY)
        );
        assert_eq!(settle_delay(EMPTY_SETTLE_BUDGET), None);
        assert_eq!(settle_delay(usize::MAX), None);
        assert!(EMPTY_SETTLE_DELAY > Duration::ZERO);
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
}
