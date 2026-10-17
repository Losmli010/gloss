//! 代码语言的采纳、归一化与语法着色（demo 同款架构：单趟正则分组捕获）。
//!
//! 按职责拆两个子模块：
//! - `tokenize`——别名归一化、按语言组装规则集正则（惰性编译一次）
//!   并单趟扫描出各类别的字节区间；
//! - `palette`——六类着色类别（`Class`）的 demo 调色板与斜体规则。
//!
//! 规则集由共享词法积木组装——行/块注释、字符串（含转义与三引号）、
//! 数字（含十六进制/浮点）、大写驼峰类型、函数形——每语言只补关键字表；
//! JSON/YAML/TOML/SQL/HTML/XML/CSS 出特化规则（键/标签/选择器）。未知
//! 语言走通用启发集（字符串/两类注释/数字/类型/函数形仍着色），产物未
//! 给出语言则无角标。
//!
//! 语言唯一来源是任务产物的 `structured.code_language`（LLM 判定），
//! 经 `adopt` 渲染时现算（采纳门槛 + 归一化收敛于此，纯函数）：门槛
//! 拦下散文与示例占位串（模型照抄「…或 null」的判定不可上角标），归一
//! 化把别名收拢到规范名（rs→rust、py→python…），角标与规则集都只认规范
//! 名。「模型说的」与「UI 用的」因此永不漂移——视图不缓存派生状态。
//! 本地不做内容探测——流式期产物未到，代码面板按通用集着色、无角标。

mod palette;
mod tokenize;

pub(crate) use tokenize::{normalize_language, tokenize};

/// 采纳产物的代码语言判定（渲染时现算）：修剪首尾空白后过词元门槛——
/// 语言标识应是单个 ASCII 词元（"rust"、"c++"、"objective-c" 这个形状，
/// 内部含空白或非 ASCII 字符的判定是散文或示例占位串）——再经归一化
/// （别名/大小写）落规范名。判定缺失或不可采纳时为 `None`：无角标、按
/// 通用启发集着色，自由文本不上角标。
pub(crate) fn adopt(verdict: Option<&str>) -> Option<String> {
    verdict
        .map(str::trim)
        .filter(|verdict| {
            !verdict.is_empty()
                && verdict
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '#' | '-' | '.'))
        })
        .and_then(normalize_language)
}

#[cfg(test)]
mod tests {
    use super::adopt;

    #[test]
    fn adopt_keeps_plausible_verdicts_and_normalizes_aliases() {
        assert_eq!(adopt(Some("rust")), Some("rust".to_owned()));
        assert_eq!(adopt(Some("Rust")), Some("rust".to_owned()));
        assert_eq!(adopt(Some("py")), Some("python".to_owned()));
        assert_eq!(adopt(Some("objective-c")), Some("objc".to_owned()));
        assert_eq!(adopt(Some("c++")), Some("cpp".to_owned()));
    }

    #[test]
    fn adopt_rejects_prose_placeholders_and_missing_verdicts() {
        assert_eq!(adopt(None), None, "no verdict, no language");
        assert_eq!(adopt(Some("")), None);
        assert_eq!(
            adopt(Some("   ")),
            None,
            "a blank verdict is no verdict after trim"
        );
        assert_eq!(
            adopt(Some("…或 null")),
            None,
            "the placeholder string the model may echo is not a language"
        );
        assert_eq!(
            adopt(Some("可能是 rust")),
            None,
            "prose never becomes a badge"
        );
    }

    #[test]
    fn adopt_trims_surrounding_whitespace_before_the_gate() {
        assert_eq!(
            adopt(Some(" python\n")),
            Some("python".to_owned()),
            "padded verdicts are trimmed, not rejected"
        );
    }
}
