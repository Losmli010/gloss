//! 流式正文的渐进提取：对累积的原始流做“转义感知”的顺序双字段扫描。
//!
//! 纯决策函数，不依赖界面库（egui）绘制侧；“未到齐”的部分一律留在进度态，由
//! 下一帧整段重扫补齐。

#[derive(Default)]
pub(super) struct StreamFields {
    pub(super) note: String,
    pub(super) examples: Vec<String>,
}

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

fn scan_fields(cursor: &mut Cursor<'_>, fields: &mut StreamFields, nested: bool) {
    loop {
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
        if !consume_value(cursor, fields, nested, &key) {
            return;
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

fn consume_value(
    cursor: &mut Cursor<'_>,
    fields: &mut StreamFields,
    nested: bool,
    key: &str,
) -> bool {
    match (nested, key) {
        (false, "note") => {
            let Some((value, closed)) = cursor.json_string() else {
                return false;
            };
            fields.note = value;
            closed
        }
        (false, "structured") => {
            if cursor.strip("{") {
                scan_fields(cursor, fields, true);
                true
            } else {
                cursor.skip_value()
            }
        }
        (true, "examples") => {
            let Some((items, closed)) = cursor.string_array() else {
                return false;
            };
            fields.examples = items;
            closed
        }
        _ => cursor.skip_value(),
    }
}

struct Cursor<'a> {
    rest: &'a str,
}

impl Cursor<'_> {
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

    fn json_string(&mut self) -> Option<(String, bool)> {
        self.skip_ws();
        if !self.strip("\"") {
            return None;
        }
        let mut out = String::new();
        let mut chars = self.rest.chars();
        loop {
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
