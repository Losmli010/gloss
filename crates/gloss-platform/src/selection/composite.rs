//! 选区读取的组合通道：AX 优先，读不到时先尝试激活宿主的无障碍树，仍
//! 拿不到再降级剪贴板兜底。
//!
//! 降级边界：AX 报权限缺失时不兜底——兜底依赖按键注入，未授权时注入会被
//! 系统静默忽略，白等超时只会拖慢失败路径；此时把权限语义原样上抛，由
//! 上层做权限引导。AX 报「选区为空」分两路：唤醒常客（上一手势注入
//! 唤醒写成功的宿主）跳过让渡直接进激活补读——树缺席时等再久也读不到，
//! 注入才是解法；其余先按几何节奏让渡补读（见 [`settle_delay`]）——释放
//! 瞬间读到的空多半是还没写好。预算耗尽仍空，与「读不到」一样先过激活
//! 闸门（见 [`wake_then`]）：对 Chromium 系宿主（无障碍树默认不建、AX 读
//! 选区恒失败的大头）注入开关按需开树，树一回答文本直接采纳，免注入免
//! 剪贴板副作用；激活落空才落剪贴板兜底。兜底的确认写入能分辨「确实没
//! 选」与「读了但拿不到」——注入 Cmd+C 后剪贴板没有真实写入就不采纳，
//! 误滑不会把陈旧剪贴板内容当成选区。
//!
//! 时延边界：让渡与激活补读的睡眠段共享一份墙钟预算（[`AX_WAIT_BUDGET`]，
//! 自进入读取起算），预算耗尽即停睡转进——把无响应宿主上的等待钉在预算
//! 内；读调用本身受 AX 消息超时约束，预算只收窄睡眠段。
//!
//! 归一化边界：AX 与剪贴板兜底两条通道的产物都在入口（[`CompositeReader`]
//! 的读取方法）修剪两端空白，判空用修剪后的值——纯空白选区（误滑）按既有
//! `SelectionEmpty` 语义上抛；下游全链路（内容闸门、原文视图、prompt、
//! 缓存 key）只见同一份已修剪文本。
//!
//! 产物与收尾分离：兜底轮的恢复写盘与写盘归因随 [`SelectionRead::into_parts`]
//! 移交 [`SelectionSettle`]，调用方在取材产物上抛（弹窗链路启动）之后
//! [`SelectionSettle::finish`] 收口——恢复不挡在弹窗前。

use std::time::Duration;

use gloss_core::log::{info, thread};
use gloss_core::model::GlossError;

use crate::selection::clipboard::FallbackSettle;

/// 空读让渡的几何节奏（毫秒）：鼠标释放到目标应用把选区写入 AX 属性之间
/// 有时序差（长选区、代码编辑器更慢），释放瞬间读到的「空」多半是还没写
/// 好而不是没有；多数宿主在数十毫秒内落定，按几何间隔补读，早密晚疏。
/// 让渡在事件线程上睡眠，与剪贴板兜底的阻塞语义同款（节奏表总睡眠
/// 250ms，远小于兜底路径的最坏两轮确认窗口）。
const EMPTY_SETTLE_SCHEDULE_MS: [u64; 3] = [25, 75, 150];

/// 激活补读之间的等待时长：注入开关后宿主异步建无障碍树（数百毫秒起），
/// 补读之间让宿主建一会儿。等待在事件线程上睡眠，与让渡的阻塞语义同款。
const ACTIVATION_SETTLE_DELAY: Duration = Duration::from_millis(250);

/// 激活补读次数上限：最多 5 次 × 250ms ≈ 1.25s 建树等待（实际受
/// [`AX_WAIT_BUDGET`] 的 700ms 整体截断，故最坏睡眠约 0.7s；各次 AX 往返
/// 耗时另计，宿主不响应时受 AX 消息超时兜底）。只发生在「激活注入首次
/// 成功」的那一个手势，换的是此后所有手势免按键注入、免剪贴板副作用。
const ACTIVATION_SETTLE_BUDGET: usize = 5;

/// AX 等待段的总预算（墙钟，自进入读取起算）：让渡与激活补读的睡眠段
/// 共享，预算耗尽即停睡转进——让渡段转激活闸门、激活段转兜底。兜底
/// （剪贴板注入确认）有自己的确认窗口，不占本预算。
const AX_WAIT_BUDGET: Duration = Duration::from_millis(700);

/// 第 `attempt` 次空读后的让渡时长；节奏表耗尽返回 `None`（纯逻辑，单测覆盖）。
fn settle_delay(attempt: usize) -> Option<Duration> {
    EMPTY_SETTLE_SCHEDULE_MS
        .get(attempt)
        .map(|ms| Duration::from_millis(*ms))
}

/// 第 `attempt` 次激活补读空结果后的等待时长；预算用尽返回 `None`（纯逻辑，单测覆盖）。
fn activation_delay(attempt: usize) -> Option<Duration> {
    (attempt < ACTIVATION_SETTLE_BUDGET).then_some(ACTIVATION_SETTLE_DELAY)
}

/// 组合判定（纯逻辑，单测覆盖）：AX 成功直接采纳；权限缺失原样上抛且不
/// 评估激活与兜底（见模块注释）；空选区在唤醒常客宿主（`responds_to_wake`，
/// 读取入口现读一次）上跳过让渡直接进激活补读，其余按 [`settle_delay`]
/// 让渡后补读，预算耗尽仍空与「其余读不到的情形」一样先过激活闸门
/// （[`wake_then`]），激活落空才落到兜底结果。`wake` 是惰性求值——激活
/// 含跨进程属性写入，未走到就不该有副作用；`fallback` 同理——兜底路径
/// 含按键注入。`delay_for`/`wake_delay_for` 把让渡与激活的节奏参数化，
/// `waits_exhausted` 把等待预算参数化，测试零睡眠直达时序断言。
fn combine(
    mut ax: impl FnMut() -> Result<String, GlossError>,
    responds_to_wake: bool,
    mut wake: impl FnMut() -> bool,
    fallback: impl FnOnce() -> Result<String, GlossError>,
    delay_for: impl Fn(usize) -> Option<Duration>,
    wake_delay_for: impl Fn(usize) -> Option<Duration>,
    waits_exhausted: impl Fn() -> bool,
) -> Result<String, GlossError> {
    let mut attempts = 0;
    loop {
        match ax() {
            Ok(text) => return Ok(text),
            Err(err @ GlossError::AccessibilityDenied) => return Err(err),
            Err(GlossError::SelectionEmpty) => {
                if attempts == 0 && responds_to_wake {
                    return wake_then(ax, &mut wake, fallback, wake_delay_for, waits_exhausted);
                }
                let Some(delay) = delay_for(attempts) else {
                    info!(
                        thread = thread::EVENT,
                        "selection still empty after settle budget, trying to wake the accessibility tree"
                    );
                    return wake_then(ax, &mut wake, fallback, wake_delay_for, waits_exhausted);
                };
                if waits_exhausted() {
                    info!(
                        thread = thread::EVENT,
                        "selection wait budget exhausted, trying to wake the accessibility tree"
                    );
                    return wake_then(ax, &mut wake, fallback, wake_delay_for, waits_exhausted);
                }
                attempts += 1;
                std::thread::sleep(delay);
            }
            Err(_) => return wake_then(ax, &mut wake, fallback, wake_delay_for, waits_exhausted),
        }
    }
}

/// 激活闸门（纯逻辑，单测覆盖）：`wake` 报「无需补读」（该宿主已评估
/// 过、不是 Chromium 家族或注入失败）时立即落兜底，不花任何等待；刚
/// 注入成功时按 [`activation_delay`] 的节奏补读，树一回答就采纳，预算
/// 耗尽仍空或读不到再落兜底——补读期间宿主建树是异步的，空结果多半是
/// 还没建好而不是没有。等待预算（`waits_exhausted`）在补读之间生效，
/// 耗尽即落兜底。补读中权限缺失与让渡环同款原样上抛：注入会被系统
/// 静默忽略，兜底只会白等。
fn wake_then(
    mut ax: impl FnMut() -> Result<String, GlossError>,
    wake: &mut impl FnMut() -> bool,
    fallback: impl FnOnce() -> Result<String, GlossError>,
    wake_delay_for: impl Fn(usize) -> Option<Duration>,
    waits_exhausted: impl Fn() -> bool,
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
                if waits_exhausted() {
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

/// 取材边界的归一化（纯逻辑，单测覆盖）：读到的文本修剪两端空白后放行，
/// 判空用修剪后的值——纯空白选区（误滑）按既有 [`GlossError::SelectionEmpty`]
/// 语义上抛。AX 与剪贴板兜底两条通道的产物都经此收口，让渡补读与兜底
/// 注入只服务「读不到」，不为已知纯空白的选区再花时序预算。
///
/// 两端本无空白时原缓冲原样归还，不复制整段。
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

/// 一次划词取材的产物：文本语义与兜底轮收尾分离。产物先行（`outcome`
/// 上抛即触发弹窗链路），收尾随后——兜底轮的恢复写盘与写盘归因不挡在
/// 弹窗前。
pub struct SelectionRead {
    /// 取材结果：文本或失败语义。
    pub outcome: Result<String, GlossError>,
    /// 兜底轮收尾；AX 通道直读为 `None`。
    settle: Option<FallbackSettle>,
}

impl SelectionRead {
    /// 拆出产物与收尾：产物交通道上抛（弹窗链路启动），收尾由调用方在
    /// 产物上抛之后调 [`SelectionSettle::finish`] 收口。
    pub fn into_parts(self) -> (Result<String, GlossError>, SelectionSettle) {
        (self.outcome, SelectionSettle(self.settle))
    }
}

/// 兜底轮收尾的移交形态：[`SelectionRead`] 的产物被通道消费后，收尾以
/// 本类型独立存活到调用方收口。
pub struct SelectionSettle(Option<FallbackSettle>);

impl SelectionSettle {
    /// 收口兜底轮：按读取时定下的决策恢复（或跳过）原剪贴板并置位写盘
    /// 归因。在取材产物上抛之后、与读取同一事件线程上调用。
    pub fn finish(self) {
        if let Some(settle) = self.0 {
            settle.finish();
        }
    }
}

mod imp {
    use std::time::Instant;

    use super::{
        AX_WAIT_BUDGET, SelectionRead, activation_delay, combine, normalize, settle_delay,
    };
    use crate::clipboard::WatchRoundFlag;
    use crate::selection::accessibility::AccessibilityReader;
    use crate::selection::activation::AxActivator;
    use crate::selection::clipboard::ClipboardFallbackReader;

    /// 选区读取的组合实现：AX 优先，读不到时先尝试激活宿主的无障碍树，
    /// 仍拿不到再降级剪贴板兜底。激活器随读取器跨手势复用（会话内按
    /// pid 去重并记忆唤醒常客）；取材轮次闩锁句柄传给兜底通道——写盘
    /// 轮次由它置位，哨兵按计数归因消费。
    #[derive(Debug)]
    pub struct CompositeReader {
        ax: AccessibilityReader,
        clipboard: ClipboardFallbackReader,
        activator: AxActivator,
    }

    impl CompositeReader {
        /// 创建组合读取器：传入与哨兵源共享的取材轮次闩锁句柄。
        pub fn new(round: WatchRoundFlag) -> Self {
            Self {
                ax: AccessibilityReader,
                clipboard: ClipboardFallbackReader::new(round),
                activator: AxActivator::default(),
            }
        }

        /// 读取前台应用的选中文本。
        ///
        /// 产物与收尾分离：[`SelectionRead::outcome`] 先行上抛（弹窗链路
        /// 启动），兜底轮的恢复写盘经 [`SelectionSettle::finish`] 随后执行
        /// ——恢复不挡在产物交付前。产物已修剪两端空白，纯空白选区按
        /// [`GlossError::SelectionEmpty`] 报告。调用方保证：在平台事件
        /// 线程上调用（收尾同线程）。
        pub fn read(&mut self) -> SelectionRead {
            let started = Instant::now();
            let CompositeReader {
                ax,
                clipboard,
                activator,
            } = self;
            // 唤醒常客判定在入口现读一次：常客与否随前台应用，一次读取
            // 期间不变；激活器的可变借用留给 wake 闭包。
            let responds_to_wake = activator.responds_to_wake();
            let mut settle = None;
            let outcome = combine(
                || ax.read(),
                responds_to_wake,
                || activator.wake(),
                || {
                    let (outcome, round) = clipboard.read();
                    settle = Some(round);
                    outcome
                },
                settle_delay,
                activation_delay,
                || started.elapsed() >= AX_WAIT_BUDGET,
            )
            .and_then(normalize);
            SelectionRead { outcome, settle }
        }
    }
}

pub use imp::CompositeReader;

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gloss_core::model::GlossError;

    use super::{
        ACTIVATION_SETTLE_BUDGET, ACTIVATION_SETTLE_DELAY, EMPTY_SETTLE_SCHEDULE_MS,
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

    fn budget_never_exhausted() -> bool {
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
            false,
            never_wake,
            || {
                clipboard_called = true;
                Ok::<String, GlossError>("from clipboard".into())
            },
            no_delay,
            no_delay,
            budget_never_exhausted,
        );
        assert_eq!(outcome, Ok("from ax".into()));
        assert!(!clipboard_called, "ax success must not touch the clipboard");
    }

    #[test]
    fn permission_denied_skips_fallback() {
        let mut clipboard_called = false;
        let outcome = combine(
            || Err::<String, GlossError>(GlossError::AccessibilityDenied),
            false,
            never_wake,
            || {
                clipboard_called = true;
                Ok::<String, GlossError>("from clipboard".into())
            },
            no_delay,
            no_delay,
            budget_never_exhausted,
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
            false,
            never_wake,
            || {
                fallback_calls += 1;
                Ok::<String, GlossError>("from clipboard".into())
            },
            no_delay,
            no_delay,
            budget_never_exhausted,
        );
        assert_eq!(outcome, Ok("from clipboard".into()));
        assert_eq!(
            reads,
            EMPTY_SETTLE_SCHEDULE_MS.len() + 1,
            "the AX read is retried through the whole settle schedule first"
        );
        assert_eq!(
            fallback_calls, 1,
            "the fallback runs once, only after the schedule is exhausted"
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
            false,
            never_wake,
            || panic!("a settled selection must not fall back to the clipboard"),
            no_delay,
            no_delay,
            budget_never_exhausted,
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
                false,
                || {
                    wake_calls += 1;
                    false
                },
                || clipboard_ok("from clipboard"),
                no_delay,
                no_delay,
                budget_never_exhausted,
            ),
            Ok("from clipboard".into())
        );
        assert_eq!(wake_calls, 1, "the activator is consulted once per give-up");
        assert_eq!(
            combine(
                ax,
                false,
                never_wake,
                || Err(GlossError::SelectionUnavailable),
                no_delay,
                no_delay,
                budget_never_exhausted,
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
            false,
            &mut wake,
            || Ok::<String, GlossError>("from clipboard".into()),
            no_delay,
            no_delay,
            budget_never_exhausted,
        );
        assert_eq!(outcome, Ok("from ax".into()));
        let outcome = combine(
            || Err::<String, GlossError>(GlossError::AccessibilityDenied),
            false,
            &mut wake,
            || Ok::<String, GlossError>("from clipboard".into()),
            no_delay,
            no_delay,
            budget_never_exhausted,
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
            false,
            never_wake,
            || Ok::<String, GlossError>("from clipboard".into()),
            no_delay,
            no_delay,
            budget_never_exhausted,
        );
        assert_eq!(outcome, Ok("from clipboard".into()));
        assert_eq!(
            reads,
            EMPTY_SETTLE_SCHEDULE_MS.len() + 1,
            "the settle schedule runs before the activator is consulted"
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
            false,
            || true,
            || panic!("a freshly woken tree must not fall back while it may still answer"),
            no_delay,
            no_wake_delay,
            budget_never_exhausted,
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
            false,
            || true,
            || Ok::<String, GlossError>("from clipboard".into()),
            no_delay,
            no_wake_delay,
            budget_never_exhausted,
        );
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
        let outcome = combine(
            || {
                reads += 1;
                Err::<String, GlossError>(GlossError::SelectionUnavailable)
            },
            false,
            || true,
            || Ok::<String, GlossError>("from clipboard".into()),
            no_delay,
            no_wake_delay,
            budget_never_exhausted,
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
                if reads <= EMPTY_SETTLE_SCHEDULE_MS.len() + 1 {
                    Err(GlossError::SelectionEmpty)
                } else {
                    Err(GlossError::AccessibilityDenied)
                }
            },
            false,
            || true,
            || panic!("a permission loss during activation must not fall back to the clipboard"),
            no_delay,
            no_wake_delay,
            budget_never_exhausted,
        );
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
        let outcome = combine(
            || {
                reads += 1;
                if reads == 1 {
                    Err(GlossError::SelectionEmpty)
                } else {
                    Ok("rescued".to_owned())
                }
            },
            true,
            || true,
            || panic!("a woken tree that answers must not reach the clipboard"),
            no_delay,
            no_wake_delay,
            budget_never_exhausted,
        );
        assert_eq!(outcome, Ok("rescued".into()));
        assert_eq!(
            reads, 2,
            "the first empty read jumps straight to the activation re-read"
        );
    }

    #[test]
    fn the_wait_budget_ends_the_settling_early() {
        let mut reads = 0;
        let outcome = combine(
            || {
                reads += 1;
                if reads == 1 {
                    Err(GlossError::SelectionEmpty)
                } else {
                    Ok("woken".to_owned())
                }
            },
            false,
            || true,
            || panic!("the budget cut goes to the wake gate, not the clipboard"),
            no_delay,
            no_wake_delay,
            || true,
        );
        assert_eq!(outcome, Ok("woken".into()));
        assert_eq!(
            reads, 2,
            "the schedule is cut at the first wait, straight to the wake gate"
        );
    }

    #[test]
    fn the_wait_budget_ends_the_activation_re_reads() {
        let mut reads = 0;
        let outcome = combine(
            || {
                reads += 1;
                Err::<String, GlossError>(GlossError::SelectionEmpty)
            },
            false,
            || true,
            || Ok("from clipboard".to_owned()),
            no_delay,
            no_wake_delay,
            || true,
        );
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
    fn settle_budget_is_bounded_and_positive() {
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
            "the schedule exhausts into the activation gate"
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
}
