//! 选区读取的组合通道：AX 优先，读不到时降级剪贴板兜底。
//!
//! 降级边界：AX 报权限缺失时不兜底——兜底依赖按键注入，未授权时注入会被
//! 系统静默忽略，白等超时只会拖慢失败路径；此时把权限语义原样上抛，由
//! 上层做权限引导。AX 报「选区为空」先短暂让渡补读（见 [`settle_delay`]），
//! 仍为空才不兜底上抛——那是「没选东西」的如实回答，注入 Cmd+C 只会把
//! 剪贴板里的陈旧内容当成「这次划词的选区」发出去。

use std::time::Duration;

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

/// 第 `attempt` 次空读后的让渡时长；预算用尽返回 `None`（纯逻辑，单测覆盖）。
fn settle_delay(attempt: usize) -> Option<Duration> {
    (attempt < EMPTY_SETTLE_BUDGET).then_some(EMPTY_SETTLE_DELAY)
}

/// 组合判定（纯逻辑，单测覆盖）：AX 成功直接采纳；权限缺失原样上抛且不
/// 评估兜底（见模块注释）；空选区按 [`settle_delay`] 让渡后补读，仍空才
/// 上抛；其余读不到的情形才落到兜底结果。`fallback` 是惰性求值——兜底
/// 路径含按键注入，未走到就不该有副作用。
/// `delay_for` 把让渡策略参数化，测试零睡眠直达时序断言。
fn combine(
    mut ax: impl FnMut() -> Result<String, GlossError>,
    fallback: impl FnOnce() -> Result<String, GlossError>,
    delay_for: impl Fn(usize) -> Option<Duration>,
) -> Result<String, GlossError> {
    let mut attempts = 0;
    loop {
        match ax() {
            Ok(text) => return Ok(text),
            Err(err @ GlossError::AccessibilityDenied) => return Err(err),
            Err(GlossError::SelectionEmpty) => {
                let Some(delay) = delay_for(attempts) else {
                    return Err(GlossError::SelectionEmpty);
                };
                attempts += 1;
                std::thread::sleep(delay);
            }
            Err(_) => return fallback(),
        }
    }
}

mod imp {
    use gloss_core::model::GlossError;

    use super::{combine, settle_delay};
    use crate::selection::accessibility::AccessibilityReader;
    use crate::selection::clipboard::ClipboardFallbackReader;

    /// 选区读取的组合实现：AX 优先，读不到时降级剪贴板兜底。
    #[derive(Debug, Default)]
    pub struct CompositeReader {
        ax: AccessibilityReader,
        clipboard: ClipboardFallbackReader,
    }

    impl CompositeReader {
        /// 创建组合读取器。
        pub fn new() -> Self {
            Self::default()
        }

        /// 读取前台应用的选中文本。
        ///
        /// 调用方保证：在平台事件线程上调用。
        pub fn read(&mut self) -> Result<String, GlossError> {
            combine(|| self.ax.read(), || self.clipboard.read(), settle_delay)
        }
    }
}

pub use imp::CompositeReader;

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gloss_core::model::GlossError;

    use super::{EMPTY_SETTLE_BUDGET, EMPTY_SETTLE_DELAY, combine, settle_delay};

    fn ax_ok(text: &str) -> impl FnMut() -> Result<String, GlossError> {
        let text = text.to_owned();
        move || Ok(text.clone())
    }

    fn clipboard_ok(text: &str) -> Result<String, GlossError> {
        Ok(text.to_owned())
    }

    fn no_delay(attempt: usize) -> Option<Duration> {
        settle_delay(attempt).map(|_| Duration::ZERO)
    }

    #[test]
    fn fallback_is_lazy_on_ax_success() {
        let mut clipboard_called = false;
        let outcome = combine(
            ax_ok("from ax"),
            || {
                clipboard_called = true;
                Ok::<String, GlossError>("from clipboard".into())
            },
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
            || {
                clipboard_called = true;
                Ok::<String, GlossError>("from clipboard".into())
            },
            no_delay,
        );
        assert_eq!(outcome, Err(GlossError::AccessibilityDenied));
        assert!(
            !clipboard_called,
            "permission denied must not fall back to the clipboard"
        );
    }

    #[test]
    fn empty_selection_settles_and_retries_before_giving_up() {
        let mut reads = 0;
        let outcome = combine(
            || {
                reads += 1;
                Err::<String, GlossError>(GlossError::SelectionEmpty)
            },
            || panic!("an empty selection must not fall back to the clipboard"),
            no_delay,
        );
        assert_eq!(outcome, Err(GlossError::SelectionEmpty));
        assert_eq!(reads, EMPTY_SETTLE_BUDGET + 1);
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
            || panic!("a settled selection must not fall back to the clipboard"),
            no_delay,
        );
        assert_eq!(outcome, Ok("the selection showed up".to_owned()));
        assert_eq!(reads, 2);
    }

    #[test]
    fn unavailable_ax_falls_back_to_clipboard() {
        let ax = || Err::<String, GlossError>(GlossError::SelectionUnavailable);
        assert_eq!(
            combine(ax, || clipboard_ok("from clipboard"), no_delay),
            Ok("from clipboard".into())
        );
        assert_eq!(
            combine(ax, || Err(GlossError::SelectionUnavailable), no_delay),
            Err(GlossError::SelectionUnavailable)
        );
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
}
