//! 流式注文的渐进提取：对累积的原始流做**转义感知**的 `note` 键扫描。
//!
//! 纯决策函数，不依赖 egui 绘制侧；「未到齐」一律落进度态（空串），由
//! 下一帧整段重扫补齐。

/// 流式注文的可见部分：对累积的原始流做**转义感知**的 `note`（义）渐进
/// 提取，返回已到达内容的反转义前缀。现行输出契约是纯 JSON 对象，`note`
/// 的位置随 kind 而定（词卡的 phonetic 在它前面），因此扫描器逐对跳过
/// 先到的完整键值——`note` 键或值未到齐时返回空串，正文区落骨架，页脚
/// 保留「正在注解」进度态。值到齐后按 JSON 字符串转义规则逐段反转义；
/// 残缺的转义序列（尾部孤反斜杠、不足四位的 `\uXXXX`）本帧丢弃、下一帧
/// 补齐，UTF-16 代理对在流式期暂缺（完成态以 `outcome.note` 为权威源）。
/// 旧契约（markdown + 围栏）不含 `note` 键，全程进度态，由 finalize 的
/// 围栏 fallback 在完成态兜住。
pub(super) fn stream_note(raw: &str) -> String {
    let mut cursor = Cursor {
        rest: raw.trim_start(),
    };
    if !cursor.strip("{") {
        return String::new();
    }
    loop {
        // 键：字符串字面量；未闭合则整路进度态。
        let Some(key) = cursor.json_string() else {
            return String::new();
        };
        cursor.skip_ws();
        if !cursor.strip(":") {
            return String::new();
        }
        cursor.skip_ws();
        if key == "note" {
            return cursor.json_string().unwrap_or_default();
        }
        // 其余键：跳过完整值；值未写完则进度态。
        if !cursor.skip_value() {
            return String::new();
        }
        cursor.skip_ws();
        if !cursor.strip(",") {
            return String::new();
        }
    }
}

/// JSON 前缀扫描游标：`rest` 恒为未消费部分；所有「未到齐」情形都消费
/// 尽量少并让调用方落进度态（下一帧整段重扫，流式帧几十 KB 上界、无
/// 分配，无需增量缓存）。
struct Cursor<'a> {
    rest: &'a str,
}

impl Cursor<'_> {
    /// 剥掉前缀；不匹配则原样保留并返回 `false`。
    fn strip(&mut self, prefix: &str) -> bool {
        let Some(rest) = self.rest.strip_prefix(prefix) else {
            return false;
        };
        self.rest = rest;
        true
    }

    fn skip_ws(&mut self) {
        self.rest = self.rest.trim_start();
    }

    /// 提取一个 JSON 字符串的反转义内容（调用方已确认 `"` 起头；未闭合
    /// 返回 `None`，残缺转义就地截断、下一帧补齐）。
    fn json_string(&mut self) -> Option<String> {
        self.skip_ws();
        if !self.strip("\"") {
            return None;
        }
        let mut out = String::new();
        let mut chars = self.rest.chars();
        loop {
            // 输入耗尽＝值未写完：已到达的部分照样上屏（下一帧整段重扫）。
            let Some(ch) = chars.next() else {
                self.rest = "";
                return Some(out);
            };
            match ch {
                '"' => {
                    self.rest = chars.as_str();
                    return Some(out);
                }
                '\\' => {
                    // 残缺转义（尾部孤反斜杠/不足四位的 \uXXXX）就地截断、
                    // 返回已到达部分——下一帧整段重扫后补齐。
                    let Some(escaped) = chars.next() else {
                        self.rest = "";
                        return Some(out);
                    };
                    match escaped {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        'b' => out.push('\u{0008}'),
                        'f' => out.push('\u{000C}'),
                        'u' => {
                            let mut hex = String::new();
                            for _ in 0..4 {
                                let Some(digit) = chars.next() else {
                                    self.rest = "";
                                    return Some(out);
                                };
                                hex.push(digit);
                            }
                            if let Ok(code) = u32::from_str_radix(&hex, 16)
                                && let Some(decoded) = char::from_u32(code)
                            {
                                out.push(decoded);
                            }
                        }
                        other => out.push(other),
                    }
                }
                other => out.push(other),
            }
        }
    }

    /// 跳过一个完整 JSON 值（字符串/数字/true/false/null/数组/对象）；
    /// 值未写完返回 `false`（进度态）。
    fn skip_value(&mut self) -> bool {
        self.skip_ws();
        let Some(first) = self.rest.chars().next() else {
            return false;
        };
        match first {
            '"' => self.json_string().is_some(),
            '{' | '[' => {
                let close = if first == '{' { '}' } else { ']' };
                if !self.strip(first.encode_utf8(&mut [0; 4])) {
                    return false;
                }
                loop {
                    self.skip_ws();
                    let Some(next) = self.rest.chars().next() else {
                        return false;
                    };
                    if next == '"' {
                        if self.json_string().is_none() {
                            return false;
                        }
                    } else if next == '{' || next == '[' {
                        if !self.skip_value() {
                            return false;
                        }
                    } else if next == close {
                        self.rest = &self.rest[next.len_utf8()..];
                        return true;
                    } else {
                        self.rest = &self.rest[next.len_utf8()..];
                    }
                }
            }
            // 数字与字面量（true/false/null）：读到结构性边界。
            _ => match self.rest.find([',', '}', ']']) {
                Some(end) => {
                    self.rest = &self.rest[end..];
                    true
                }
                None => false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::stream_note;

    #[test]
    fn stream_note_extracts_the_json_field_progressively() {
        assert_eq!(
            stream_note(r#"{"note":"正文一\n正文二","title":"x"}"#),
            "正文一\n正文二",
            "escapes decode and the value stops at the closing quote"
        );
        assert_eq!(stream_note(""), "");
        assert_eq!(
            stream_note("plain markdown"),
            "",
            "non-JSON stays in progress"
        );
        assert_eq!(stream_note("{"), "");
        assert_eq!(stream_note(r#"{"bod"#), "", "a partial key keeps waiting");
        assert_eq!(
            stream_note(r#"{"note""#),
            "",
            "key without colon keeps waiting"
        );
        assert_eq!(
            stream_note(r#"{"note":"#),
            "",
            "colon without value keeps waiting"
        );
        assert_eq!(
            stream_note(r#"{"note":""#),
            "",
            "an open value shows nothing yet"
        );
        assert_eq!(
            stream_note(r#"{"note":"未闭合"#),
            "未闭合",
            "an unterminated value still shows what arrived"
        );
        assert_eq!(
            stream_note(r#"{"phonetic":"/ɡlɒs/","note":"义释"}"#),
            "义释",
            "a preceding complete foreign key (word card's phonetic) is skipped"
        );
        assert_eq!(
            stream_note(r#"{"phonetic":"/ɡ"#),
            "",
            "a foreign string value that is still streaming keeps progress"
        );
        assert_eq!(
            stream_note(r#"{"examples":["一","二"],"note":"义"}"#),
            "义",
            "an array-valued foreign key is skipped whole"
        );
        assert_eq!(
            stream_note(r#"{"note":"esc\"ape\\path"}"#),
            "esc\"ape\\path",
            "quote and backslash escapes decode"
        );
        assert_eq!(
            stream_note(r#"{"note":"你\u4f60好"}"#),
            "你你好",
            "a complete unicode escape decodes"
        );
        assert_eq!(
            stream_note(r#"{"note":"你\u4"#),
            "你",
            "a partial unicode escape waits for the next frame"
        );
        assert_eq!(
            stream_note(r#"{"note":"尾\"#),
            "尾",
            "a dangling backslash is dropped until it completes"
        );
        assert_eq!(
            stream_note("正文\n```gloss\n{\"title\":\"x\"}\n```"),
            "",
            "the legacy fence contract has no note key: progress state"
        );
    }
}
