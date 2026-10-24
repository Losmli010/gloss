//! 设置页的分区内容：连接区（端点地址、密钥与模型）、通用区与更新区
//! 的控件组装，以及目标语言、界面语言、主题三个下拉；控件基元见
//! 控件模块与下文。

use std::borrow::Cow;
use std::collections::HashMap;

use gloss_core::config::{CACHE_TTL_MAX_SECS, Language, Theme};
use gloss_core::model::Lang;

use super::controls::{
    add_input, choice_hint, choice_row, error_text, toggle_switch, underline_if_error,
};
use super::{FieldError, FieldKey, SettingsAction, SettingsState};
use crate::ui::i18n::{Text, fill};
use crate::ui::style::space;
use crate::update::UpdateMsg;
use crate::update::state::{FailStep, UpdatePhase, UpdateState};

pub(super) fn connection_section(
    ui: &mut egui::Ui,
    state: &mut SettingsState,
    errors: &HashMap<FieldKey, FieldError>,
    text: &Text,
) {
    ui.label("Base URL");
    let response = add_input(ui, &mut state.draft.base_url, |e| {
        e.hint_text(text.gloss_settings_base_url_hint.as_str())
            .desired_width(f32::INFINITY)
    });
    underline_if_error(ui, &response, errors, FieldKey::BaseUrl);
    error_text(ui, errors, FieldKey::BaseUrl, text);
    ui.add_space(space::ITEM);

    ui.label("API Key");
    ui.horizontal(|ui| {
        let input_width = ui.available_width() - CLEAR_BUTTON_RESERVE;
        let typed = add_input(ui, &mut state.api_key, |e| {
            e.password(true)
                .hint_text(if state.clear_key {
                    text.gloss_settings_key_clear_hint.as_str()
                } else {
                    text.gloss_settings_key_keep_hint.as_str()
                })
                .desired_width(input_width)
        })
        .changed();
        if typed {
            state.clear_key = false;
        }
        let label = if state.clear_key {
            text.gloss_settings_undo_clear_key.as_str()
        } else {
            text.gloss_settings_clear_key.as_str()
        };
        if ui.button(label).clicked() {
            state.clear_key = !state.clear_key;
            state.api_key.clear();
        }
    });
    ui.add_space(space::ITEM);

    ui.label(&text.gloss_settings_model);
    let response = add_input(ui, &mut state.draft.model, |e| {
        e.hint_text(text.gloss_settings_model_hint.as_str())
            .desired_width(f32::INFINITY)
    });
    underline_if_error(ui, &response, errors, FieldKey::Model);
    error_text(ui, errors, FieldKey::Model, text);
    choice_hint(ui, &text.gloss_settings_model_hint);
}

const CLEAR_BUTTON_RESERVE: f32 = 88.0;

pub(super) fn general_section(ui: &mut egui::Ui, state: &mut SettingsState, text: &Text) {
    choice_row(ui, &text.gloss_settings_target_lang, |ui| {
        lang_combo(ui, &mut state.draft.target_lang, text);
    });
    choice_row(ui, &text.gloss_settings_ui_language, |ui| {
        language_combo(ui, &mut state.draft.language, text);
    });
    choice_row(ui, &text.gloss_settings_ui_theme, |ui| {
        theme_combo(ui, &mut state.draft.theme, text);
    });
    choice_row(ui, &text.gloss_settings_cache_ttl, |ui| {
        ui.add(
            egui::DragValue::new(&mut state.draft.cache_ttl_secs)
                .range(0..=CACHE_TTL_MAX_SECS)
                .suffix(text.gloss_settings_cache_ttl_suffix.as_str()),
        );
    });
    choice_hint(ui, &text.gloss_settings_cache_ttl_hint);
    choice_row(ui, &text.gloss_settings_watch_clipboard_images, |ui| {
        toggle_switch(ui, &mut state.draft.watch_clipboard_images)
    });
    choice_hint(ui, &text.gloss_settings_watch_clipboard_images_hint);
}

pub(super) fn update_section(
    ui: &mut egui::Ui,
    update: &UpdateState,
    action: &mut SettingsAction,
    text: &Text,
) {
    let label = update_label(update, text);
    let button = update_button(update, text);
    choice_row(ui, &label, |ui| {
        if let Some((label, msg)) = button
            && ui.button(label).clicked()
        {
            *action = SettingsAction::Update(msg);
        }
    });
    if update.phase == UpdatePhase::Failed(FailStep::Install) {
        choice_hint(ui, &text.gloss_settings_update_install_hint);
    }
}

fn update_label<'a>(update: &UpdateState, text: &'a Text) -> Cow<'a, str> {
    let version = update.version.as_deref().unwrap_or_default();
    match update.phase {
        UpdatePhase::Idle => Cow::Borrowed(&text.gloss_settings_update_idle),
        UpdatePhase::Checking => Cow::Borrowed(&text.gloss_settings_update_checking),
        UpdatePhase::UpToDate => Cow::Borrowed(&text.gloss_settings_update_up_to_date),
        UpdatePhase::UpdateAvailable => Cow::Owned(fill(
            &text.gloss_settings_update_available,
            &[("version", version)],
        )),
        UpdatePhase::Downloading => Cow::Borrowed(&text.gloss_settings_update_downloading),
        UpdatePhase::UpdateReady => Cow::Owned(fill(
            &text.gloss_settings_update_ready,
            &[("version", version)],
        )),
        UpdatePhase::ReadyToRestart => Cow::Borrowed(&text.gloss_settings_update_installing),
        UpdatePhase::Failed(step) => failed_label(step, text),
    }
}

fn failed_label(step: FailStep, text: &Text) -> Cow<'_, str> {
    match step {
        FailStep::Manifest => Cow::Borrowed(&text.gloss_settings_update_failed_check),
        FailStep::Download => Cow::Borrowed(&text.gloss_settings_update_failed_download),
        FailStep::Install => Cow::Borrowed(&text.gloss_settings_update_failed_install),
    }
}

fn update_button<'a>(update: &UpdateState, text: &'a Text) -> Option<(Cow<'a, str>, UpdateMsg)> {
    match update.phase {
        UpdatePhase::Idle => Some((
            Cow::Borrowed(&text.gloss_settings_update_check),
            UpdateMsg::Check,
        )),
        UpdatePhase::Checking => None,
        UpdatePhase::UpToDate => Some((
            Cow::Borrowed(&text.gloss_settings_update_check),
            UpdateMsg::Check,
        )),
        UpdatePhase::UpdateAvailable => Some((
            Cow::Borrowed(&text.gloss_settings_update_download),
            UpdateMsg::ConfirmDownload,
        )),
        UpdatePhase::Downloading => Some((
            Cow::Borrowed(&text.gloss_settings_update_cancel),
            UpdateMsg::Cancel,
        )),
        UpdatePhase::UpdateReady => Some((
            Cow::Borrowed(&text.gloss_settings_update_restart),
            UpdateMsg::ConfirmRestart,
        )),
        UpdatePhase::ReadyToRestart => None,
        UpdatePhase::Failed(_) => Some((
            Cow::Borrowed(&text.gloss_settings_update_retry),
            UpdateMsg::Retry,
        )),
    }
}

const LANG_CHOICES: fn() -> [Lang; 5] = || [Lang::Zh, Lang::En, Lang::Ja, Lang::Ko, Lang::Fr];

fn lang_label<'a>(lang: &Lang, text: &'a Text) -> &'a str {
    match lang {
        Lang::Zh => &text.gloss_langs_zh,
        Lang::En => &text.gloss_langs_en,
        Lang::Ja => &text.gloss_langs_ja,
        Lang::Ko => &text.gloss_langs_ko,
        Lang::Fr => &text.gloss_langs_fr,
        Lang::Other(_) => &text.gloss_langs_other,
    }
}

fn lang_combo(ui: &mut egui::Ui, current: &mut Lang, text: &Text) {
    egui::ComboBox::from_id_salt("target_lang")
        .selected_text(lang_label(current, text))
        .show_ui(ui, |ui| {
            for lang in LANG_CHOICES() {
                let label = lang_label(&lang, text);
                ui.selectable_value(current, lang, label);
            }
        });
}

fn language_label<'a>(language: &Language, text: &'a Text) -> &'a str {
    match language {
        Language::System => &text.gloss_ui_language_system,
        Language::Zh => &text.gloss_ui_language_zh,
        Language::En => &text.gloss_ui_language_en,
    }
}

fn language_combo(ui: &mut egui::Ui, current: &mut Language, text: &Text) {
    egui::ComboBox::from_id_salt("ui_language")
        .selected_text(language_label(current, text))
        .show_ui(ui, |ui| {
            for language in [Language::System, Language::Zh, Language::En] {
                ui.selectable_value(current, language, language_label(&language, text));
            }
        });
}

fn theme_combo(ui: &mut egui::Ui, current: &mut Theme, text: &Text) {
    let label = |theme: &Theme| match theme {
        Theme::System => text.gloss_theme_system.as_str(),
        Theme::Light => text.gloss_theme_light.as_str(),
        Theme::Dark => text.gloss_theme_dark.as_str(),
    };
    egui::ComboBox::from_id_salt("theme")
        .selected_text(label(current))
        .show_ui(ui, |ui| {
            for theme in [Theme::System, Theme::Light, Theme::Dark] {
                ui.selectable_value(current, theme, label(&theme));
            }
        });
}
