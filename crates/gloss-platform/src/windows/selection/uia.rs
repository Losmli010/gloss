//! 用户界面自动化（UIA）读选区：焦点元素的文本模式（TextPattern）读取器。
//!
//! 经组件对象模型（COM）向系统问询“焦点元素 → 选中文本”，是向目标应用
//! 发起的同步跨进程调用，按线程约束必须运行在平台事件线程（调用方保证
//! 亲和性，套间在事件线程启动序初始化）。读取器随组合通道在组装段构造，
//! 自动化对象因此在首次读取时才创建——构造发生在主线程（组装段），而
//! 套间只在事件线程初始化，事件线程上的首次读取是对象唯一合法的创建点；
//! 创建失败不再重试（后续读取直接按不可用降级兜底），创建成功则复用同一
//! 对象。系统（Windows）无系统级授权门，权限拒绝语义不存在：应用不支持
//! 文本模式（自绘界面）或调用失败返回读取不可用错误——组合通道落剪贴板
//! 兜底；选区为空串或无选中范围返回空选区错误——组合通道先让渡补读。
//! 一律以错误传播，不允许程序崩溃逃出。
//!
//! 策略取最简一条路径：只读文本模式的选中范围。

use gloss_core::log::{debug, thread};
use gloss_core::model::GlossError;
use windows::Win32::UI::Accessibility::IUIAutomation;

use crate::ffi::{com, uia};

const MAX_SELECTION_BYTES: usize = 256 * 1024;

fn settled<T>(slot: &mut Option<Option<T>>, create: impl FnOnce() -> Option<T>) -> Option<&T> {
    if slot.is_none() {
        *slot = Some(create());
    }
    slot.as_ref().and_then(|inner| inner.as_ref())
}

fn interpret(outcome: Result<Option<String>, i32>) -> Result<String, GlossError> {
    match outcome {
        Ok(Some(text)) if !text.is_empty() => Ok(text),
        Ok(Some(_)) => Err(GlossError::SelectionEmpty),
        Ok(None) => Err(GlossError::SelectionUnavailable),
        Err(_) => Err(GlossError::SelectionUnavailable),
    }
}

#[derive(Default)]
pub struct UiaReader {
    automation: Option<Option<IUIAutomation>>,
}

impl UiaReader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read(&mut self) -> Result<String, GlossError> {
        let Some(automation) = settled(&mut self.automation, || {
            let created = com::create_automation();
            match &created {
                Ok(_) => {}
                Err(err) => debug!(
                    thread = thread::EVENT,
                    error = %err,
                    "uia automation object unavailable, selection read degraded"
                ),
            }
            created.ok()
        }) else {
            return Err(GlossError::SelectionUnavailable);
        };
        let outcome = uia::selected_text(automation, MAX_SELECTION_BYTES);
        match &outcome {
            Ok(Some(text)) if !text.is_empty() => debug!(
                thread = thread::EVENT,
                chars = text.chars().count(),
                "selected text acquired"
            ),
            Ok(Some(_)) => debug!(
                thread = thread::EVENT,
                "focused element reports an empty selection"
            ),
            Ok(None) => debug!(
                thread = thread::EVENT,
                "selected text withheld over the size ceiling"
            ),
            Err(code) => debug!(
                thread = thread::EVENT,
                code = *code,
                "uia selection read failed"
            ),
        }
        interpret(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Result<Option<String>, i32> {
        Ok(Some(s.to_string()))
    }

    #[test]
    fn uia_selected_text_passes_through_verbatim() {
        assert_eq!(interpret(text("hello")), Ok("hello".to_string()));
        assert_eq!(interpret(text("  ")), Ok("  ".to_string()));
    }

    #[test]
    fn uia_an_empty_selection_is_empty_but_a_missing_value_stays_unavailable() {
        assert_eq!(
            interpret(Ok(Some(String::new()))),
            Err(GlossError::SelectionEmpty),
            "an empty string is an existing selection reporting no text"
        );
        assert_eq!(
            interpret(Ok(None)),
            Err(GlossError::SelectionUnavailable),
            "withheld text (over the size ceiling) is worth a fallback attempt"
        );
    }

    #[test]
    fn uia_unsupported_patterns_and_failed_calls_stay_unavailable() {
        assert_eq!(
            interpret(Err(-1)),
            Err(GlossError::SelectionUnavailable),
            "the no-text-pattern marker maps to selection unavailable"
        );
        for code in [-2147220990, -2147023174, 0] {
            assert_eq!(
                interpret(Err(code)),
                Err(GlossError::SelectionUnavailable),
                "code {code} should map to selection unavailable"
            );
        }
    }

    #[test]
    fn uia_first_use_creates_the_object_once_and_reuses_it() {
        let mut slot = None;
        let mut creations = 0;
        assert_eq!(
            settled(&mut slot, || {
                creations += 1;
                Some(7u32)
            })
            .copied(),
            Some(7)
        );
        assert_eq!(
            settled(&mut slot, || {
                creations += 1;
                Some(9u32)
            })
            .copied(),
            Some(7),
            "later uses share the created object"
        );
        assert_eq!(creations, 1, "the object is created on the first use only");
    }

    #[test]
    fn uia_a_failed_creation_is_never_retried() {
        let mut slot = None;
        let mut creations = 0;
        assert_eq!(
            settled(&mut slot, || {
                creations += 1;
                None::<u32>
            }),
            None
        );
        assert_eq!(
            settled(&mut slot, || {
                creations += 1;
                Some(3u32)
            }),
            None,
            "a failed creation degrades every later use"
        );
        assert_eq!(creations, 1, "the failure is final, not retried per read");
    }

    #[test]
    fn uia_reader_starts_pending_without_touching_com() {
        let reader = UiaReader::new();
        assert!(
            reader.automation.is_none(),
            "construction must not create the automation object: it would leave the event thread"
        );
    }
}
