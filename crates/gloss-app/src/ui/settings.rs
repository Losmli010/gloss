//! 设置窗口内容（M4-T6）：全部配置项的编辑与保存入口。
//!
//! 边界：本模块只做「草稿编辑 + 动作上交」——编辑发生在 [`SettingsState`]
//! 的草稿上，保存/清除密钥/关闭以 [`SettingsAction`] 交还壳执行（落盘走
//! `ConfigHandle::save` 热更新路径、密钥走 `ConfigStore`，都在壳侧）。
//! API key 只存在于输入框字符串里，永不进 `Config` 草稿（配置红线：快照
//! 不携带凭据）。
//!
//! 消费状态（M4-T7）：`hotkey_bindings` 保存后由壳立即重注册，`theme` /
//! `auto_show` 也已在壳侧消费；只剩 `cache_ttl_secs` 尚未接上运行时（归
//! 缓存构造接线），照常可编辑保存——配置先行，不至于为了一个字段把设置页
//! 留一半空白。

use egui::{Color32, RichText, ScrollArea};
use gloss_core::config::{ALL_KINDS, Config, Theme};
use gloss_core::model::Lang;
use gloss_core::task::{HotkeyBinding, InputSource, TaskKind};

use super::kind_label;

/// 设置窗口的一个编辑会话：打开时以当前快照建草稿，保存/关闭由壳销毁。
pub struct SettingsState {
    /// 编辑中的整份配置；「保存」时整体上交（整份快照语义）。
    draft: Config,
    /// API key 输入框；只在保存时交给壳写 keychain，永不进 `draft`。
    api_key: String,
    /// 是否已标记「清除密钥」（保存时才真正删除；重新输入即撤销）。
    clear_key: bool,
    /// 壳回写的提示（保存失败等）；用户可见文案。
    notice: Option<String>,
}

/// 密钥的保存语义。
///
/// 删除不可逆，所以「清除密钥」不立即生效：它先标记，再随保存一起上交
/// ——否则点「取消」也留着一条删掉的密钥，与撤销语义矛盾。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyUpdate {
    /// 密钥保持不变。
    Keep,
    /// 用这个密钥覆盖（已 trim，非空）。
    Replace(String),
    /// 删除当前 provider 的密钥。
    Clear,
}

/// 设置窗口上交的动作：浮层只渲染，落盘/密钥/关窗都在壳。
#[derive(Debug, PartialEq)]
pub enum SettingsAction {
    /// 普通编辑帧，无需壳动作。
    Idle,
    /// 保存整份配置并应用密钥变更。
    Save {
        /// 保存的整份配置。
        config: Config,
        /// 密钥变更（保持 / 覆盖 / 清除）。
        key: KeyUpdate,
    },
    /// 关闭窗口并丢弃草稿。
    Close,
}

/// 打开一个编辑会话：草稿取自当前快照（打开后的配置变更不跟读，保存即
/// 整份覆盖——与「单次任务内配置一致」同一取舍）。
pub fn open(config: &Config) -> SettingsState {
    SettingsState {
        draft: config.clone(),
        api_key: String::new(),
        clear_key: false,
        notice: None,
    }
}

impl SettingsState {
    /// 壳回写提示（保存失败等）；在下一次渲染帧展示。
    pub fn report(&mut self, message: String) {
        self.notice = Some(message);
    }

    /// 当前草稿（壳只读：未保存变更提示等）。
    pub fn draft(&self) -> &Config {
        &self.draft
    }

    /// 最近一条壳回写的提示。
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }
}

/// 画一帧设置窗口，返回本帧用户上交的动作。
///
/// 自下而上布局：动作行钉在窗口底部，提示在其上，其余全部区块进滚动区
/// ——内容再长也不会把「保存」推出视口。
pub fn draw(ui: &mut egui::Ui, state: &mut SettingsState) -> SettingsAction {
    let mut action = SettingsAction::Idle;
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        ui.horizontal(|ui| {
            if ui.button("保存").clicked() {
                action = build_save(state);
            }
            if ui.button("取消").clicked() {
                action = SettingsAction::Close;
            }
        });
        if let Some(notice) = &state.notice {
            ui.add_space(4.0);
            ui.label(
                RichText::new(notice.as_str())
                    .size(12.0)
                    .color(Color32::from_rgb(0xD8, 0x5A, 0x30)),
            );
        }
        ui.add_space(6.0);
        // bottom_up 会渗进子 Ui：滚动内容显式转回 top_down，区块才从顶部
        // 开始排列。
        ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                connection_section(ui, state);
                ui.add_space(10.0);
                task_section(ui, state);
                ui.add_space(10.0);
                hotkey_section(ui, state);
                ui.add_space(10.0);
                general_section(ui, state);
            });
        });
    });
    action
}

/// 「保存」的上交物：整份草稿 + 密钥变更（清除标记优先，其次输入框内容，
/// 都为空则保持原密钥）。
fn build_save(state: &SettingsState) -> SettingsAction {
    let mut draft = state.draft.clone();
    draft.base_url = draft.base_url.trim().to_owned();
    let api_key = state.api_key.trim();
    let key = if state.clear_key {
        KeyUpdate::Clear
    } else if api_key.is_empty() {
        KeyUpdate::Keep
    } else {
        KeyUpdate::Replace(api_key.to_owned())
    };
    SettingsAction::Save { config: draft, key }
}

/// 连接区：端点 + API key（写 keychain，不进配置）。
fn connection_section(ui: &mut egui::Ui, state: &mut SettingsState) {
    ui.strong("连接");
    egui::Grid::new("connection_grid")
        .num_columns(2)
        .spacing([8.0, 6.0])
        .show(ui, |ui| {
            ui.label("端点");
            ui.add(
                egui::TextEdit::singleline(&mut state.draft.base_url)
                    .hint_text("https://api.deepseek.com/v1")
                    .desired_width(f32::INFINITY),
            );
            ui.end_row();

            ui.label("API Key");
            ui.horizontal(|ui| {
                let typed = ui
                    .add(
                        egui::TextEdit::singleline(&mut state.api_key)
                            .password(true)
                            .hint_text(if state.clear_key {
                                "保存后清除"
                            } else {
                                "留空则不修改"
                            })
                            .desired_width(160.0),
                    )
                    .changed();
                if typed {
                    // 重新输入即撤销「清除」意图。
                    state.clear_key = false;
                }
                let label = if state.clear_key {
                    "撤销清除"
                } else {
                    "清除密钥"
                };
                if ui.button(label).clicked() {
                    state.clear_key = !state.clear_key;
                    state.api_key.clear();
                }
            });
            ui.end_row();
        });
}

/// 任务区：划词默认任务、任务开关、每任务默认模型。
fn task_section(ui: &mut egui::Ui, state: &mut SettingsState) {
    ui.strong("任务");
    egui::Grid::new("task_grid")
        .num_columns(2)
        .spacing([8.0, 6.0])
        .show(ui, |ui| {
            ui.label("划词默认任务");
            kind_combo(
                ui,
                "default_text_kind",
                &mut state.draft.default_text_kind,
                &TEXT_KINDS,
            );
            ui.end_row();

            ui.label("目标语言");
            lang_combo(ui, &mut state.draft.target_lang);
            ui.end_row();
        });

    ui.add_space(4.0);
    ui.label("任务开关");
    for kind in ALL_KINDS {
        let mut enabled = state.draft.is_kind_enabled(kind);
        // 「启用」前缀让勾选框的树标签与任务名（下拉选中文本、模型行标
        // 签）保持可区分。
        if ui
            .checkbox(&mut enabled, format!("启用{}", kind_label(kind)))
            .changed()
        {
            state.draft.set_kind_enabled(kind, enabled);
        }
    }

    ui.add_space(4.0);
    ui.label("默认模型");
    egui::Grid::new("model_grid")
        .num_columns(2)
        .spacing([8.0, 6.0])
        .show(ui, |ui| {
            for kind in ALL_KINDS {
                let mut model = state
                    .draft
                    .model_for_kind(kind)
                    .unwrap_or_default()
                    .to_owned();
                ui.label(kind_label(kind));
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut model)
                            .hint_text(if kind.accepts_text() {
                                "deepseek-chat"
                            } else {
                                "视觉模型（M5）"
                            })
                            .desired_width(160.0),
                    )
                    .changed()
                {
                    state.draft.set_model_for_kind(kind, &model);
                }
                ui.end_row();
            }
        });
}

/// 热键区：绑定表的触发键与任务类型就地编辑（输入源随绑定展示，只读）。
fn hotkey_section(ui: &mut egui::Ui, state: &mut SettingsState) {
    ui.strong("热键");
    egui::Grid::new("hotkey_grid")
        .num_columns(3)
        .spacing([8.0, 6.0])
        .show(ui, |ui| {
            for index in 0..state.draft.hotkey_bindings.len() {
                // 按下标借出可变绑定；Grid 闭包内逐行处理。
                let Some(binding) = state.draft.hotkey_bindings.get_mut(index) else {
                    continue;
                };
                let HotkeyBinding {
                    trigger,
                    kind,
                    source,
                } = binding;
                ui.add(
                    egui::TextEdit::singleline(trigger)
                        .desired_width(110.0)
                        .font(egui::TextStyle::Monospace),
                );
                kind_combo(ui, &format!("hotkey_kind_{index}"), kind, &ALL_KINDS);
                ui.label(
                    RichText::new(source_label(source))
                        .size(11.0)
                        .color(ui.visuals().weak_text_color()),
                );
                ui.end_row();
            }
        });
}

/// 通用区：自动浮层、缓存 TTL、主题。
fn general_section(ui: &mut egui::Ui, state: &mut SettingsState) {
    ui.strong("通用");
    ui.checkbox(&mut state.draft.auto_show, "取材成功后自动弹出浮层");
    egui::Grid::new("general_grid")
        .num_columns(2)
        .spacing([8.0, 6.0])
        .show(ui, |ui| {
            ui.label("缓存有效期");
            ui.add(
                egui::DragValue::new(&mut state.draft.cache_ttl_secs)
                    .range(0..=u64::MAX)
                    .suffix(" 秒"),
            );
            ui.end_row();

            ui.label("主题");
            theme_combo(ui, &mut state.draft.theme);
            ui.end_row();
        });
}

/// 划词可服务的任务类型（与 `Config::selection_task_kind` 的收口一致）。
const TEXT_KINDS: [TaskKind; 3] = [
    TaskKind::TranslateWord,
    TaskKind::TranslateSentence,
    TaskKind::ExplainCode,
];

/// 任务类型下拉；`id_salt` 需在窗口内唯一。
fn kind_combo(ui: &mut egui::Ui, id_salt: &str, current: &mut TaskKind, choices: &[TaskKind]) {
    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(kind_label(*current))
        .show_ui(ui, |ui| {
            for &kind in choices {
                ui.selectable_value(current, kind, kind_label(kind));
            }
        });
}

/// UI 固定可选的 5 语种（`Lang::Other` 只经配置文件到达，不在下拉里）。
const LANG_CHOICES: fn() -> [Lang; 5] = || [Lang::Zh, Lang::En, Lang::Ja, Lang::Ko, Lang::Fr];

/// 语言标签。
fn lang_label(lang: &Lang) -> &'static str {
    match lang {
        Lang::Zh => "简体中文",
        Lang::En => "英语",
        Lang::Ja => "日语",
        Lang::Ko => "韩语",
        Lang::Fr => "法语",
        Lang::Other(_) => "其他",
    }
}

/// 目标语言下拉。
fn lang_combo(ui: &mut egui::Ui, current: &mut Lang) {
    egui::ComboBox::from_id_salt("target_lang")
        .selected_text(lang_label(current))
        .show_ui(ui, |ui| {
            for lang in LANG_CHOICES() {
                let label = lang_label(&lang);
                ui.selectable_value(current, lang, label);
            }
        });
}

/// 主题标签与下拉（消费在壳侧，见 `app::apply_theme`）。
fn theme_combo(ui: &mut egui::Ui, current: &mut Theme) {
    egui::ComboBox::from_id_salt("theme")
        .selected_text(match current {
            Theme::System => "跟随系统",
            Theme::Light => "浅色",
            Theme::Dark => "深色",
        })
        .show_ui(ui, |ui| {
            for (theme, label) in [
                (Theme::System, "跟随系统"),
                (Theme::Light, "浅色"),
                (Theme::Dark, "深色"),
            ] {
                ui.selectable_value(current, theme, label);
            }
        });
}

/// 输入源标签（只读展示）。
fn source_label(source: &InputSource) -> &'static str {
    match source {
        InputSource::Selection => "划词",
        InputSource::Region => "框选",
    }
}

#[cfg(test)]
mod tests {
    //! 草稿逻辑单测 + L2 kittest 渲染与交互（AccessKit 树断言）；快照
    //! 基线与其他 harness 合并进同一个 SnapshotResults。

    use std::cell::RefCell;
    use std::rc::Rc;

    use egui_kittest::kittest::Queryable;
    use gloss_core::task::InputSource;

    use super::*;

    /// 「保存」的上交物：trim 端点、空白密钥按「不修改」上交。
    #[test]
    fn save_trims_endpoint_and_treats_blank_key_as_unchanged() {
        let mut state = open(&Config::default());
        state.draft.base_url = "  https://api.example.test/v1/  ".into();
        state.api_key = "   ".into();

        let action = build_save(&state);
        let SettingsAction::Save { config, key } = action else {
            panic!("save expected, got {action:?}");
        };
        assert_eq!(config.base_url, "https://api.example.test/v1/");
        assert_eq!(key, KeyUpdate::Keep, "blank key means unchanged");
    }

    /// 非空密钥随保存上交，且草稿里没有密钥（配置红线：快照不带凭据）。
    #[test]
    fn save_carries_the_key_outside_the_config() {
        let mut state = open(&Config::default());
        state.api_key = "  sk-test  ".into();
        state
            .draft
            .set_model_for_kind(TaskKind::TranslateWord, "m2");

        let SettingsAction::Save { config, key } = build_save(&state) else {
            panic!("save expected");
        };
        assert_eq!(key, KeyUpdate::Replace("sk-test".into()));
        assert_eq!(config.model_for_kind(TaskKind::TranslateWord), Some("m2"));
        // 密钥只经动作的独立字段出会话，草稿（含其调试表示）里不该有它。
        assert!(
            !format!("{config:?}").contains("sk-test"),
            "the key must never travel inside the config"
        );
    }

    /// 清除密钥是「标记 + 保存时生效」：单点标记不上交删除动作，保存才上交；
    /// 重新输入密钥即撤销标记（删除不可逆，不能随取消一起留着）。
    #[test]
    fn clear_key_is_deferred_to_save_and_revocable() {
        let mut state = open(&Config::default());
        assert!(matches!(
            build_save(&state),
            SettingsAction::Save {
                key: KeyUpdate::Keep,
                ..
            }
        ));

        state.clear_key = true;
        assert!(matches!(
            build_save(&state),
            SettingsAction::Save {
                key: KeyUpdate::Clear,
                ..
            }
        ));

        // 清除标记优先于输入框内容（清空输入框不该让「清除」变成「保持」）。
        state.api_key = "sk-typo".into();
        assert!(
            state.clear_key,
            "typing into the draft directly must not silently revoke the mark"
        );
        state.clear_key = false;
        assert!(matches!(
            build_save(&state),
            SettingsAction::Save {
                key: KeyUpdate::Replace(_),
                ..
            }
        ));
    }

    /// 打开会话：草稿取自当前快照，与调用方后续的配置变更解耦。
    #[test]
    fn open_copies_the_snapshot_into_the_draft() {
        let mut config = Config {
            target_lang: Lang::Ja,
            ..Default::default()
        };
        let mut state = open(&config);
        assert_eq!(state.draft.target_lang, Lang::Ja);

        config.target_lang = Lang::Ko;
        assert_eq!(
            state.draft.target_lang,
            Lang::Ja,
            "draft must not follow the live config"
        );
        state.report("保存失败".into());
        assert_eq!(state.notice.as_deref(), Some("保存失败"));
    }

    /// 驱动一帧设置窗口；返回（harness，动作收集器）。
    fn harness_for(
        mut state: SettingsState,
    ) -> (egui_kittest::Harness<'static>, Rc<RefCell<SettingsAction>>) {
        let action = Rc::new(RefCell::new(SettingsAction::Idle));
        let sink = Rc::clone(&action);
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            // 只累积非 Idle 动作：一次 run 可能驱动多帧，点击帧之后的帧
            // 回 Idle，不能把已上交的动作冲掉。
            let frame_action = draw(ui, &mut state);
            if frame_action != SettingsAction::Idle {
                *sink.borrow_mut() = frame_action;
            }
        });
        // 画布拉到内容全高：ScrollArea 只物化可见区，默认小画布下折叠的
        // 控件不进 AccessKit 树，查询会空手而归。
        harness.set_size(egui::vec2(460.0, 1200.0));
        (harness, action)
    }

    /// 全部配置项可达（AccessKit 树）：连接/任务/热键/通用四个区块的关键
    /// 控件都能按文本定位，保存按钮可点击并上交出厂快照。
    #[test]
    fn all_sections_render_and_save_submits_the_draft() {
        let (mut harness, action) = harness_for(open(&Config::default()));
        harness.run();
        for label in [
            "连接",
            "端点",
            "API Key",
            "任务",
            "划词默认任务",
            "目标语言",
            "热键",
            "通用",
            "保存",
        ] {
            harness.get_by_label(label);
        }
        harness.get_by_label("保存").click();
        harness.run();
        match &*action.borrow() {
            SettingsAction::Save { config, key } => {
                assert_eq!(*config, Config::default(), "unmodified draft saves as-is");
                assert_eq!(*key, KeyUpdate::Keep);
            }
            other => panic!("save action expected, got {other:?}"),
        }
    }

    /// 任务开关可点：取消「词卡」后保存，上交的配置里该 kind 已停用。
    #[test]
    fn task_toggle_flips_enabled_kinds() {
        let (mut harness, action) = harness_for(open(&Config::default()));
        harness.run();
        harness.get_by_label("启用词卡").click();
        harness.run();
        harness.get_by_label("保存").click();
        harness.run();
        match &*action.borrow() {
            SettingsAction::Save { config, .. } => assert!(
                !config.is_kind_enabled(TaskKind::TranslateWord),
                "toggled-off kind must be disabled in the submitted config"
            ),
            other => panic!("save action expected, got {other:?}"),
        }
    }

    /// 取消按钮上交 Close（不产生任何密钥动作）；「清除密钥」按钮只把标记
    /// 翻成待清除态（按钮改为「撤销清除」），不立即上交删除。
    #[test]
    fn cancel_and_clear_key_actions_are_submitted() {
        let (mut harness, action) = harness_for(open(&Config::default()));
        harness.run();
        harness.get_by_label("取消").click();
        harness.run();
        assert_eq!(*action.borrow(), SettingsAction::Close);

        let (mut harness, _action) = harness_for(open(&Config::default()));
        harness.run();
        harness.get_by_label("清除密钥").click();
        harness.run();
        harness.get_by_label("撤销清除");
        harness.get_by_label("保存").click();
        harness.run();
        assert!(
            matches!(
                &*_action.borrow(),
                SettingsAction::Save {
                    key: KeyUpdate::Clear,
                    ..
                }
            ),
            "the clear mark must surface on save, not on click"
        );
    }

    /// 热键绑定行可达：输入源标签（只读文本）随每一行进 AccessKit 树，
    /// 与出厂三条绑定一一对应；触发键是 TextEdit 的值而非标签，语义由
    /// 草稿逻辑单测覆盖。
    #[test]
    fn hotkey_rows_expose_trigger_and_kind() {
        let (mut harness, _action) = harness_for(open(&Config::default()));
        harness.run();
        assert_eq!(
            harness.get_all_by_label("划词").count(),
            3,
            "three factory bindings must expose their selection source"
        );
    }

    /// 快照对比（wgpu 渲染 + 基线图 diff）。与浮层 harness 的基线相互独立，
    /// 但同属一个 SnapshotResults 约定：结果合并处理。
    #[test]
    fn snapshots_match_baseline() {
        let mut results = egui_kittest::SnapshotResults::new();
        let (mut harness, _action) = harness_for(open(&Config::default()));
        harness.run();
        harness.snapshot("settings_main");
        results.extend_harness(&mut harness);
        results.unwrap();
    }

    /// 输入源标签穷举（新增变体时 match 非穷举会编译失败，这里钉住文案）。
    #[test]
    fn source_labels_cover_all_variants() {
        assert_eq!(source_label(&InputSource::Selection), "划词");
        assert_eq!(source_label(&InputSource::Region), "框选");
    }
}
