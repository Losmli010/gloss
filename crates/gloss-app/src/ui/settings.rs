//! 设置窗口内容：全部配置项的编辑、逐字段校验与保存入口。
//!
//! 边界：本模块只做「草稿编辑 + 校验 + 动作上交」——编辑发生在
//! [`SettingsState`] 的草稿上，校验是纯函数（规则下沉 gloss-core：
//! Base URL 与引擎请求前检查共源、热键语法与注册映射共源），保存/
//! 清除密钥/关闭以 [`SettingsAction`] 交还壳执行（落盘走
//! `ConfigHandle::save` 热更新路径、密钥走 `ConfigStore`，都在壳侧）。
//! API key 只存在于输入框字符串里，永不进 `Config` 草稿（配置红线：
//! 快照不携带凭据）。
//!
//! 校验时机：首次点「保存」才进入错误态（编辑中的半成品不追着标红），
//! 之后每帧实时复检、改对即清。视觉规范见 docs/14（卡片分区、开关行、
//! 保存主按钮）。
//!
//! 文案与校验分家：校验只产出类型化错误（[`FieldError`]）与类型化提示
//! （[`SettingsNotice`]），文案统一在渲染帧按当前 locale 落地——同一份草稿
//! 换语言即换措辞，校验逻辑本身与语言无关。

use std::collections::HashMap;

use egui::{RichText, ScrollArea, Stroke, vec2};
use gloss_core::config::{
    ALL_KINDS, BaseUrlError, CACHE_TTL_MAX_SECS, Config, Language, Theme, validate_base_url,
};
use gloss_core::hotkey::parse_trigger;
use gloss_core::model::{GlossError, Lang};
use gloss_core::task::{HotkeyBinding, TaskKind};

use super::kind_label;
use super::style::{color, font, radius, space};
use crate::i18n::{Text, fill};

/// 校验出错的字段：错误提示按字段定位到具体控件。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum FieldKey {
    /// Base URL（结构性校验）。
    BaseUrl,
    /// 热键绑定行（按行下标）。
    Hotkey(usize),
    /// 任务默认模型（禁换行）。
    Model(TaskKind),
}

/// 校验错误的类型化形态：只记「哪里错了、错成什么样」，措辞归文案表。
///
/// 变体与 `[gloss_settings.error]` 的键一一对应（见 [`Self::message`]），
/// 校验输出因此与语言无关——同一份草稿在任何 locale 下得出同一张错误表。
#[derive(Debug, Clone, PartialEq, Eq)]
enum FieldError {
    /// 该触发键与更早的行重复；`line` 是那条绑定的 1 起数行号。
    DuplicateHotkey {
        /// 先占用该组合的行号（1 起数）。
        line: usize,
    },
    /// 触发键为空。
    EmptyTrigger,
    /// 触发键语法不合法，`trigger` 是用户原样输入。
    InvalidTrigger {
        /// 用户输入的原样回显。
        trigger: String,
    },
    /// 模型名含换行。
    NewlineInModel,
    /// Base URL 为空。
    BaseUrlEmpty,
    /// Base URL 不是合法地址（语法不合法或非 https）。
    BaseUrlNotHttps,
    /// Base URL 内嵌账号密码。
    BaseUrlCredentials,
    /// Base URL 携带查询参数或锚点。
    BaseUrlQuery,
}

impl FieldError {
    /// 就地提示文案。
    fn message(&self, text: &Text) -> String {
        match self {
            Self::DuplicateHotkey { line } => {
                let line = line.to_string();
                fill(
                    &text.gloss_settings_error_duplicate_hotkey,
                    &[("line", &line)],
                )
            }
            Self::EmptyTrigger => text.gloss_settings_error_empty_trigger.clone(),
            Self::InvalidTrigger { trigger } => fill(
                &text.gloss_settings_error_invalid_trigger,
                &[("trigger", trigger)],
            ),
            Self::NewlineInModel => text.gloss_settings_error_newline_in_model.clone(),
            Self::BaseUrlEmpty => text.gloss_settings_error_base_url_empty.clone(),
            Self::BaseUrlNotHttps => text.gloss_settings_error_base_url_invalid.clone(),
            Self::BaseUrlCredentials => text.gloss_settings_error_base_url_credentials.clone(),
            Self::BaseUrlQuery => text.gloss_settings_error_base_url_query.clone(),
        }
    }
}

/// 设置窗口的一个编辑会话：打开时以当前快照建草稿，保存/关闭由壳销毁。
pub struct SettingsState {
    /// 编辑中的整份配置；「保存」时整体上交（整份快照语义）。
    draft: Config,
    /// API key 输入框；只在保存时交给壳写 keychain，永不进 `draft`。
    api_key: String,
    /// 是否已标记「清除密钥」（保存时才真正删除；重新输入即撤销）。
    clear_key: bool,
    /// 壳回写的提示（保存失败等）；文案在渲染帧落地。
    notice: Option<SettingsNotice>,
    /// 是否已进入校验态：首次点「保存」置位，此后每帧就地标注错误；
    /// 置位前编辑不打扰。
    validated: bool,
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

/// 壳回写的用户提示：三种失败各有各的措辞，具体错因按 [`GlossError`] 变体
/// 带进来（不预拼英文 `Display` 字符串——那是诊断文本，改它不该改界面）。
#[derive(Debug, Clone, PartialEq)]
pub enum SettingsNotice {
    /// 密钥写入失败，配置因此没有保存。
    KeyUpdateFailed(GlossError),
    /// 配置落盘失败（密钥未改动）。
    SaveFailed(GlossError),
    /// 密钥已生效，但配置落盘失败。
    KeyUpdatedSaveFailed(GlossError),
}

impl SettingsNotice {
    /// 提示文案：前缀说明场合，`{{detail}}` 填错因的本地化措辞。
    fn message(&self, text: &Text) -> String {
        let (template, error) = match self {
            Self::KeyUpdateFailed(err) => (&text.gloss_settings_notice_key_update_failed, err),
            Self::SaveFailed(err) => (&text.gloss_settings_notice_save_failed, err),
            Self::KeyUpdatedSaveFailed(err) => {
                (&text.gloss_settings_notice_key_updated_save_failed, err)
            }
        };
        fill(template, &[("detail", &text.for_error_detail(error))])
    }
}

/// 打开一个编辑会话：草稿取自当前快照（打开后的配置变更不跟读，保存即
/// 整份覆盖——与「单次任务内配置一致」同一取舍）。
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
    /// 壳回写提示（保存失败等）；在下一次渲染帧按当前 locale 展示。
    pub fn report(&mut self, notice: SettingsNotice) {
        self.notice = Some(notice);
    }

    /// 当前草稿（壳只读：未保存变更提示等）。
    pub fn draft(&self) -> &Config {
        &self.draft
    }

    /// 最近一条壳回写的提示。
    pub fn notice(&self) -> Option<&SettingsNotice> {
        self.notice.as_ref()
    }

    /// 逐字段校验结果：每帧从草稿重算，不存陈旧错误。未进入校验态时
    /// 返回空表（编辑中不标注）。
    fn errors(&self) -> HashMap<FieldKey, FieldError> {
        if self.validated {
            validate_draft(&self.draft)
        } else {
            HashMap::new()
        }
    }
}

/// 「保存」的上交物：草稿先全量校验——有错则不落盘、就地标注并汇总统
/// 计；无错才走整份快照 + 密钥变更（清除标记优先，其次输入框内容，都
/// 为空则保持原密钥）。
fn build_save(state: &mut SettingsState) -> SettingsAction {
    // 保存尝试即进入校验态并保持：壳侧落盘失败保留会话时，后续编辑仍
    // 实时复检，不会退出错误模式。
    state.validated = true;
    let errors = validate_draft(&state.draft);
    if !errors.is_empty() {
        return SettingsAction::Idle;
    }
    let mut draft = state.draft.clone();
    draft.base_url = draft.base_url.trim().to_owned();
    for binding in &mut draft.model_by_kind {
        binding.model = binding.model.trim().to_owned();
    }
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

/// 逐字段校验（纯逻辑）：规则单点下沉 gloss-core，这里只做组合与
/// [`FieldError`] 映射（文案留给渲染帧）。
fn validate_draft(draft: &Config) -> HashMap<FieldKey, FieldError> {
    let mut errors = HashMap::new();
    if let Err(err) = validate_base_url(&draft.base_url) {
        errors.insert(FieldKey::BaseUrl, base_url_error(&err));
    }
    // 热键：语法 + 规范串去重（先到者保留，后者按重复报）。
    let mut seen: HashMap<String, usize> = HashMap::new();
    for (index, binding) in draft.hotkey_bindings.iter().enumerate() {
        match parse_trigger(&binding.trigger) {
            Ok(parsed) => {
                let canonical = parsed.canonical();
                if let Some(&first) = seen.get(&canonical) {
                    errors.insert(
                        FieldKey::Hotkey(index),
                        FieldError::DuplicateHotkey { line: first + 1 },
                    );
                } else {
                    seen.insert(canonical, index);
                }
            }
            Err(gloss_core::hotkey::TriggerError::Empty) => {
                errors.insert(FieldKey::Hotkey(index), FieldError::EmptyTrigger);
            }
            Err(_) => {
                errors.insert(
                    FieldKey::Hotkey(index),
                    FieldError::InvalidTrigger {
                        trigger: binding.trigger.clone(),
                    },
                );
            }
        }
    }
    // 模型名：保存时 trim，禁换行（粘贴事故防护）；空 = 用内置默认，合法。
    for binding in &draft.model_by_kind {
        if binding.model.trim().contains('\n') {
            errors.insert(FieldKey::Model(binding.kind), FieldError::NewlineInModel);
        }
    }
    errors
}

/// Base URL 校验错误 → 字段错误（规则与提示的分界在这里）。
fn base_url_error(err: &BaseUrlError) -> FieldError {
    match err {
        BaseUrlError::Empty => FieldError::BaseUrlEmpty,
        BaseUrlError::Invalid | BaseUrlError::NotHttps => FieldError::BaseUrlNotHttps,
        BaseUrlError::EmbeddedCredentials => FieldError::BaseUrlCredentials,
        BaseUrlError::QueryOrFragment => FieldError::BaseUrlQuery,
    }
}

/// 画一帧设置窗口，返回本帧用户上交的动作。`text` 是当前 locale 的文案表
/// （由壳按帧给），本函数不探测语言。
///
/// 自下而上布局：动作行钉在窗口底部（保存主按钮右对齐），提示在其上，
/// 其余全部区块进滚动区——内容再长也不会把「保存」推出视口。
/// 窗口内边距由 [`WINDOW_PADDING`] 统一给出；整幅先铺 `window_fill`
/// 底色（设置窗不透明，清屏色不随主题，底色必须由 egui 自己画，
/// 深浅主题切换才连同文字一起翻转）。
pub(crate) fn draw(ui: &mut egui::Ui, state: &mut SettingsState, text: &Text) -> SettingsAction {
    let mut action = SettingsAction::Idle;
    let errors = state.errors();
    egui::Frame::new()
        .fill(ui.visuals().window_fill)
        .inner_margin(egui::Margin::same(WINDOW_PADDING))
        .show(ui, |ui| {
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                // bottom_up 会渗进子 Ui：滚动内容显式转回 top_down，区块才
                // 从顶部开始排列。
                action_row(ui, state, &mut action, text);
                notices(ui, state, &errors, text);
                ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        section(ui, &text.gloss_settings_section_model, |ui| {
                            connection_section(ui, state, &errors, text);
                        });
                        ui.add_space(space::SECTION);
                        section(ui, &text.gloss_settings_section_task, |ui| {
                            task_section(ui, state, &errors, text);
                        });
                        ui.add_space(space::SECTION);
                        section(ui, &text.gloss_settings_section_hotkey, |ui| {
                            hotkey_section(ui, state, &errors, text);
                        });
                        ui.add_space(space::SECTION);
                        section(ui, &text.gloss_settings_section_general, |ui| {
                            general_section(ui, state, text);
                        });
                    });
                });
            });
        });
    action
}

/// 设置窗口内边距（窗口私有量，docs/14 定版 16）。
const WINDOW_PADDING: i8 = 16;

/// 动作行：取消（次按钮）+ 保存（ACCENT 主按钮），右对齐。
fn action_row(
    ui: &mut egui::Ui,
    state: &mut SettingsState,
    action: &mut SettingsAction,
    text: &Text,
) {
    // 先分配固定行高再右对齐：bottom_up 里直接 with_layout(RTL) 的子区域
    // 会撑满剩余整高，按钮被垂直居中到窗口中部，滚动区被挤剩一条。
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), ACTION_ROW_HEIGHT),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            let save = ui.add(
                egui::Button::new(
                    RichText::new(text.gloss_settings_save.as_str()).color(egui::Color32::WHITE),
                )
                .fill(color::ACCENT)
                .corner_radius(egui::CornerRadius::same(6)),
            );
            if save.clicked() {
                *action = build_save(state);
            }
            if ui.button(text.gloss_settings_cancel.as_str()).clicked() {
                *action = SettingsAction::Close;
            }
        },
    );
}

/// 动作行高度（按钮高 + 上下留白）。
const ACTION_ROW_HEIGHT: f32 = 30.0;

/// 提示行：校验汇总（进入校验态且有错）与壳回写提示（保存失败等）。
fn notices(
    ui: &mut egui::Ui,
    state: &SettingsState,
    errors: &HashMap<FieldKey, FieldError>,
    text: &Text,
) {
    if state.validated && !errors.is_empty() {
        let count = errors.len().to_string();
        ui.add_space(space::TIGHT);
        ui.label(
            RichText::new(fill(
                &text.gloss_settings_invalid_summary,
                &[("count", &count)],
            ))
            .size(font::CAPTION)
            .color(color::DANGER),
        );
    }
    if let Some(notice) = &state.notice {
        ui.add_space(space::TIGHT);
        ui.label(
            RichText::new(notice.message(text))
                .size(font::CAPTION)
                .color(color::DANGER),
        );
    }
}

/// 分区：区块标（弱色小字）+ faint_bg 卡片。
fn section(ui: &mut egui::Ui, title: &str, content: impl FnOnce(&mut egui::Ui)) {
    caption(ui, title);
    ui.add_space(space::TIGHT);
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(egui::CornerRadius::same(radius::CARD))
        .inner_margin(egui::Margin::same(space::CARD_PADDING))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            content(ui);
        });
}

/// 弱色小字（区块标 / 子块标 / 说明提示共用一档）。
fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(font::CAPTION)
            .color(ui.visuals().weak_text_color()),
    );
}

/// 设置页输入框统一入口：浅色主题下白底输入框与近白卡片几乎同色，
/// 补一圈浅灰描边保持可见；深色主题输入框本身比卡片暗，不加描边。
fn add_input<'t>(
    ui: &mut egui::Ui,
    text: &'t mut String,
    build: impl FnOnce(egui::TextEdit<'t>) -> egui::TextEdit<'t>,
) -> egui::Response {
    ui.scope(|ui| {
        if !ui.visuals().dark_mode {
            ui.visuals_mut().widgets.inactive.bg_stroke =
                Stroke::new(1.0, egui::Color32::from_gray(0xE2));
        }
        ui.add(build(egui::TextEdit::singleline(text)))
    })
    .inner
}

/// 字段错误态：控件底边 DANGER 下划线。
fn underline_if_error(
    ui: &mut egui::Ui,
    response: &egui::Response,
    errors: &HashMap<FieldKey, FieldError>,
    key: FieldKey,
) {
    if errors.contains_key(&key) {
        let rect = response.rect;
        ui.painter().line_segment(
            [
                egui::pos2(rect.left(), rect.bottom()),
                egui::pos2(rect.right(), rect.bottom()),
            ],
            Stroke::new(1.5, color::DANGER),
        );
    }
}

/// 就地错误提示（控件正下方，CAPTION DANGER）。
fn error_text(
    ui: &mut egui::Ui,
    errors: &HashMap<FieldKey, FieldError>,
    key: FieldKey,
    text: &Text,
) {
    if let Some(error) = errors.get(&key) {
        ui.label(
            RichText::new(error.message(text))
                .size(font::CAPTION)
                .color(color::DANGER),
        );
    }
}

/// 模型区：Base URL + API Key（写 keychain，不进配置）。
fn connection_section(
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
            // 重新输入即撤销「清除」意图。
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
}

/// 清除密钥按钮的占位余量（按钮宽 + 间距；输入框占满剩余宽）。
const CLEAR_BUTTON_RESERVE: f32 = 88.0;

/// 任务区：默认任务、目标语言、任务开关（左右开关钮）、每任务默认模型。
fn task_section(
    ui: &mut egui::Ui,
    state: &mut SettingsState,
    errors: &HashMap<FieldKey, FieldError>,
    text: &Text,
) {
    choice_row(ui, &text.gloss_settings_default_kind, |ui| {
        kind_combo(
            ui,
            "default_text_kind",
            &mut state.draft.default_text_kind,
            &TEXT_KINDS,
            text,
        );
    });
    choice_hint(ui, &text.gloss_settings_default_kind_hint);
    choice_row(ui, &text.gloss_settings_target_lang, |ui| {
        lang_combo(ui, &mut state.draft.target_lang, text);
    });

    ui.add_space(space::TIGHT);
    caption(ui, &text.gloss_settings_kind_switch);
    caption(ui, &text.gloss_settings_kind_switch_hint);
    for kind in ALL_KINDS {
        let enabled = state.draft.is_kind_enabled(kind);
        if switch_row(ui, kind_label(kind, text), enabled, text) {
            state.draft.set_kind_enabled(kind, !enabled);
        }
    }

    ui.add_space(space::TIGHT);
    caption(ui, &text.gloss_settings_default_model);
    caption(ui, &text.gloss_settings_default_model_hint);
    for kind in ALL_KINDS {
        let mut model = state
            .draft
            .model_for_kind(kind)
            .unwrap_or_default()
            .to_owned();
        ui.label(kind_label(kind, text));
        let response = add_input(ui, &mut model, |e| {
            let edit = e.desired_width(f32::INFINITY);
            if kind.accepts_text() {
                edit
            } else {
                edit.hint_text(text.gloss_settings_vision_model_hint.as_str())
            }
        });
        underline_if_error(ui, &response, errors, FieldKey::Model(kind));
        error_text(ui, errors, FieldKey::Model(kind), text);
        if response.changed() {
            state.draft.set_model_for_kind(kind, &model);
        }
        ui.add_space(space::TIGHT);
    }
}

/// 开关行：任务名左、开关钮右；点击切换。开关的可访问标签是
/// 「启用{任务名}」（模板见文案表）——与可见文本区分，读屏与测试按它
/// 定位且不与裸任务名重名。返回是否被点击。
fn switch_row(ui: &mut egui::Ui, label: &str, enabled: bool, text: &Text) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (rect, response) = ui.allocate_exact_size(vec2(40.0, 24.0), egui::Sense::click());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    fill(&text.gloss_settings_switch_label, &[("kind", label)]),
                )
            });
            let track_fill = if enabled {
                color::ACCENT
            } else {
                ui.visuals().widgets.inactive.bg_fill
            };
            ui.painter().rect_filled(rect, 12.0, track_fill);
            let knob_x = if enabled {
                rect.right() - 12.0
            } else {
                rect.left() + 12.0
            };
            ui.painter().circle_filled(
                egui::pos2(knob_x, rect.center().y),
                10.0,
                egui::Color32::WHITE,
            );
            response.clicked()
        })
        .inner
    })
    .inner
}

/// 热键区：绑定表就地编辑（左右结构——触发键左、任务下拉右）。
fn hotkey_section(
    ui: &mut egui::Ui,
    state: &mut SettingsState,
    errors: &HashMap<FieldKey, FieldError>,
    text: &Text,
) {
    caption(ui, &text.gloss_settings_hotkey_hint);
    for index in 0..state.draft.hotkey_bindings.len() {
        let key = FieldKey::Hotkey(index);
        ui.horizontal(|ui| {
            let Some(binding) = state.draft.hotkey_bindings.get_mut(index) else {
                return;
            };
            let HotkeyBinding { trigger, kind, .. } = binding;
            let response = add_input(ui, trigger, |e| {
                e.desired_width(110.0).font(egui::TextStyle::Monospace)
            });
            underline_if_error(ui, &response, errors, key);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                kind_combo(ui, &format!("hotkey_kind_{index}"), kind, &ALL_KINDS, text);
            });
        });
        error_text(ui, errors, key, text);
        ui.add_space(space::ITEM);
    }
}

/// 通用区：界面语言、界面主题、缓存有效期（上限由控件钳制）。
fn general_section(ui: &mut egui::Ui, state: &mut SettingsState, text: &Text) {
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
}

/// 双列行：行标签左、控件推到卡片右缘（两端对齐）。
fn choice_row(ui: &mut egui::Ui, label: &str, control: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            control(ui);
        });
    });
}

/// 双列行的说明提示（左对齐，CAPTION 弱色）。
fn choice_hint(ui: &mut egui::Ui, text: &str) {
    caption(ui, text);
}

/// 划词可服务的任务类型（与 `Config::selection_task_kind` 的收口一致）。
const TEXT_KINDS: [TaskKind; 3] = [
    TaskKind::TranslateWord,
    TaskKind::TranslateSentence,
    TaskKind::ExplainCode,
];

/// 任务类型下拉；`id_salt` 需在窗口内唯一。
fn kind_combo(
    ui: &mut egui::Ui,
    id_salt: &str,
    current: &mut TaskKind,
    choices: &[TaskKind],
    text: &Text,
) {
    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(kind_label(*current, text))
        .show_ui(ui, |ui| {
            for &kind in choices {
                ui.selectable_value(current, kind, kind_label(kind, text));
            }
        });
}

/// UI 固定可选的 5 语种（`Lang::Other` 只经配置文件到达，不在下拉里）。
const LANG_CHOICES: fn() -> [Lang; 5] = || [Lang::Zh, Lang::En, Lang::Ja, Lang::Ko, Lang::Fr];

/// 语言标签：产物语言（译文给谁看），与界面语言分属两张表。
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

/// 目标语言下拉。
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

/// 界面语言标签（三态偏好，落定见 `Language::resolve`）。
fn language_label<'a>(language: &Language, text: &'a Text) -> &'a str {
    match language {
        Language::System => &text.gloss_ui_language_system,
        Language::Zh => &text.gloss_ui_language_zh,
        Language::En => &text.gloss_ui_language_en,
    }
}

/// 界面语言下拉：本项同时决定 prompt 模板语言（`Language::resolve`）与本表
/// 的选表依据；保存后下一次触发与下一帧界面即生效。
fn language_combo(ui: &mut egui::Ui, current: &mut Language, text: &Text) {
    egui::ComboBox::from_id_salt("ui_language")
        .selected_text(language_label(current, text))
        .show_ui(ui, |ui| {
            for language in [Language::System, Language::Zh, Language::En] {
                ui.selectable_value(current, language, language_label(&language, text));
            }
        });
}

/// 主题标签与下拉（消费在壳侧，见 `app::theme`）。
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

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    use egui_kittest::kittest::Queryable;
    use gloss_core::config_handle::ConfigHandle;
    use gloss_core::model::Locale;
    use gloss_core::task::TaskKind;

    use crate::stubs::ports::MemoryConfigStore;

    use super::*;

    #[test]
    fn field_errors_are_worded_per_locale() {
        let zh = Text::get(Locale::Zh);
        let en = Text::get(Locale::En);
        for (error, zh_message, en_message) in [
            (
                FieldError::DuplicateHotkey { line: 2 },
                "与第 2 行重复",
                "Duplicate of line 2",
            ),
            (
                FieldError::EmptyTrigger,
                "触发键不能为空",
                "Hotkey cannot be empty",
            ),
            (
                FieldError::InvalidTrigger {
                    trigger: "Cmd+".into(),
                },
                "无法解析触发键「Cmd+」",
                "Cannot parse hotkey \"Cmd+\"",
            ),
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
        state
            .draft
            .set_model_for_kind(TaskKind::TranslateWord, "m2");

        let SettingsAction::Save { config, key } = build_save(&mut state) else {
            panic!("save expected");
        };
        assert_eq!(key, KeyUpdate::Replace("sk-test".into()));
        assert_eq!(config.model_for_kind(TaskKind::TranslateWord), Some("m2"));
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
    fn duplicate_hotkey_triggers_are_flagged_by_canonical_form() {
        let mut state = open(&Config::default());
        state.draft.hotkey_bindings[1].trigger =
            state.draft.hotkey_bindings[0].trigger.to_ascii_lowercase();

        let errors = validate_draft(&state.draft);
        assert!(
            errors.contains_key(&FieldKey::Hotkey(1)),
            "same combination in a different spelling must be flagged"
        );
        assert!(
            !errors.contains_key(&FieldKey::Hotkey(0)),
            "the first binding keeps the combination"
        );
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
        let action = Rc::new(RefCell::new(SettingsAction::Idle));
        let sink = Rc::clone(&action);
        let text = Text::get(locale);
        let mut state = state;
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            let frame_action = draw(ui, &mut state, text);
            if frame_action != SettingsAction::Idle {
                *sink.borrow_mut() = frame_action;
            }
        });
        harness.set_size(egui::vec2(460.0, 1200.0));
        (harness, action)
    }

    #[test]
    fn english_catalog_relabels_the_settings_window() {
        let (mut harness, _action) = harness_for(open(&Config::default()), Locale::En);
        harness.run();
        for label in [
            "Model",
            "Tasks",
            "Hotkeys",
            "General",
            "Save",
            "Cancel",
            "Default task",
            "Target language",
            "Task switches",
            "Clear key",
            "Interface language",
            "Interface theme",
            "Cache lifetime",
            "Enable Word card",
        ] {
            harness.get_by_label(label);
        }
        assert!(
            harness.query_by_label("保存").is_none(),
            "the English table must not leave Chinese labels behind"
        );
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
        for label in [
            "模型",
            "Base URL",
            "API Key",
            "任务",
            "默认任务",
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

    #[test]
    fn task_toggle_flips_enabled_kinds() {
        let (mut harness, action) = harness_for(open(&Config::default()), Locale::Zh);
        harness.run();
        harness.get_by_label("启用词卡").click_accesskit();
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
    fn hotkey_rows_expose_their_triggers() {
        let (mut harness, _action) = harness_for(open(&Config::default()), Locale::Zh);
        harness.run();
        for trigger in ["Cmd+Shift+D", "Cmd+Shift+F", "Cmd+Shift+E"] {
            assert!(
                harness.get_all_by_value(trigger).next().is_some(),
                "trigger `{trigger}` must be an editable row"
            );
        }
    }

    #[test]
    fn snapshots_match_baseline() {
        let mut results = egui_kittest::SnapshotResults::new();
        let (mut harness, _action) = harness_for(open(&Config::default()), Locale::Zh);
        harness.run();
        harness.snapshot("settings_main");
        results.extend_harness(&mut harness);

        let mut noticed = open(&Config::default());
        noticed.report(SettingsNotice::SaveFailed(GlossError::Config(
            "disk on fire".into(),
        )));
        let (mut harness, _action) = harness_for(noticed, Locale::Zh);
        harness.run();
        harness.get_by_label_contains("disk on fire");
        harness.snapshot("settings_notice");
        results.extend_harness(&mut harness);

        let mut invalid = open(&Config::default());
        invalid.draft.base_url = "htp://api.example.com".into();
        let (mut harness, _action) = harness_for(invalid, Locale::Zh);
        harness.run();
        harness.get_by_label("保存").click();
        harness.run();
        harness.get_by_label_contains("已就地标红");
        harness.snapshot("settings_invalid");
        results.extend_harness(&mut harness);
        results.unwrap();
    }
}
