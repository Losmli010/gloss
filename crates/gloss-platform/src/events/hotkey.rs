//! 全局热键事件源：注册 → global-hotkey 全局事件队列 → 事件线程转发。
//!
//! 线程约束：manager 必须创建在泵系统消息的主线程——macOS 后端要求主线程跑
//! NSApp 事件循环（winit 所在线程），Windows 后端的隐藏窗口与 `WM_HOTKEY`
//! 投递、连同 `Drop` 的 `DestroyWindow` 都亲和创建线程。注册后的按键事件走
//! global-hotkey 自己的全局 crossbeam 队列，任意线程可消费——平台事件线程经
//! [`HotkeyPump`] 抽干转发，主线程约束不影响其余事件源。因此组装点必须把
//! registrar 放在主线程创建、只把 pump 下发事件线程；在事件线程里创建
//! registrar 会让 Windows 热键静默全灭（无错误无日志）。
//!
//! 绑定来自配置（M4-T7）：registrar 启动时按 `Config::hotkey_bindings` 建表，
//! 设置页保存后经 [`HotkeyBinder`] 端口重绑定。**重绑定同样必须在主线程
//! 调用**（注销与注册是同一类亲和调用），这也是该端口不加 `Send + Sync` 的
//! 原因。表本身与事件线程的 pump 共享（`Arc<RwLock<..>>`）：换表后 pump 立刻
//! 按新映射解析按键，不需要重启事件线程。
//!
//! 热键是可降级功能：单个注册失败（被其他应用占用等）只告警跳过，管理器
//! 整体创建失败降级为空表，都不允许阻断启动或 panic。
//!
//! 本模块的日志一律 `thread = UI`：创建、注册、重绑定全部发生在主线程
//! （`log::thread` 的 `EVENT` 专指平台事件线程，见其文档）。

use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyEventReceiver, GlobalHotKeyManager, HotKeyState,
};

use gloss_core::log::{debug, info, thread, warn};
use gloss_core::ports::HotkeyBinder;
use gloss_core::task::HotkeyBinding;

/// 已注册热键的管理端。macOS/Windows 上必须在主线程创建与重绑定
/// （见模块注释）。
pub struct HotkeyRegistrar {
    /// 持有 manager 保活；`None` 表示创建失败、热键功能整体降级。
    manager: Option<GlobalHotKeyManager>,
    /// 当前已注册的键。重绑定时先按它们注销：表里的 id 是 `HotKey::id()`
    /// 的派生值，不足以还原出可注销的 `HotKey`，所以这份原始值要留。
    registered: Mutex<Vec<HotKey>>,
    /// 生效中的 id → 绑定表，与事件线程的 [`HotkeyPump`] 共享。
    /// 写侧只有主线程的启动与重绑定（低频），读侧是事件线程每 tick 一次。
    table: Arc<RwLock<HashMap<u32, HotkeyBinding>>>,
}

impl HotkeyRegistrar {
    /// 按给定绑定表注册。绑定来自配置（M4-T7 起不再有写死的默认表：
    /// 出厂默认在 `gloss_core::config`，与设置页可编辑的是同一份）。
    pub fn new(bindings: impl IntoIterator<Item = HotkeyBinding>) -> Self {
        // Linux 只作 CI 平台：global-hotkey 的 X11 后端在无显示环境创建会
        // 直接段错误，不支持的平台明确跳过，而不是冒崩溃风险。
        #[cfg(all(unix, not(target_os = "macos")))]
        let manager: Option<GlobalHotKeyManager> = {
            warn!(
                thread = thread::UI,
                "global hotkeys unsupported on this platform, disabled"
            );
            None
        };
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let manager = match GlobalHotKeyManager::new() {
            Ok(manager) => Some(manager),
            Err(err) => {
                warn!(thread = thread::UI, error = %err, "hotkey manager unavailable, hotkeys disabled");
                None
            }
        };
        let (table, registered) = register_all(manager.as_ref(), bindings);
        info!(
            thread = thread::UI,
            count = table.len(),
            "global hotkeys registered"
        );
        Self {
            manager,
            registered: Mutex::new(registered),
            table: Arc::new(RwLock::new(table)),
        }
    }

    /// 交给平台事件线程的只读消费端。
    pub fn pump(&self) -> HotkeyPump {
        HotkeyPump {
            table: Arc::clone(&self.table),
        }
    }
}

/// 重绑定（端口实现，M4-T7）：**只在主线程调用**（见模块注释的线程亲和
/// 约束）。返回实际生效的条数——少于入参说明有绑定被降级跳过。
///
/// 先注销再注册，而不是反过来：旧键留着的话，它仍在系统级被吞掉，而它的
/// id 已不在新表里——按下去什么都不发生，比「热键失效」更难查。注销失败
/// 只记 debug：此时新表照常接管，功能表现为「旧键可能多响一次」。
impl HotkeyBinder for HotkeyRegistrar {
    fn rebind(&self, bindings: &[HotkeyBinding]) -> usize {
        let stale = {
            let mut registered = self
                .registered
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            std::mem::take(&mut *registered)
        };
        if let Some(manager) = &self.manager {
            for hotkey in stale {
                if let Err(err) = manager.unregister(hotkey) {
                    debug!(
                        thread = thread::UI,
                        error = %err,
                        "hotkey unregister failed, binding may stay live"
                    );
                }
            }
        }

        let (table, registered) = register_all(self.manager.as_ref(), bindings.iter().cloned());
        let applied = table.len();
        *self.table.write().unwrap_or_else(PoisonError::into_inner) = table;
        *self
            .registered
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = registered;
        info!(
            thread = thread::UI,
            declared = bindings.len(),
            applied,
            "hotkey bindings re-registered"
        );
        applied
    }
}

/// 解析并注册一组绑定，产出（id → 绑定表，已注册键表）。
///
/// 管理器不可用时仍保留解析成功的条目进表：事件不会到达，但表内容可诊断
/// （配置了什么、各绑定 id 是什么），也便于测试。
fn register_all(
    manager: Option<&GlobalHotKeyManager>,
    bindings: impl IntoIterator<Item = HotkeyBinding>,
) -> (HashMap<u32, HotkeyBinding>, Vec<HotKey>) {
    let mut table = HashMap::new();
    let mut registered = Vec::new();
    let mut seen_triggers = HashSet::new();
    for binding in bindings {
        // 同一触发键注册两次时后者覆盖前者、前者无痕丢失，明确拒绝。
        if !seen_triggers.insert(binding.trigger.clone()) {
            warn!(thread = thread::UI, trigger = %binding.trigger, "hotkey binding skipped: duplicate trigger");
            continue;
        }
        let (hotkey, modifiers) = match parse_trigger(&binding.trigger) {
            Ok(parsed) => parsed,
            Err(err) => {
                warn!(thread = thread::UI, trigger = %binding.trigger, error = err, "hotkey binding skipped: unparseable trigger");
                continue;
            }
        };
        // 无修饰键的裸键会作为全局热键在系统级吞掉普通输入（如字母 a），
        // 一律拒绝——热键必须带修饰键。
        if modifiers.is_empty() {
            warn!(thread = thread::UI, trigger = %binding.trigger, "hotkey binding skipped: bare key would capture plain typing system-wide");
            continue;
        }
        if let Some(manager) = manager
            && let Err(err) = manager.register(hotkey)
        {
            warn!(thread = thread::UI, trigger = %binding.trigger, error = %err, "hotkey registration failed, binding skipped");
            continue;
        }
        // 注册失败（上面已 continue）不留注销记录；管理器缺失时无事可注销。
        if manager.is_some() {
            registered.push(hotkey);
        }
        table.insert(hotkey.id(), binding);
    }
    (table, registered)
}

/// 事件线程侧的热键消费端：抽干 global-hotkey 的全局事件队列。
#[derive(Clone)]
pub struct HotkeyPump {
    table: Arc<RwLock<HashMap<u32, HotkeyBinding>>>,
}

impl HotkeyPump {
    /// 返回本次抽干中「按下」的绑定；抬起（Released）事件丢弃，避免一次
    /// 按键产生两次触发。读锁只在这一小段持有——写侧是主线程保存路径上
    /// 的低频重绑定，不会让事件线程等在主线程后面。
    pub fn poll(&self) -> Vec<HotkeyBinding> {
        let table = self.table.read().unwrap_or_else(PoisonError::into_inner);
        drain_pressed(GlobalHotKeyEvent::receiver(), &table)
    }
}

/// 抽干队列，只保留「按下」且在表中的事件。Released 与未知 id 丢弃后必须
/// 继续抽干——按下/抬起成对出现，若遇非 Pressed 就停会把同批后续按下
/// 推迟到下一个 drain 周期。
fn drain_pressed(
    receiver: &GlobalHotKeyEventReceiver,
    table: &HashMap<u32, HotkeyBinding>,
) -> Vec<HotkeyBinding> {
    let mut pressed = Vec::new();
    // Err（Empty 即抽干完毕）退出循环；每个取到的事件独立过滤，
    // Released / 未知 id 只是被跳过，不会中断本轮抽干。
    while let Ok(event) = receiver.try_recv() {
        if event.state == HotKeyState::Pressed
            && let Some(binding) = table.get(&event.id)
        {
            pressed.push(binding.clone());
        }
    }
    pressed
}

/// 把触发键字符串解析成 global-hotkey 的 `HotKey`，如 `"Cmd+Shift+1"`。
/// 修饰键大小写不敏感；无法识别的段返回 Err（注册侧告警跳过）。
/// 同时返回修饰键集合，供注册侧拒绝无修饰键的裸键（HotKey 本身不暴露）。
fn parse_trigger(trigger: &str) -> Result<(HotKey, Modifiers), String> {
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
    Ok((HotKey::new(Some(modifiers), key), modifiers))
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
    use crossbeam_channel::unbounded;
    use gloss_core::config::Config;
    use gloss_core::task::{InputSource, TaskKind};

    use super::*;

    fn binding(trigger: &str) -> HotkeyBinding {
        HotkeyBinding {
            trigger: trigger.to_owned(),
            kind: TaskKind::TranslateWord,
            source: InputSource::Selection,
        }
    }

    #[test]
    fn parses_modifier_combinations() {
        let (hotkey, modifiers) = parse_trigger("Cmd+Shift+1").unwrap();
        assert!(hotkey.id() != 0);
        assert!(modifiers.contains(Modifiers::SUPER) && modifiers.contains(Modifiers::SHIFT));
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

    /// 出厂默认绑定必须都能被本模块解析、且都带修饰键——两处一旦分叉，
    /// 用户装完按默认热键什么都不会发生（且只在日志里留一行 warn）。
    /// M4-T7 起默认表只剩 core 一份，本测试就是那条「单点」的护栏。
    #[test]
    fn factory_bindings_are_all_registerable() {
        let bindings = Config::default().hotkey_bindings;
        assert!(!bindings.is_empty(), "factory defaults must ship bindings");
        for binding in &bindings {
            let (_, modifiers) = parse_trigger(&binding.trigger)
                .unwrap_or_else(|err| panic!("`{}` must parse: {err}", binding.trigger));
            assert!(
                !modifiers.is_empty(),
                "`{}` must carry a modifier or it would eat plain typing",
                binding.trigger
            );
        }
    }

    /// 管理器不可用/注册失败的路径必须安静降级：不 panic、poll 恒为空。
    /// Linux 上热键明确不支持（见 `new`），manager 恒为 None，正好覆盖整体
    /// 降级分支；此时表里应保留全部解析成功的出厂绑定，且没有键被记为
    /// 「已注册」（没有管理器就没有可注销的东西）。
    #[cfg(target_os = "linux")]
    #[test]
    fn degraded_registrar_keeps_parsed_table_and_stays_quiet() {
        let factory = Config::default().hotkey_bindings;
        let registrar = HotkeyRegistrar::new(factory.clone());
        assert_eq!(registrar.table.read().unwrap().len(), factory.len());
        assert!(
            registrar.registered.lock().unwrap().is_empty(),
            "no manager means nothing was handed to the platform"
        );
        assert!(registrar.pump().poll().is_empty(), "no real keypress in CI");
    }

    /// macOS/Windows runner 语义不确定（可能成功注册），只验证不 panic。
    #[cfg(not(target_os = "linux"))]
    #[test]
    fn registrar_construction_never_panics() {
        let registrar = HotkeyRegistrar::new(Config::default().hotkey_bindings);
        assert!(registrar.pump().poll().is_empty(), "no real keypress in CI");
    }

    /// 无修饰键的裸键会系统级吞掉普通输入，注册侧必须拒绝；
    /// 同一触发键重复注册会无痕覆盖前者，也必须拒绝。
    #[test]
    fn bare_keys_and_duplicates_are_rejected_before_registration() {
        let registrar = HotkeyRegistrar::new([
            binding("a"), // 裸键
            binding("Cmd+Shift+F12"),
            binding("Cmd+Shift+F12"), // 重复
            binding("shift"),         // 仅修饰键、无键位
        ]);
        let table = registrar.table.read().unwrap();
        let kept: Vec<&HotkeyBinding> = table
            .values()
            .filter(|b| b.trigger == "Cmd+Shift+F12")
            .collect();
        assert_eq!(kept.len(), 1, "duplicates collapse to at most one entry");
        assert!(
            table
                .values()
                .all(|b| b.trigger != "a" && b.trigger != "shift"),
            "bare keys must not reach the table: {:?}",
            table.values().map(|b| &b.trigger).collect::<Vec<_>>()
        );
    }

    /// 纯解析/过滤路径（不经平台管理器）：重复键、裸键与无法识别的触发键
    /// 一并剔除；管理器缺失时表照建，但不留注销记录。
    #[test]
    fn register_all_filters_without_a_manager() {
        let (table, registered) = register_all(
            None,
            [
                binding("a"),       // 裸键
                binding("shift"),   // 仅修饰键
                binding("Cmd+Foo"), // 无法识别的键位
                binding("Cmd+Shift+D"),
                binding("Cmd+Shift+D"), // 重复
                binding("Cmd+Shift+F"),
            ],
        );
        assert_eq!(
            table.len(),
            2,
            "only the two distinct valid triggers survive"
        );
        assert!(
            registered.is_empty(),
            "without a manager there is nothing to unregister"
        );
        assert!(
            table.values().all(|b| b.trigger.starts_with("Cmd+Shift+")),
            "no rejected trigger may reach the table"
        );
    }

    /// 重绑定的核心契约：pump 读到的是 `rebind` 换上的那份表，而不是启动时
    /// 建的那份——事件线程因此不必重启就能按新绑定解析按键。
    ///
    /// 「已注册键全部属于新表」在 macOS/Windows 上可能因 F9 恰好被别的应用
    /// 占用而成空断言，故再用 `Arc::ptr_eq` 钉住共享关系本身；Linux（CI）
    /// 上管理器恒为 None、条目必进表，另有条数断言兜底。
    #[test]
    fn pump_observes_the_rebound_table() {
        let registrar = HotkeyRegistrar::new([binding("Cmd+Shift+F12")]);
        let pump = registrar.pump();
        assert!(
            Arc::ptr_eq(&pump.table, &registrar.table),
            "the pump must read the registrar's table, not a copy of it"
        );

        let applied = registrar.rebind(&[binding("Cmd+Alt+Ctrl+F9")]);
        let seen: Vec<String> = pump
            .table
            .read()
            .unwrap()
            .values()
            .map(|b| b.trigger.clone())
            .collect();
        assert!(
            seen.iter().all(|trigger| trigger == "Cmd+Alt+Ctrl+F9"),
            "pump must read the rebound table, got {seen:?}"
        );
        assert!(
            applied <= 1,
            "applied count can never exceed the declared bindings"
        );
        #[cfg(target_os = "linux")]
        {
            assert_eq!(seen.len(), 1, "degraded platform still fills the table");
            assert_eq!(applied, 1, "applied count must match the new table");
        }
    }

    /// 重绑定是**替换**不是追加：连续改小绑定表，表跟着缩小到空。
    #[cfg(target_os = "linux")]
    #[test]
    fn rebind_replaces_instead_of_appending() {
        let registrar = HotkeyRegistrar::new([
            binding("Cmd+Shift+D"),
            binding("Cmd+Shift+F"),
            binding("Cmd+Shift+E"),
        ]);
        assert_eq!(registrar.table.read().unwrap().len(), 3);

        assert_eq!(registrar.rebind(&[binding("Cmd+Shift+D")]), 1);
        assert_eq!(registrar.table.read().unwrap().len(), 1);

        assert_eq!(registrar.rebind(&[]), 0);
        assert!(
            registrar.table.read().unwrap().is_empty(),
            "an empty table must clear every binding"
        );
    }

    /// 端口对象安全：组装点以 `Arc<dyn HotkeyBinder>` 注入 App，适配器必须
    /// 能经 trait 对象调用，并如实回报生效条数。两条断言都选与平台无关的
    /// 输入（空表、裸键），在三种目标平台上结果一致。
    #[test]
    fn binder_port_is_object_safe_and_reports_applied_count() {
        let binder: Arc<dyn HotkeyBinder> = Arc::new(HotkeyRegistrar::new(Vec::new()));
        assert_eq!(binder.rebind(&[]), 0, "an empty table applies nothing");
        assert_eq!(
            binder.rebind(&[binding("a")]),
            0,
            "bare keys are rejected, so nothing is applied"
        );
    }

    /// 抽干过滤不断流：Released 与未知 id 丢弃后，同批后续的 Pressed 仍会被
    /// 收集（按下/抬起成对出现，这是此前 let-chain 版本的隐藏缺陷）。
    #[test]
    fn drain_pressed_skips_released_and_unknown_ids_without_stopping() {
        let (tx, rx) = unbounded::<GlobalHotKeyEvent>();
        let mut table = HashMap::new();
        let kept = binding("Cmd+Shift+D");
        let (hotkey, _) = parse_trigger("Cmd+Shift+D").unwrap();
        table.insert(hotkey.id(), kept.clone());

        let other_id = hotkey.id().wrapping_add(1);
        tx.send(GlobalHotKeyEvent {
            id: hotkey.id(),
            state: HotKeyState::Pressed,
        })
        .unwrap();
        tx.send(GlobalHotKeyEvent {
            id: hotkey.id(),
            state: HotKeyState::Released,
        })
        .unwrap();
        tx.send(GlobalHotKeyEvent {
            id: other_id,
            state: HotKeyState::Pressed,
        })
        .unwrap();
        tx.send(GlobalHotKeyEvent {
            id: hotkey.id(),
            state: HotKeyState::Pressed,
        })
        .unwrap();

        let drained = drain_pressed(&rx, &table);
        assert_eq!(
            drained,
            vec![kept.clone(), kept],
            "exactly the two valid presses"
        );
    }
}
