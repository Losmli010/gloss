//! 选区读取的组合通道：用户界面自动化（UIA）优先，读不到时降级剪贴板兜底。
//!
//! 降级边界：文本模式（TextPattern）缺席（自绘应用不支持）与调用失败都
//! 是“读不到”，直接落剪贴板兜底；系统（Windows）没有需要唤醒的无障碍树，
//! 无宿主激活闸门——文本模式缺席时等再久也读不到，注入才是解法。
//!
//! 时延边界：让渡的睡眠段共享一份墙钟预算（读等待预算，自进入读取起算），
//! 预算耗尽即停睡转进——把无响应宿主上的等待钉在预算内；读取调用本身
//! 受组件对象模型（COM）跨进程调用的耗时约束，预算只收窄睡眠段。
//!
//! 归一化边界：用户界面自动化（UIA）与剪贴板兜底两条通道的产物都在入口
//! （组合读取器的读取方法）修剪两端空白，判空用修剪后的值——纯空白选区
//! （误滑）按既有空选区语义上抛；下游全链路（内容闸门、原文视图、提示词、
//! 缓存键）只见同一份已修剪文本。
//!
//! 产物与收尾分离：兜底轮的恢复写盘与写盘归因随读取结果拆分移交恢复收尾
//! 结构，调用方在取材产物上抛（弹窗链路启动）之后收口——恢复不挡在弹窗前。

use std::time::Duration;

use gloss_core::log::{info, thread};
use gloss_core::model::GlossError;

const EMPTY_SETTLE_SCHEDULE_MS: [u64; 3] = [25, 75, 150];

const UIA_WAIT_BUDGET: Duration = Duration::from_millis(700);

fn settle_delay(attempt: usize) -> Option<Duration> {
    EMPTY_SETTLE_SCHEDULE_MS
        .get(attempt)
        .map(|ms| Duration::from_millis(*ms))
}

struct Pacing<S, E> {
    settle: S,
    waits_exhausted: E,
}

impl<S, E> Pacing<S, E>
where
    S: Fn(usize) -> Option<Duration>,
    E: Fn() -> bool,
{
    fn new(settle: S, waits_exhausted: E) -> Self {
        Self {
            settle,
            waits_exhausted,
        }
    }
}

fn combine(
    mut main: impl FnMut() -> Result<String, GlossError>,
    fallback: impl FnOnce() -> Result<String, GlossError>,
    pacing: Pacing<impl Fn(usize) -> Option<Duration>, impl Fn() -> bool>,
) -> Result<String, GlossError> {
    let mut attempts = 0;
    loop {
        match main() {
            Ok(text) => return Ok(text),
            Err(GlossError::SelectionEmpty) => {
                let Some(delay) = (pacing.settle)(attempts) else {
                    info!(
                        thread = thread::EVENT,
                        "selection still empty after settle budget, falling back to the clipboard"
                    );
                    return fallback();
                };
                if (pacing.waits_exhausted)() {
                    info!(
                        thread = thread::EVENT,
                        "selection wait budget exhausted, falling back to the clipboard"
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

mod imp {
    use std::time::Instant;

    use gloss_core::ports::{TextAcquirer, TextRead, TextSettle};

    use super::{Pacing, UIA_WAIT_BUDGET, combine, normalize, settle_delay};
    use crate::clipboard::WatchRoundFlag;
    use crate::selection::clipboard::ClipboardFallbackReader;
    use crate::selection::uia::UiaReader;

    pub struct CompositeReader {
        uia: UiaReader,
        clipboard: ClipboardFallbackReader,
    }

    impl CompositeReader {
        pub fn new(round: WatchRoundFlag) -> Self {
            Self {
                uia: UiaReader::new(),
                clipboard: ClipboardFallbackReader::new(round),
            }
        }
    }

    impl TextAcquirer for CompositeReader {
        fn read(&mut self) -> TextRead {
            let started = Instant::now();
            let CompositeReader { uia, clipboard } = self;
            let mut settle = None;
            let outcome = combine(
                || uia.read(),
                || {
                    let (outcome, round) = clipboard.read();
                    settle = Some(round);
                    outcome
                },
                Pacing::new(settle_delay, || started.elapsed() >= UIA_WAIT_BUDGET),
            )
            .and_then(normalize);
            TextRead::new(
                outcome,
                settle.map_or_else(TextSettle::idle, |settle| {
                    TextSettle::deferred(move || settle.finish())
                }),
            )
        }
    }
}

pub use imp::CompositeReader;

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gloss_core::model::GlossError;

    use super::{EMPTY_SETTLE_SCHEDULE_MS, Pacing, combine, normalize, settle_delay};

    fn uia_ok(text: &str) -> impl FnMut() -> Result<String, GlossError> {
        let text = text.to_owned();
        move || Ok(text.clone())
    }

    fn clipboard_ok(text: &str) -> Result<String, GlossError> {
        Ok(text.to_owned())
    }

    fn budget_never_exhausted() -> bool {
        false
    }

    fn no_delay(attempt: usize) -> Option<Duration> {
        settle_delay(attempt).map(|_| Duration::ZERO)
    }

    #[test]
    fn combined_fallback_is_lazy_on_uia_success() {
        let mut clipboard_called = false;
        let outcome = combine(
            uia_ok("from uia"),
            || {
                clipboard_called = true;
                Ok::<String, GlossError>("from clipboard".into())
            },
            Pacing::new(no_delay, budget_never_exhausted),
        );
        assert_eq!(outcome, Ok("from uia".into()));
        assert!(
            !clipboard_called,
            "uia success must not touch the clipboard"
        );
    }

    #[test]
    fn combined_empty_selection_settles_then_falls_back_to_the_clipboard() {
        let mut reads = 0;
        let mut fallback_calls = 0;
        let outcome = combine(
            || {
                reads += 1;
                Err::<String, GlossError>(GlossError::SelectionEmpty)
            },
            || {
                fallback_calls += 1;
                Ok::<String, GlossError>("from clipboard".into())
            },
            Pacing::new(no_delay, budget_never_exhausted),
        );
        assert_eq!(outcome, Ok("from clipboard".into()));
        assert_eq!(
            reads,
            EMPTY_SETTLE_SCHEDULE_MS.len() + 1,
            "the uia read is retried through the whole settle schedule first"
        );
        assert_eq!(
            fallback_calls, 1,
            "the fallback runs once, only after the schedule is exhausted"
        );
    }

    #[test]
    fn combined_empty_then_ready_read_adopts_the_late_selection() {
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
            Pacing::new(no_delay, budget_never_exhausted),
        );
        assert_eq!(outcome, Ok("the selection showed up".to_owned()));
        assert_eq!(reads, 2);
    }

    #[test]
    fn combined_unavailable_uia_falls_back_to_clipboard() {
        let outcome = combine(
            || Err::<String, GlossError>(GlossError::SelectionUnavailable),
            || clipboard_ok("from clipboard"),
            Pacing::new(no_delay, budget_never_exhausted),
        );
        assert_eq!(outcome, Ok("from clipboard".into()));
        assert_eq!(
            combine(
                || Err::<String, GlossError>(GlossError::SelectionUnavailable),
                || Err(GlossError::SelectionUnavailable),
                Pacing::new(no_delay, budget_never_exhausted),
            ),
            Err(GlossError::SelectionUnavailable)
        );
    }

    #[test]
    fn combined_the_wait_budget_ends_the_settling_early() {
        let mut reads = 0;
        let outcome = combine(
            || {
                reads += 1;
                Err::<String, GlossError>(GlossError::SelectionEmpty)
            },
            || Ok("from clipboard".to_owned()),
            Pacing::new(no_delay, || true),
        );
        assert_eq!(outcome, Ok("from clipboard".into()));
        assert_eq!(
            reads, 2,
            "one initial read plus one settle attempt before the budget cut"
        );
    }

    #[test]
    fn combined_settle_schedule_is_bounded_and_positive() {
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
            "the schedule exhausts into the fallback"
        );
        assert_eq!(settle_delay(usize::MAX), None);
        assert!(
            EMPTY_SETTLE_SCHEDULE_MS.iter().all(|ms| *ms > 0),
            "every settle wait is positive"
        );
    }

    #[test]
    fn combined_acquired_text_is_trimmed_at_the_boundary() {
        assert_eq!(
            normalize("  hello world \n\t".to_owned()),
            Ok("hello world".to_owned())
        );
        assert_eq!(normalize("hello".to_owned()), Ok("hello".to_owned()));
    }

    #[test]
    fn combined_acquired_text_keeps_interior_whitespace_verbatim() {
        assert_eq!(
            normalize("  a\n b\tc ".to_owned()),
            Ok("a\n b\tc".to_owned())
        );
    }

    #[test]
    fn combined_pure_whitespace_selection_maps_to_selection_empty() {
        for raw in ["   ", "\n\t \n", "\u{3000}", ""] {
            assert_eq!(
                normalize(raw.to_owned()),
                Err(GlossError::SelectionEmpty),
                "a whitespace-only selection must take the existing empty-selection semantics"
            );
        }
    }
}
