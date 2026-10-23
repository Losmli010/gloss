//! 设置窗口内容：全部配置项的编辑、逐字段校验与保存入口。
//!
//! 边界：本模块只做「草稿编辑 + 校验 + 动作上交」——编辑发生在
//! [`SettingsState`] 的草稿上，校验是纯函数（规则下沉 gloss-core：
//! `Base URL` 与引擎请求前检查共源），保存/
//! 清除密钥/关闭以 [`SettingsAction`] 交还壳执行（落盘走
//! `ConfigHandle::save` 热更新路径、密钥走 `ConfigStore`，都在壳侧）。
//! `API key` 只存在于输入框字符串里，永不进 `Config` 草稿（配置红线：
//! 快照不携带凭据）。
//!
//! 校验时机：首次点「保存」才进入错误态（编辑中的半成品不追着标红），
//! 之后每帧实时复检、改对即清。视觉规范以设计定稿的设置页稿为准（卡片
//! 分区、开关行、保存主按钮）。
//!
//! 文案与校验分家：校验只产出类型化错误（[`FieldError`]）与类型化提示
//! （[`SettingsNotice`]），文案统一在渲染帧按当前 locale 落地——同一份
//! 草稿换语言即换措辞，校验逻辑本身与语言无关。
//!
//! 渲染按职责分子模块：类型与校验在这里，控件基元在 [`controls`]，
//! 分区内容组装在 [`fields`]。

use std::collections::HashMap;

use egui::ScrollArea;
use gloss_core::config::{BaseUrlError, Config, validate_base_url};
use gloss_core::model::GlossError;

use crate::ui::i18n::{Text, fill};
use crate::ui::style::space;
use crate::update::UpdateMsg;
use crate::update::state::UpdateState;
use controls::{action_row, notices, section};
use fields::{connection_section, general_section, update_section};

mod controls;
mod fields;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum FieldKey {
    BaseUrl,
    Model,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FieldError {
    NewlineInModel,
    BaseUrlEmpty,
    BaseUrlNotHttps,
    BaseUrlCredentials,
    BaseUrlQuery,
}

impl FieldError {
    fn message<'a>(&'a self, text: &'a Text) -> &'a str {
        match self {
            Self::NewlineInModel => &text.gloss_settings_error_newline_in_model,
            Self::BaseUrlEmpty => &text.gloss_settings_error_base_url_empty,
            Self::BaseUrlNotHttps => &text.gloss_settings_error_base_url_invalid,
            Self::BaseUrlCredentials => &text.gloss_settings_error_base_url_credentials,
            Self::BaseUrlQuery => &text.gloss_settings_error_base_url_query,
        }
    }
}

pub struct SettingsState {
    draft: Config,
    api_key: String,
    clear_key: bool,
    notice: Option<SettingsNotice>,
    validated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyUpdate {
    Keep,
    Replace(String),
    Clear,
}

#[derive(Debug, PartialEq)]
pub enum SettingsAction {
    Idle,
    Save { config: Box<Config>, key: KeyUpdate },
    Close,
    Update(UpdateMsg),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SettingsNotice {
    KeyUpdateFailed(GlossError),
    SaveFailed(GlossError),
    KeyUpdatedSaveFailed(GlossError),
}

impl SettingsNotice {
    fn message(&self, text: &Text) -> String {
        let (template, error) = match self {
            Self::KeyUpdateFailed(err) => (&text.gloss_settings_notice_key_update_failed, err),
            Self::SaveFailed(err) => (&text.gloss_settings_notice_save_failed, err),
            Self::KeyUpdatedSaveFailed(err) => {
                (&text.gloss_settings_notice_key_updated_save_failed, err)
            }
        };
        fill(
            template,
            &[("detail", text.for_error_detail(error).as_ref())],
        )
    }
}

pub fn open(config: &Config) -> SettingsState {
    SettingsState {
        draft: config.clone(),
        api_key: String::new(),
        clear_key: false,
        notice: None,
        validated: false,
    }
}

impl SettingsState {
    pub fn report(&mut self, notice: SettingsNotice) {
        self.notice = Some(notice);
    }

    pub fn draft(&self) -> &Config {
        &self.draft
    }

    pub fn notice(&self) -> Option<&SettingsNotice> {
        self.notice.as_ref()
    }

    fn errors(&self) -> HashMap<FieldKey, FieldError> {
        if self.validated {
            validate_draft(&self.draft)
        } else {
            HashMap::new()
        }
    }
}

fn build_save(state: &mut SettingsState) -> SettingsAction {
    state.validated = true;
    let errors = validate_draft(&state.draft);
    if !errors.is_empty() {
        return SettingsAction::Idle;
    }
    let mut draft = state.draft.clone();
    draft.base_url = draft.base_url.trim().to_owned();
    draft.model = draft.model.trim().to_owned();
    if draft.model.is_empty() {
        draft.model = gloss_core::config::DEFAULT_MODEL.to_owned();
    }
    let api_key = state.api_key.trim();
    let key = if state.clear_key {
        KeyUpdate::Clear
    } else if api_key.is_empty() {
        KeyUpdate::Keep
    } else {
        KeyUpdate::Replace(api_key.to_owned())
    };
    SettingsAction::Save {
        config: Box::new(draft),
        key,
    }
}

fn validate_draft(draft: &Config) -> HashMap<FieldKey, FieldError> {
    let mut errors = HashMap::new();
    if let Err(err) = validate_base_url(&draft.base_url) {
        errors.insert(FieldKey::BaseUrl, base_url_error(&err));
    }
    if draft.model.trim().contains('\n') {
        errors.insert(FieldKey::Model, FieldError::NewlineInModel);
    }
    errors
}

fn base_url_error(err: &BaseUrlError) -> FieldError {
    match err {
        BaseUrlError::Empty => FieldError::BaseUrlEmpty,
        BaseUrlError::Invalid | BaseUrlError::NotHttps => FieldError::BaseUrlNotHttps,
        BaseUrlError::EmbeddedCredentials => FieldError::BaseUrlCredentials,
        BaseUrlError::QueryOrFragment => FieldError::BaseUrlQuery,
    }
}

pub fn draw(
    ui: &mut egui::Ui,
    state: &mut SettingsState,
    update: &UpdateState,
    text: &Text,
) -> SettingsAction {
    let mut action = SettingsAction::Idle;
    let errors = state.errors();
    egui::Frame::new()
        .fill(ui.visuals().window_fill)
        .inner_margin(egui::Margin::same(WINDOW_PADDING))
        .show(ui, |ui| {
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                action_row(ui, state, &mut action, text);
                notices(ui, state, &errors, text);
                ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        section(ui, &text.gloss_settings_section_model, |ui| {
                            connection_section(ui, state, &errors, text);
                        });
                        ui.add_space(space::SECTION);
                        section(ui, &text.gloss_settings_section_general, |ui| {
                            general_section(ui, state, text);
                        });
                        ui.add_space(space::SECTION);
                        section(ui, &text.gloss_settings_section_update, |ui| {
                            update_section(ui, update, &mut action, text);
                        });
                    });
                });
            });
        });
    action
}

const WINDOW_PADDING: i8 = 16;

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    use egui_kittest::kittest::{NodeT, Queryable};
    use gloss_core::config::Language;
    use gloss_core::config_handle::ConfigHandle;
    use gloss_core::model::{Lang, Locale};

    use crate::stubs::ports::MemoryConfigStore;
    use crate::update::state::{FailStep, UpdatePhase};

    use super::*;

    #[test]
    fn field_errors_are_worded_per_locale() {
        let zh = Text::get(Locale::Zh);
        let en = Text::get(Locale::En);
        for (error, zh_message, en_message) in [
            (
                FieldError::NewlineInModel,
                "不能包含换行",
                "Must not contain line breaks",
            ),
            (
                FieldError::BaseUrlEmpty,
                "请填写服务地址",
                "Enter the service URL",
            ),
            (
                FieldError::BaseUrlNotHttps,
                "不是合法地址：应以 https:// 开头",
                "Not a valid URL: it must start with https://",
            ),
            (
                FieldError::BaseUrlCredentials,
                "不能内嵌账号密码",
                "Credentials must not be embedded",
            ),
            (
                FieldError::BaseUrlQuery,
                "不能携带查询参数或锚点",
                "Query strings and fragments are not allowed",
            ),
        ] {
            assert_eq!(error.message(zh), zh_message, "{error:?}");
            assert_eq!(error.message(en), en_message, "{error:?}");
        }
    }

    #[test]
    fn save_failure_notice_names_the_cause() {
        let mut state = open(&Config::default());
        state.report(SettingsNotice::SaveFailed(GlossError::Config(
            "disk on fire".into(),
        )));

        let (mut harness, _action) = harness_for(state, Locale::Zh);
        harness.run();
        harness.get_by_label_contains("保存失败：disk on fire");
    }

    #[test]
    fn blank_model_saves_as_the_factory_default() {
        let mut state = open(&Config::default());
        state.draft.model = "   ".into();

        let SettingsAction::Save { config, .. } = build_save(&mut state) else {
            panic!("save expected");
        };
        assert_eq!(
            config.model,
            gloss_core::config::DEFAULT_MODEL,
            "a blank model must not reach the disk: the frozen empty id would fail every task"
        );
    }

    #[test]
    fn save_trims_endpoint_and_treats_blank_key_as_unchanged() {
        let mut state = open(&Config::default());
        state.draft.base_url = "  https://api.example.test/v1/  ".into();
        state.api_key = "   ".into();

        let action = build_save(&mut state);
        let SettingsAction::Save { config, key } = action else {
            panic!("save expected, got {action:?}");
        };
        assert_eq!(config.base_url, "https://api.example.test/v1/");
        assert_eq!(key, KeyUpdate::Keep, "blank key means unchanged");
    }

    #[test]
    fn save_carries_the_key_outside_the_config() {
        let mut state = open(&Config::default());
        state.api_key = "  sk-test  ".into();
        state.draft.model = "m2".into();

        let SettingsAction::Save { config, key } = build_save(&mut state) else {
            panic!("save expected");
        };
        assert_eq!(key, KeyUpdate::Replace("sk-test".into()));
        assert_eq!(config.model, "m2");
        assert!(
            !format!("{config:?}").contains("sk-test"),
            "the key must never travel inside the config"
        );
    }

    #[test]
    fn clear_key_is_deferred_to_save_and_revocable() {
        let mut state = open(&Config::default());
        assert!(matches!(
            build_save(&mut state),
            SettingsAction::Save {
                key: KeyUpdate::Keep,
                ..
            }
        ));

        state.clear_key = true;
        assert!(matches!(
            build_save(&mut state),
            SettingsAction::Save {
                key: KeyUpdate::Clear,
                ..
            }
        ));

        state.api_key = "sk-typo".into();
        assert!(
            state.clear_key,
            "typing into the draft directly must not silently revoke the mark"
        );
        state.clear_key = false;
        assert!(matches!(
            build_save(&mut state),
            SettingsAction::Save {
                key: KeyUpdate::Replace(_),
                ..
            }
        ));
    }

    #[test]
    fn invalid_draft_blocks_save_and_enters_the_error_state() {
        let mut state = open(&Config::default());
        state.draft.base_url = "htp://api.example.com".into();

        assert_eq!(build_save(&mut state), SettingsAction::Idle);
        assert_eq!(
            state.errors().get(&FieldKey::BaseUrl),
            Some(&FieldError::BaseUrlNotHttps),
            "the invalid field must be flagged with the typed error"
        );
    }

    #[test]
    fn fixing_the_field_restores_save() {
        let mut state = open(&Config::default());
        state.draft.base_url = "htp://api.example.com".into();
        assert_eq!(build_save(&mut state), SettingsAction::Idle);

        state.draft.base_url = "https://api.example.com".into();
        assert!(
            state.errors().is_empty(),
            "a fixed field must clear its error"
        );
        assert!(matches!(
            build_save(&mut state),
            SettingsAction::Save { .. }
        ));
    }

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
        state.report(SettingsNotice::KeyUpdateFailed(GlossError::Config(
            "keychain locked".into(),
        )));
        assert_eq!(
            state.notice(),
            Some(&SettingsNotice::KeyUpdateFailed(GlossError::Config(
                "keychain locked".into()
            ))),
            "the notice keeps the variant so the wording stays late-bound"
        );
    }

    fn harness_for(
        state: SettingsState,
        locale: Locale,
    ) -> (egui_kittest::Harness<'static>, Rc<RefCell<SettingsAction>>) {
        harness_for_update(state, &UpdateState::default(), locale)
    }

    fn harness_for_update(
        state: SettingsState,
        update: &UpdateState,
        locale: Locale,
    ) -> (egui_kittest::Harness<'static>, Rc<RefCell<SettingsAction>>) {
        let action = Rc::new(RefCell::new(SettingsAction::Idle));
        let sink = Rc::clone(&action);
        let text = Text::get(locale);
        let update = update.clone();
        let mut state = state;
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            let frame_action = draw(ui, &mut state, &update, text);
            if frame_action != SettingsAction::Idle {
                *sink.borrow_mut() = frame_action;
            }
        });
        crate::ui::context::apply_selection_colors(&harness.ctx);
        harness.set_size(egui::vec2(
            crate::present::windows::SETTINGS_WIDTH as f32,
            crate::present::windows::SETTINGS_HEIGHT as f32,
        ));
        (harness, action)
    }

    fn snapshot_harness(
        state: SettingsState,
        update: &UpdateState,
    ) -> egui_kittest::Harness<'static> {
        let text = Text::get(Locale::En);
        let update = update.clone();
        let mut state = state;
        let mut harness = egui_kittest::Harness::builder()
            .with_theme(egui::Theme::Light)
            .build_ui(move |ui| {
                let _ = draw(ui, &mut state, &update, text);
            });
        crate::ui::context::apply_selection_colors(&harness.ctx);
        harness.set_size(egui::vec2(
            crate::present::windows::SETTINGS_WIDTH as f32,
            crate::present::windows::SETTINGS_HEIGHT as f32,
        ));
        harness
    }

    #[test]
    fn english_catalog_relabels_the_settings_window() {
        let (mut harness, _action) = harness_for(open(&Config::default()), Locale::En);
        harness.run();
        for label in [
            "Model",
            "General",
            "Save",
            "Cancel",
            "Target language",
            "Clear key",
            "Interface language",
            "Interface theme",
            "Cache lifetime",
            "Watch the clipboard for images",
        ] {
            harness.get_by_label(label);
        }
        assert!(
            harness.query_by_label("保存").is_none(),
            "the English table must not leave Chinese labels behind"
        );
    }

    #[test]
    fn clipboard_watch_toggle_travels_with_the_saved_draft() {
        let (mut harness, action) = harness_for(open(&Config::default()), Locale::En);
        harness.run();
        harness.get_by_label("Watch the clipboard for images");
        assert_eq!(
            harness
                .get_by_role(egui::accesskit::Role::CheckBox)
                .accesskit_node()
                .toggled(),
            Some(egui::accesskit::Toggled::True),
            "the factory default keeps clipboard watching on"
        );
        harness.get_by_role(egui::accesskit::Role::CheckBox).click();
        harness.run();
        harness.get_by_label("Save").click();
        harness.run();
        match &*action.borrow() {
            SettingsAction::Save { config, .. } => assert!(
                !config.watch_clipboard_images,
                "the toggled checkbox must reach the saved snapshot"
            ),
            other => panic!("save action expected, got {other:?}"),
        }
    }

    #[test]
    fn saving_the_language_swaps_the_rendered_labels_without_a_restart() {
        let store = Arc::new(MemoryConfigStore::default());
        let handle = Arc::new(ConfigHandle::with_config(store, Config::default()));
        let render = |handle: &ConfigHandle| {
            let locale = handle.snapshot().language.resolve(Locale::Zh);
            harness_for(open(&Config::default()), locale)
        };

        let (mut before, _action) = render(&handle);
        before.run();
        before.get_by_label("保存");

        handle
            .save(Config {
                language: Language::En,
                ..Default::default()
            })
            .expect("save should succeed");

        let (mut after, _action) = render(&handle);
        after.run();
        after.get_by_label("Save");
        assert!(
            after.query_by_label("保存").is_none(),
            "the saved language must drive the very next frame"
        );
    }

    #[test]
    fn a_rendered_notice_follows_the_locale() {
        let mut state = open(&Config::default());
        state.report(SettingsNotice::SaveFailed(GlossError::Config(
            "disk on fire".into(),
        )));

        let (mut harness, _action) = harness_for(state, Locale::En);
        harness.run();
        harness.get_by_label_contains("Could not save settings: disk on fire");
    }

    #[test]
    fn every_notice_renders_its_localized_prefix_and_detail() {
        let zh = Text::get(Locale::Zh);
        let en = Text::get(Locale::En);
        for (notice, zh_message, en_message) in [
            (
                SettingsNotice::KeyUpdateFailed(GlossError::Config("disk on fire".into())),
                "密钥更新失败（配置未保存）：disk on fire",
                "Could not update the API key (settings not saved): disk on fire",
            ),
            (
                SettingsNotice::SaveFailed(GlossError::Config("disk on fire".into())),
                "保存失败：disk on fire",
                "Could not save settings: disk on fire",
            ),
            (
                SettingsNotice::KeyUpdatedSaveFailed(GlossError::Config("disk on fire".into())),
                "密钥已更新，但配置保存失败：disk on fire",
                "The API key was updated but settings could not be saved: disk on fire",
            ),
        ] {
            assert_eq!(notice.message(zh), zh_message);
            assert_eq!(notice.message(en), en_message);
            assert!(
                !notice.message(zh).contains("{{") && !notice.message(en).contains("{{"),
                "a mistyped placeholder must not reach the user as literal braces"
            );
        }
    }

    #[test]
    fn base_url_errors_map_to_their_own_field_error() {
        for (error, expected) in [
            (BaseUrlError::Empty, FieldError::BaseUrlEmpty),
            (BaseUrlError::Invalid, FieldError::BaseUrlNotHttps),
            (BaseUrlError::NotHttps, FieldError::BaseUrlNotHttps),
            (
                BaseUrlError::EmbeddedCredentials,
                FieldError::BaseUrlCredentials,
            ),
            (BaseUrlError::QueryOrFragment, FieldError::BaseUrlQuery),
        ] {
            assert_eq!(base_url_error(&error), expected, "{error:?}");
        }
    }

    #[test]
    fn all_sections_render_and_save_submits_the_draft() {
        let (mut harness, action) = harness_for(open(&Config::default()), Locale::Zh);
        harness.run();
        for label in ["模型", "Base URL", "API Key", "目标语言", "通用", "保存"] {
            harness.get_by_label(label);
        }
        harness.get_by_label("保存").click();
        harness.run();
        match &*action.borrow() {
            SettingsAction::Save { config, key } => {
                assert_eq!(**config, Config::default(), "unmodified draft saves as-is");
                assert_eq!(*key, KeyUpdate::Keep);
            }
            other => panic!("save action expected, got {other:?}"),
        }
    }

    #[test]
    fn invalid_save_is_blocked_with_field_hints() {
        let (mut harness, action) = harness_for(
            {
                let mut state = open(&Config::default());
                state.draft.base_url = "htp://api.example.com".into();
                state
            },
            Locale::Zh,
        );
        harness.run();
        harness.get_by_label("保存").click();
        harness.run();
        assert!(
            matches!(&*action.borrow(), SettingsAction::Idle),
            "an invalid draft must not submit a save"
        );
        harness.get_by_label_contains("应以 https:// 开头");
        harness.get_by_label_contains("有 1 处输入未通过校验");
        harness.get_by_label_contains("已就地标红");
    }

    #[test]
    fn cancel_and_clear_key_actions_are_submitted() {
        let (mut harness, action) = harness_for(open(&Config::default()), Locale::Zh);
        harness.run();
        harness.get_by_label("取消").click();
        harness.run();
        assert_eq!(*action.borrow(), SettingsAction::Close);

        let (mut harness, _action) = harness_for(open(&Config::default()), Locale::Zh);
        harness.run();
        harness.get_by_label("清除密钥").click_accesskit();
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

    #[test]
    fn snapshots_match_baseline() {
        let mut results = egui_kittest::SnapshotResults::new();
        let mut harness = snapshot_harness(open(&Config::default()), &UpdateState::default());
        harness.run();
        harness.snapshot("settings_main");
        results.extend_harness(&mut harness);

        let mut noticed = open(&Config::default());
        noticed.report(SettingsNotice::SaveFailed(GlossError::Config(
            "disk on fire".into(),
        )));
        let mut harness = snapshot_harness(noticed, &UpdateState::default());
        harness.run();
        harness.get_by_label_contains("disk on fire");
        harness.snapshot("settings_notice");
        results.extend_harness(&mut harness);

        let mut invalid = open(&Config::default());
        invalid.draft.base_url = "htp://api.example.com".into();
        let mut harness = snapshot_harness(invalid, &UpdateState::default());
        harness.run();
        harness.get_by_label("Save").click();
        harness.run();
        harness.get_by_label_contains("marked in place");
        harness.snapshot("settings_invalid");
        results.extend_harness(&mut harness);

        snapshot_update_phases(&mut results);
        results.unwrap();
    }

    fn snapshot_update_phases(results: &mut egui_kittest::SnapshotResults) {
        for (name, update) in [
            (
                "settings_update_up_to_date",
                UpdateState {
                    phase: UpdatePhase::UpToDate,
                    version: None,
                },
            ),
            (
                "settings_update_available",
                UpdateState {
                    phase: UpdatePhase::UpdateAvailable,
                    version: Some("0.2.0".into()),
                },
            ),
            (
                "settings_update_downloading",
                UpdateState {
                    phase: UpdatePhase::Downloading,
                    version: Some("0.2.0".into()),
                },
            ),
            (
                "settings_update_ready",
                UpdateState {
                    phase: UpdatePhase::UpdateReady,
                    version: Some("0.2.0".into()),
                },
            ),
            (
                "settings_update_failed_install",
                UpdateState {
                    phase: UpdatePhase::Failed(FailStep::Install),
                    version: Some("0.2.0".into()),
                },
            ),
        ] {
            let mut harness = snapshot_harness(open(&Config::default()), &update);
            harness.run();
            harness.snapshot(name);
            results.extend_harness(&mut harness);
        }
    }

    fn update_state_of(phase: UpdatePhase) -> UpdateState {
        let version = matches!(
            phase,
            UpdatePhase::UpdateAvailable
                | UpdatePhase::Downloading
                | UpdatePhase::UpdateReady
                | UpdatePhase::Failed(_)
        )
        .then(|| "0.2.0".to_owned());
        UpdateState { phase, version }
    }

    #[test]
    fn update_section_buttons_follow_the_phase() {
        for (phase, label, expected) in [
            (UpdatePhase::Idle, "检查更新", UpdateMsg::Check),
            (UpdatePhase::UpToDate, "检查更新", UpdateMsg::Check),
            (
                UpdatePhase::UpdateAvailable,
                "下载更新",
                UpdateMsg::ConfirmDownload,
            ),
            (UpdatePhase::Downloading, "取消下载", UpdateMsg::Cancel),
            (
                UpdatePhase::UpdateReady,
                "重启更新",
                UpdateMsg::ConfirmRestart,
            ),
            (
                UpdatePhase::Failed(FailStep::Manifest),
                "重试",
                UpdateMsg::Retry,
            ),
            (
                UpdatePhase::Failed(FailStep::Download),
                "重试",
                UpdateMsg::Retry,
            ),
        ] {
            let (mut harness, action) = harness_for_update(
                open(&Config::default()),
                &update_state_of(phase),
                Locale::Zh,
            );
            harness.run();
            harness.get_by_label(label).click();
            harness.run();
            assert_eq!(
                *action.borrow(),
                SettingsAction::Update(expected),
                "phase {phase:?} must offer the `{label}` action"
            );
        }
    }

    #[test]
    fn update_busy_phases_offer_no_action_and_show_the_target_version() {
        for phase in [UpdatePhase::Checking, UpdatePhase::ReadyToRestart] {
            let (mut harness, action) = harness_for_update(
                open(&Config::default()),
                &update_state_of(phase),
                Locale::Zh,
            );
            harness.run();
            assert!(
                matches!(&*action.borrow(), SettingsAction::Idle),
                "phase {phase:?} must not offer an action"
            );
        }
        let (mut harness, _action) = harness_for_update(
            open(&Config::default()),
            &update_state_of(UpdatePhase::UpdateAvailable),
            Locale::Zh,
        );
        harness.run();
        harness.get_by_label_contains("0.2.0");
    }

    #[test]
    fn failed_install_shows_the_install_hint() {
        let (mut harness, action) = harness_for_update(
            open(&Config::default()),
            &update_state_of(UpdatePhase::Failed(FailStep::Install)),
            Locale::Zh,
        );
        harness.run();
        harness.get_by_label_contains("应用程序");
        assert!(matches!(&*action.borrow(), SettingsAction::Idle));
    }
}
