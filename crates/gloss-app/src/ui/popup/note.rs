//! 流式正文的渐进提取：对累积的原始流做**转义感知**的顺序双字段扫描。
//!
//! 纯决策函数，不依赖 egui 绘制侧；「未到齐」的部分一律留在进度态，由
//! 下一帧整段重扫补齐。

/// 流式期已到达的正文：注（`note`，反转义前缀）与疏（`structured`
/// 内 `examples` 已到齐条目 + 在写条目的反转义前缀）。
#[derive(Default)]
pub(super) struct StreamFields {
    pub(super) note: String,
    pub(super) examples: Vec<String>,
}

/// 流式正文的可见部分：对累积的原始流做**转义感知**的顺序双字段扫描。
/// 现行输出契约是纯 JSON 对象（见 `gloss_core::schema`），字段次序固定
/// `note` 在前、`structured` 随后，扫描器逐对处理键值：`note` 值闭合后
/// 游标继续推进，遇 `structured` 下钻一层——`examples` 数组逐条渐进出，
/// 描述流完不用等整个响应，疏证条目随流上屏。任一字段「未到齐」即停在
/// 该处（已到达部分照常返回），正文区全空时由调用方落骨架。值到齐后按
/// JSON 字符串转义规则逐段反转义；残缺的转义序列（尾部孤反斜杠、不足
/// 四位的 `\uXXXX`）本帧丢弃、下一帧补齐，UTF-16 代理对在流式期暂缺
/// （完成态以 `outcome` 为权威源）。违约输出（散文、围栏）不含 `note`
/// 键，全程进度态，完成态落降级卡。
pub(super) fn stream_fields(raw: &str) -> StreamFields {
    let mut fields = StreamFields::default();
    let mut cursor = Cursor {
        rest: raw.trim_start(),
    };
    if !cursor.strip("{") {
        return fields;
    }
    scan_fields(&mut cursor, &mut fields, false);
    fields
}

/// 单层对象的键循环：顶层认 `note` 与 `structured`（下钻），`structured`
/// 层认 `examples`；其余键跳过完整值。「未到齐」消费尽量少并停在该处
/// （下一帧整段重扫，流式帧几十 KB 上界、无分配，无需增量缓存）。
fn scan_fields(cursor: &mut Cursor<'_>, fields: &mut StreamFields, nested: bool) {
    loop {
        // 键：字符串字面量；未闭合则停在已提取的字段上。
        let Some((key, key_closed)) = cursor.json_string() else {
            return;
        };
        if !key_closed {
            return;
        }
        cursor.skip_ws();
        if !cursor.strip(":") {
            return;
        }
        cursor.skip_ws();
        match (nested, key.as_str()) {
            (false, "note") => {
                let Some((value, closed)) = cursor.json_string() else {
                    return;
                };
                fields.note = value;
                if !closed {
                    return;
                }
            }
            (false, "structured") => {
                // 嵌套契约：疏证字段在 structured 对象里，下钻一层继续
                // 键循环（它消费自己的闭合括号）；不是对象则按普通值跳过。
                if cursor.strip("{") {
                    scan_fields(cursor, fields, true);
                } else if !cursor.skip_value() {
                    return;
                }
            }
            (true, "examples") => {
                let Some((items, closed)) = cursor.string_array() else {
                    return;
                };
                fields.examples = items;
                if !closed {
                    return;
                }
            }
            _ => {
                // 其余键：跳过完整值；值未写完则停。
                if !cursor.skip_value() {
                    return;
                }
            }
        }
        cursor.skip_ws();
        if cursor.strip("}") {
            return;
        }
        if !cursor.strip(",") {
            return;
        }
    }
}

/// JSON 前缀扫描游标：`rest` 恒为未消费部分；所有「未到齐」情形都消费
/// 尽量少并让调用方停在该处（下一帧整段重扫，流式帧几十 KB 上界、无
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

    /// 提取一个 JSON 字符串的反转义内容（未闭合返回 `closed = false`，
    /// 残缺转义就地截断、下一帧补齐）。
    fn json_string(&mut self) -> Option<(String, bool)> {
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
                return Some((out, false));
            };
            match ch {
                '"' => {
                    self.rest = chars.as_str();
                    return Some((out, true));
                }
                '\\' => {
                    // 残缺转义（尾部孤反斜杠/不足四位的 \uXXXX）就地截断、
                    // 返回已到达部分——下一帧整段重扫后补齐。
                    let Some(escaped) = chars.next() else {
                        self.rest = "";
                        return Some((out, false));
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
                                    return Some((out, false));
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

    /// 提取一个字符串数组（`interpretation` 的流式形态）：已到齐的条目
    /// 全量返回，在写条目带反转义前缀；数组闭合前 `closed = false`。
    /// 元素不是字符串（异常输出）时停在已到达条目上，完成态由 finalize
    /// 解析兜住。
    fn string_array(&mut self) -> Option<(Vec<String>, bool)> {
        self.skip_ws();
        if !self.strip("[") {
            return None;
        }
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            if self.strip("]") {
                return Some((items, true));
            }
            let Some((item, closed)) = self.json_string() else {
                return Some((items, false));
            };
            items.push(item);
            if !closed {
                return Some((items, false));
            }
            self.skip_ws();
            if self.strip(",") {
                continue;
            }
            if self.strip("]") {
                return Some((items, true));
            }
            return Some((items, false));
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
            '"' => matches!(self.json_string(), Some((_, true))),
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
                        if !matches!(self.json_string(), Some((_, true))) {
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
    use super::stream_fields;

    #[test]
    fn stream_fields_extracts_the_json_fields_progressively() {
        let fields = stream_fields(r#"{"note":"正文一\n正文二"}"#);
        assert_eq!(
            fields.note, "正文一\n正文二",
            "escapes decode at the closing quote"
        );
        assert!(fields.examples.is_empty());
    }

    #[test]
    fn stream_fields_descends_into_structured_for_the_examples() {
        let fields =
            stream_fields(r#"{"note":"描述","structured":{"examples":["条目一","条目二"]}}"#);
        assert_eq!(fields.note, "描述");
        assert_eq!(fields.examples, ["条目一", "条目二"]);
    }

    #[test]
    fn stream_fields_shows_the_partial_item_while_the_array_streams() {
        let fields =
            stream_fields(r#"{"note":"描述","structured":{"examples":["条目一","条目二仍"#);
        assert_eq!(fields.note, "描述");
        assert_eq!(
            fields.examples,
            ["条目一", "条目二仍"],
            "the item being written shows its arrived prefix"
        );
    }

    #[test]
    fn stream_fields_note_stays_visible_while_waiting_for_the_array() {
        let fields = stream_fields(r#"{"note":"描述","structured":{"examples":["条目一", "#);
        assert_eq!(fields.note, "描述");
        assert_eq!(fields.examples, ["条目一"]);
    }

    #[test]
    fn stream_fields_keeps_note_only_until_its_value_closes() {
        let fields = stream_fields(r#"{"note":"未闭合"#);
        assert_eq!(fields.note, "未闭合");
        assert!(fields.examples.is_empty());
        assert_eq!(stream_fields(r#"{"note":"#).note, "");
        assert_eq!(stream_fields(r#"{"note":""#).note, "");
    }

    #[test]
    fn stream_fields_tolerates_foreign_keys_around_the_contract() {
        let fields = stream_fields(
            r#"{"kind":"ImageExplain","note":"义","structured":{"examples":["一"]},"tail":42}"#,
        );
        assert_eq!(fields.note, "义");
        assert_eq!(fields.examples, ["一"]);

        let code = stream_fields(
            r#"{"note":"义","structured":{"examples":["例句"],"code_language":"rust"}}"#,
        );
        assert_eq!(code.note, "义");
        assert_eq!(
            code.examples,
            ["例句"],
            "a string-valued sibling key inside structured is skipped"
        );

        let open_structured = stream_fields(r#"{"note":"义","structured":{"phonetic":"/ɡ"#);
        assert_eq!(open_structured.note, "义");
        assert!(open_structured.examples.is_empty());

        let array_foreign = stream_fields(r#"{"examples":["一","二"],"note":"义"}"#);
        assert_eq!(
            array_foreign.note, "义",
            "a top-level array-valued foreign key is skipped whole"
        );

        let word_card = stream_fields(
            r#"{"note":"义释","structured":{"phonetic":"/ɡlɒs/","examples":["例句"]}}"#,
        );
        assert_eq!(word_card.note, "义释");
        assert_eq!(
            word_card.examples,
            ["例句"],
            "the phonetic sibling inside structured is skipped, the examples still land"
        );
    }

    #[test]
    fn stream_fields_decodes_escapes_in_both_fields() {
        let fields =
            stream_fields(r#"{"note":"esc\"ape\\你\u4f60","structured":{"examples":["尾\"#);
        assert_eq!(fields.note, "esc\"ape\\你你");
        assert_eq!(
            fields.examples,
            ["尾"],
            "a dangling backslash is dropped until it completes"
        );
    }

    #[test]
    fn stream_fields_stays_in_progress_off_contract() {
        for raw in [
            "",
            "plain markdown",
            "{",
            r#"{"bod"#,
            r#"{"note""#,
            r#"{"note":"#,
            "正文\n```gloss\n{\"title\":\"x\"}\n```",
        ] {
            let fields = stream_fields(raw);
            assert_eq!(fields.note, "", "{raw:?} must stay in progress");
            assert!(fields.examples.is_empty(), "{raw:?} must stay in progress");
        }
    }

    #[test]
    fn stream_fields_stops_at_the_closing_brace() {
        let fields = stream_fields(r#"{"note":"义","structured":{"examples":["一"]},"tail":"x"}"#);
        assert_eq!(fields.note, "义");
        assert_eq!(fields.examples, ["一"]);
    }
}
