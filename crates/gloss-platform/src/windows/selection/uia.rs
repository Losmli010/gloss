//! 用户界面自动化（UIA）读选区：焦点元素的文本模式（TextPattern）读取器。
//!
//! 经组件对象模型（COM）向系统问询“焦点元素 → 选中文本”，是向目标应用
//! 发起的同步跨进程调用，按线程约束必须运行在平台事件线程（调用方保证
//! 亲和性，套间在事件线程启动序初始化）。读取器不持有任何系统状态：自动
//! 化对象在每次读取的调用内生灭（对象与接口指针都是套间亲和的，持有即
//! 跨线程——端口的发送约束与套间亲和不可两立），构造因此零系统调用、
//! 取材通道整体可安全跨线程移交。系统（Windows）无系统级授权门，权限
//! 拒绝语义不存在：应用不支持文本模式（自绘界面）、自动化对象创建失败或
//! 调用失败返回读取不可用错误——组合通道落剪贴板兜底；选区为空串或无
//! 选中范围返回空选区错误——组合通道先让渡补读。一律以错误传播，不允许
//! 程序崩溃逃出。
//!
//! 策略取最简一条路径：只读文本模式的选中范围。

use gloss_core::log::{debug, thread};
use gloss_core::model::GlossError;

use crate::common::image::MAX_SELECTION_BYTES;
use crate::ffi::uia;

fn interpret(outcome: Result<Option<String>, i32>) -> Result<String, GlossError> {
    match outcome {
        Ok(Some(text)) if !text.is_empty() => Ok(text),
        Ok(Some(_)) => Err(GlossError::SelectionEmpty),
        Ok(None) => Err(GlossError::SelectionUnavailable),
        Err(_) => Err(GlossError::SelectionUnavailable),
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct UiaReader;

impl UiaReader {
    pub fn new() -> Self {
        Self
    }

    pub fn read(&mut self) -> Result<String, GlossError> {
        let outcome = uia::selected_text(MAX_SELECTION_BYTES);
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
        for code in [-2, -2147220990, -2147023174, 0] {
            assert_eq!(
                interpret(Err(code)),
                Err(GlossError::SelectionUnavailable),
                "code {code} should map to selection unavailable"
            );
        }
    }
}
