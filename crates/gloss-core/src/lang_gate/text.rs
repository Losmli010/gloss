//! 字段文本的解析层：（markdown）事件流 → 结构节点判定与正文文本节点序列，
//! 以及数学定界区的切分。数学定界必须在同一文本节点内成对，跨节点不
//! 拼装；落单的美元符号与数字前货币符号不构成容器、也不判违约。

use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};

pub(crate) struct FieldText {
    pub(crate) chars: Vec<char>,
    pub(crate) node: Vec<u32>,
}

impl FieldText {
    pub(crate) fn len(&self) -> usize {
        self.chars.len()
    }

    fn push(&mut self, text: &str, node: u32) {
        for c in text.chars() {
            self.chars.push(c);
            self.node.push(node);
        }
    }
}

pub(crate) struct ParsedField {
    pub(crate) markdown_forbidden: bool,
    pub(crate) text: FieldText,
}

pub(crate) fn parse_field(markdown_allowed: bool, raw: &str) -> ParsedField {
    let mut parse = ParseState {
        markdown_allowed,
        markdown_forbidden: false,
        node: 0,
        in_code_block: false,
        in_autolink: false,
        text: FieldText {
            chars: Vec::with_capacity(raw.len()),
            node: Vec::with_capacity(raw.len()),
        },
    };
    for event in Parser::new_ext(raw, Options::empty()) {
        parse.accept(event);
    }
    ParsedField {
        markdown_forbidden: parse.markdown_forbidden,
        text: parse.text,
    }
}

struct ParseState {
    markdown_allowed: bool,
    markdown_forbidden: bool,
    node: u32,
    in_code_block: bool,
    in_autolink: bool,
    text: FieldText,
}

impl ParseState {
    fn accept(&mut self, event: Event<'_>) {
        match event {
            Event::Text(t) => {
                if !self.in_code_block && !self.in_autolink {
                    self.text.push(&t, self.node);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                self.node += 1;
                self.text.push("\n", self.node);
            }
            Event::Start(tag) => self.start(tag),
            Event::End(tag_end) => match tag_end {
                TagEnd::CodeBlock => self.in_code_block = false,
                TagEnd::Link => self.in_autolink = false,
                _ => {}
            },
            Event::Code(_)
            | Event::Html(_)
            | Event::InlineHtml(_)
            | Event::Rule
            | Event::InlineMath(_)
            | Event::DisplayMath(_) => self.flag(),
            Event::FootnoteReference(_) | Event::TaskListMarker(_) => self.flag(),
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        if let Tag::Link { link_type, .. } = &tag
            && matches!(link_type, LinkType::Autolink | LinkType::Email)
        {
            self.in_autolink = true;
        }
        if matches!(tag, Tag::CodeBlock(_)) {
            self.in_code_block = true;
        }
        if is_structure_tag(&tag) {
            self.flag();
        }
    }

    fn flag(&mut self) {
        self.markdown_forbidden |= !self.markdown_allowed;
    }
}

fn is_structure_tag(tag: &Tag<'_>) -> bool {
    !matches!(tag, Tag::Paragraph)
}

pub(crate) fn math_ranges(text: &FieldText) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut open: Option<(bool, usize)> = None;
    let mut index = 0;
    while index < text.len() {
        let Some((block, width)) = delimiter_at(text, index) else {
            index += 1;
            continue;
        };
        if !block && text.chars.get(index + 1).is_some_and(char::is_ascii_digit) {
            index += width;
            continue;
        }
        match open {
            Some((open_block, open_at))
                if open_block == block && text.node[open_at] == text.node[index] =>
            {
                ranges.push((open_at, index + width));
                open = None;
            }
            _ => open = Some((block, index)),
        }
        index += width;
    }
    ranges
}

fn delimiter_at(text: &FieldText, index: usize) -> Option<(bool, usize)> {
    if text.chars[index] != '$' {
        return None;
    }
    if text.chars.get(index + 1) == Some(&'$') {
        return Some((true, 2));
    }
    Some((false, 1))
}
