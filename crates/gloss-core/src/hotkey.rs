//! 触发键字符串的语法解析（纯逻辑）：platform 的热键注册映射与设置页
//! 的逐字段校验共用这一份语法，避免两套口径漂移。
//!
//! 语法：段间以 `+` 分隔、大小写不敏感；修饰键（Cmd/Super/Win/Meta、
//! Ctrl/Control、Shift、Alt/Option/Opt）可任意组合，主键恰好一个——
//! 数字（`1`）、字母（`a`）、功能键（`F1`–`F24`）与少量命名键
//! （Space/Enter/Tab/Escape/方向键）。全局热键必须带修饰键：裸键会在
//! 系统级吞掉普通打字。

use std::fmt;

/// 修饰键集合（语法层面；平台注册侧映射到各自的热键类型）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TriggerModifiers {
    /// Command（`Cmd` / `Super` / `Win` / `Meta` 同义）。
    pub super_key: bool,
    /// Control（`Ctrl` / `Control`）。
    pub ctrl: bool,
    /// Shift。
    pub shift: bool,
    /// Alt（`Alt` / `Option` / `Opt`）。
    pub alt: bool,
}

impl TriggerModifiers {
    /// 是否至少带一个修饰键。
    pub fn any(self) -> bool {
        self.super_key || self.ctrl || self.shift || self.alt
    }

    /// 规范化呈现序（macOS 惯例：Ctrl / Alt / Shift / Cmd）。
    fn names(self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if self.ctrl {
            names.push("Ctrl");
        }
        if self.alt {
            names.push("Alt");
        }
        if self.shift {
            names.push("Shift");
        }
        if self.super_key {
            names.push("Cmd");
        }
        names
    }
}

/// 解析出的触发键：修饰键集合 + 主键规范名（物理键名，如 `Digit1` /
/// `KeyA` / `F1` / `ArrowUp`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTrigger {
    /// 修饰键集合。
    pub modifiers: TriggerModifiers,
    /// 主键规范名。
    pub key: String,
}

impl ParsedTrigger {
    /// 规范化串（如 `Cmd+Shift+1`）：重复绑定检测用它比较——写法不同
    /// （`cmd+shift+1`、`Shift+Cmd+1`）但指同一组合的触发键视为重复。
    pub fn canonical(&self) -> String {
        let mut parts: Vec<String> = self
            .modifiers
            .names()
            .into_iter()
            .map(str::to_owned)
            .collect();
        parts.push(self.key.clone());
        parts.join("+")
    }
}

/// 触发键语法错误。`Display` 是英文（进日志），展示层文案自行映射。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerError {
    /// 空串（或纯空白）。
    Empty,
    /// 只有修饰键、没有主键。
    NoKey,
    /// 没有任何修饰键（裸键会被系统级吞掉普通打字）。
    NoModifiers,
    /// 出现了两个主键。
    DuplicateKey,
    /// 无法识别的段。
    UnknownKey(String),
}

impl fmt::Display for TriggerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TriggerError::Empty => write!(f, "empty trigger"),
            TriggerError::NoKey => write!(f, "no key in trigger"),
            TriggerError::NoModifiers => {
                write!(f, "bare key would capture plain typing system-wide")
            }
            TriggerError::DuplicateKey => write!(f, "duplicate key in trigger"),
            TriggerError::UnknownKey(name) => write!(f, "unknown key `{name}` in trigger"),
        }
    }
}

/// 解析触发键字符串，如 `Cmd+Shift+1`。规则见模块文档；解析成功即保证
/// 带修饰键——裸键在这里被拒绝，注册与设置校验同源。
pub fn parse_trigger(trigger: &str) -> Result<ParsedTrigger, TriggerError> {
    if trigger.trim().is_empty() {
        return Err(TriggerError::Empty);
    }
    let mut modifiers = TriggerModifiers::default();
    let mut key: Option<String> = None;
    for token in trigger.split('+') {
        let token = token.trim();
        match token.to_ascii_lowercase().as_str() {
            "cmd" | "command" | "super" | "win" | "meta" => modifiers.super_key = true,
            "ctrl" | "control" => modifiers.ctrl = true,
            "shift" => modifiers.shift = true,
            "alt" | "option" | "opt" => modifiers.alt = true,
            other => match canonical_key_name(other) {
                Some(canonical) if key.is_none() => key = Some(canonical),
                Some(_) => return Err(TriggerError::DuplicateKey),
                None => return Err(TriggerError::UnknownKey(other.to_owned())),
            },
        }
    }
    let key = key.ok_or(TriggerError::NoKey)?;
    if !modifiers.any() {
        return Err(TriggerError::NoModifiers);
    }
    Ok(ParsedTrigger { modifiers, key })
}

/// 主键段 → 规范物理键名（keyboard-types 命名法）。
fn canonical_key_name(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let canonical = match lower.as_str() {
        "space" => "Space".to_owned(),
        "enter" | "return" => "Enter".to_owned(),
        "tab" => "Tab".to_owned(),
        "esc" | "escape" => "Escape".to_owned(),
        "up" => "ArrowUp".to_owned(),
        "down" => "ArrowDown".to_owned(),
        "left" => "ArrowLeft".to_owned(),
        "right" => "ArrowRight".to_owned(),
        other => {
            let mut chars = other.chars();
            let (first, rest) = (chars.next()?, chars.as_str());
            if first.is_ascii_digit() && rest.is_empty() {
                format!("Digit{}", first)
            } else if first.is_ascii_alphabetic() && rest.is_empty() {
                format!("Key{}", first.to_ascii_uppercase())
            } else if first == 'f'
                && !rest.is_empty()
                && !rest.starts_with('0')
                && rest.bytes().all(|b| b.is_ascii_digit())
            {
                let number: usize = rest.parse().ok()?;
                (1..=24)
                    .contains(&number)
                    .then(|| other.to_ascii_uppercase())?
            } else {
                return None;
            }
        }
    };
    Some(canonical)
}

#[cfg(test)]
mod tests {
    use super::{TriggerError, parse_trigger};

    #[test]
    fn parses_modifier_combinations() {
        let parsed = parse_trigger("Cmd+Shift+1").unwrap();
        assert!(parsed.modifiers.super_key && parsed.modifiers.shift);
        assert_eq!(parsed.key, "Digit1");
        assert_eq!(parsed.canonical(), "Shift+Cmd+Digit1");
    }

    #[test]
    fn canonical_forms_agree_across_spellings() {
        let spellings = ["Cmd+Alt+T", "cmd+alt+t", "Meta+Option+T", "Super+Opt+T"];
        let canonical: Vec<_> = spellings
            .iter()
            .map(|spelling| parse_trigger(spelling).unwrap().canonical())
            .collect();
        assert!(
            canonical.windows(2).all(|pair| pair[0] == pair[1]),
            "same physical combination must share one canonical form"
        );
    }

    #[test]
    fn named_and_function_keys_resolve() {
        assert_eq!(parse_trigger("Ctrl+Enter").unwrap().key, "Enter");
        assert_eq!(parse_trigger("Ctrl+Return").unwrap().key, "Enter");
        assert_eq!(parse_trigger("Ctrl+Esc").unwrap().key, "Escape");
        assert_eq!(parse_trigger("Cmd+Up").unwrap().key, "ArrowUp");
        assert_eq!(parse_trigger("Cmd+F12").unwrap().key, "F12");
    }

    #[test]
    fn bare_keys_and_broken_grammars_are_rejected() {
        assert_eq!(parse_trigger(""), Err(TriggerError::Empty));
        assert_eq!(parse_trigger("   "), Err(TriggerError::Empty));
        assert_eq!(parse_trigger("Cmd"), Err(TriggerError::NoKey));
        assert_eq!(
            parse_trigger("Cmd+"),
            Err(TriggerError::UnknownKey(String::new())),
            "与原 platform 解析行为一致：悬空分隔符按未知段拒绝"
        );
        assert_eq!(parse_trigger("t"), Err(TriggerError::NoModifiers));
        assert_eq!(
            parse_trigger("Cmd+Ctrl+T+R"),
            Err(TriggerError::DuplicateKey)
        );
        assert_eq!(
            parse_trigger("Cmd+Alt+?"),
            Err(TriggerError::UnknownKey("?".into()))
        );
        assert_eq!(
            parse_trigger("Cmd+F25"),
            Err(TriggerError::UnknownKey("f25".into())),
            "错误串与原 platform 解析一致（段名小写化后回传）"
        );
        assert_eq!(
            parse_trigger("Cmd+f01"),
            Err(TriggerError::UnknownKey("f01".into())),
            "前导零功能键与原 parse_code 一致（Code::from_str 必失败，提前拒绝）"
        );
    }
}
