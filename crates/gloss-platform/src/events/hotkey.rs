//! 全局热键事件源：注册 → global-hotkey 全局事件队列 → 事件线程转发。
//!
//! 线程约束：global-hotkey 的 macOS 后端要求 manager 在主线程创建（winit 的
//! NSApp 事件循环所在线程）；注册后的按键事件走它自己的全局 crossbeam 队列，
//! 任意线程可消费——平台事件线程经 [`HotkeyPump`] 抽干转发，主线程约束不影响
//! 其余事件源。
//!
//! 热键是可降级功能：单个注册失败（被其他应用占用等）只告警跳过，管理器
//! 整体创建失败降级为空表，都不允许阻断启动或 panic。

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use gloss_core::log::{info, thread, warn};
use gloss_core::task::{HotkeyBinding, InputSource, TaskKind};

/// 配置化之前的默认热键表：触发键字符串、任务类型、输入源。
/// 有意避开 macOS 截图（Cmd+Shift+3/4/5）等系统级组合。
const DEFAULT_BINDINGS: &[(&str, TaskKind, InputSource)] = &[
    (
        "Cmd+Shift+D",
        TaskKind::TranslateWord,
        InputSource::Selection,
    ),
    (
        "Cmd+Shift+F",
        TaskKind::TranslateSentence,
        InputSource::Selection,
    ),
    ("Cmd+Shift+E", TaskKind::ExplainCode, InputSource::Selection),
];

/// 已注册热键的管理端。macOS 上必须在主线程创建（见模块注释）。
pub struct HotkeyRegistrar {
    // 持有 manager 保活；None 表示创建失败、热键功能整体降级。
    _manager: Option<GlobalHotKeyManager>,
    table: Arc<HashMap<u32, HotkeyBinding>>,
}

impl HotkeyRegistrar {
    /// 用内置默认热键表注册。
    pub fn with_defaults() -> Self {
        Self::new(
            DEFAULT_BINDINGS
                .iter()
                .map(|(trigger, kind, source)| HotkeyBinding {
                    trigger: (*trigger).to_owned(),
                    kind: *kind,
                    source: *source,
                }),
        )
    }

    /// 注册一组绑定。解析失败的键与管理器不可用时的行为见模块注释。
    pub fn new(bindings: impl IntoIterator<Item = HotkeyBinding>) -> Self {
        // Linux 只作 CI 平台：global-hotkey 的 X11 后端在无显示环境创建会
        // 直接段错误，不支持的平台明确跳过，而不是冒崩溃风险。
        #[cfg(all(unix, not(target_os = "macos")))]
        let manager: Option<GlobalHotKeyManager> = {
            warn!(
                thread = thread::EVENT,
                "global hotkeys unsupported on this platform, disabled"
            );
            None
        };
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let manager = match GlobalHotKeyManager::new() {
            Ok(manager) => Some(manager),
            Err(err) => {
                warn!(thread = thread::EVENT, error = %err, "hotkey manager unavailable, hotkeys disabled");
                None
            }
        };
        let mut table = HashMap::new();
        for binding in bindings {
            let hotkey = match parse_trigger(&binding.trigger) {
                Ok(hotkey) => hotkey,
                Err(err) => {
                    warn!(thread = thread::EVENT, trigger = %binding.trigger, error = err, "hotkey binding skipped: unparseable trigger");
                    continue;
                }
            };
            if let Some(manager) = &manager
                && let Err(err) = manager.register(hotkey)
            {
                warn!(thread = thread::EVENT, trigger = %binding.trigger, error = %err, "hotkey registration failed, binding skipped");
                continue;
            }
            // 管理器不可用时仍保留解析成功的条目：事件不会到达，但表内容
            // 可诊断（配置了什么、各绑定 id 是什么），也便于测试。
            table.insert(hotkey.id(), binding);
        }
        info!(
            thread = thread::EVENT,
            count = table.len(),
            "global hotkeys registered"
        );
        Self {
            _manager: manager,
            table: Arc::new(table),
        }
    }

    /// 交给平台事件线程的只读消费端。
    pub fn pump(&self) -> HotkeyPump {
        HotkeyPump {
            table: Arc::clone(&self.table),
        }
    }
}

/// 事件线程侧的热键消费端：抽干 global-hotkey 的全局事件队列。
#[derive(Clone)]
pub struct HotkeyPump {
    table: Arc<HashMap<u32, HotkeyBinding>>,
}

impl HotkeyPump {
    /// 返回本次抽干中「按下」的绑定；抬起（Released）事件丢弃，避免一次
    /// 按键产生两次触发。
    pub fn poll(&self) -> Vec<HotkeyBinding> {
        let receiver = GlobalHotKeyEvent::receiver();
        let mut pressed = Vec::new();
        while let Ok(event) = receiver.try_recv()
            && event.state == HotKeyState::Pressed
            && let Some(binding) = self.table.get(&event.id)
        {
            pressed.push(binding.clone());
        }
        pressed
    }
}

/// 把触发键字符串解析成 global-hotkey 的 `HotKey`，如 `"Cmd+Shift+1"`。
/// 修饰键大小写不敏感；无法识别的段返回 Err（注册侧告警跳过）。
fn parse_trigger(trigger: &str) -> Result<HotKey, String> {
    let mut modifiers = Modifiers::empty();
    let mut key: Option<Code> = None;
    for token in trigger.split('+') {
        let token = token.trim();
        match token.to_ascii_lowercase().as_str() {
            // SUPER 在 macOS 即 Command、Windows 即 Win 键，是 global-hotkey
            // 的跨平台「系统键」概念，故 Cmd 与 Win 同映射。
            "cmd" | "command" | "super" | "win" | "meta" => modifiers |= Modifiers::SUPER,
            "ctrl" | "control" => modifiers |= Modifiers::CONTROL,
            "shift" => modifiers |= Modifiers::SHIFT,
            "alt" | "option" | "opt" => modifiers |= Modifiers::ALT,
            other => match parse_code(other) {
                Some(code) if key.is_none() => key = Some(code),
                Some(_) => return Err(format!("duplicate key in `{trigger}`")),
                None => return Err(format!("unknown key `{other}` in `{trigger}`")),
            },
        }
    }
    let key = key.ok_or_else(|| format!("no key in `{trigger}`"))?;
    Ok(HotKey::new(Some(modifiers), key))
}

/// 解析单个键名：数字/字母/功能键/少量命名键。global-hotkey 的 `Code`
/// 遵循 keyboard-types 的物理键名（`Digit1` / `KeyA` / `F1`）。
fn parse_code(name: &str) -> Option<Code> {
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
            } else if first == 'f' && !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()) {
                // 功能键 F1..F24，键名原样大写。
                other.to_ascii_uppercase()
            } else {
                return None;
            }
        }
    };
    Code::from_str(&canonical).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modifier_combinations() {
        let hotkey = parse_trigger("Cmd+Shift+1").unwrap();
        assert!(hotkey.id() != 0);
        assert!(
            parse_trigger("ctrl+alt+p").is_ok(),
            "case-insensitive modifiers"
        );
        assert!(parse_trigger("Option+K").is_ok(), "option aliases alt");
    }

    #[test]
    fn rejects_malformed_triggers() {
        for bad in ["", "Cmd", "Cmd+Foo", "Cmd+1+2", "++", "Cmd+"] {
            assert!(parse_trigger(bad).is_err(), "`{bad}` should not parse");
        }
    }

    #[test]
    fn code_covers_digits_letters_and_named_keys() {
        assert!(matches!(parse_code("1"), Some(Code::Digit1)));
        assert!(matches!(parse_code("a"), Some(Code::KeyA)));
        assert!(matches!(parse_code("f5"), Some(Code::F5)));
        assert!(matches!(parse_code("Return"), Some(Code::Enter)));
        assert!(parse_code("Foo").is_none());
    }

    #[test]
    fn default_bindings_are_all_parseable() {
        for (trigger, _, _) in DEFAULT_BINDINGS {
            assert!(parse_trigger(trigger).is_ok(), "`{trigger}` must parse");
        }
    }

    /// 管理器不可用/注册失败的路径必须安静降级：不 panic、poll 恒为空。
    /// Linux 上热键明确不支持（见 `new`），manager 恒为 None，正好覆盖整体
    /// 降级分支；此时表里应保留全部解析成功的默认绑定。
    #[cfg(target_os = "linux")]
    #[test]
    fn degraded_registrar_keeps_parsed_table_and_stays_quiet() {
        let registrar = HotkeyRegistrar::with_defaults();
        assert_eq!(registrar.table.len(), DEFAULT_BINDINGS.len());
        assert!(registrar.pump().poll().is_empty(), "no real keypress in CI");
    }

    /// macOS/Windows runner 语义不确定（可能成功注册），只验证不 panic。
    #[cfg(not(target_os = "linux"))]
    #[test]
    fn registrar_construction_never_panics() {
        let registrar = HotkeyRegistrar::with_defaults();
        assert!(registrar.pump().poll().is_empty(), "no real keypress in CI");
    }
}
