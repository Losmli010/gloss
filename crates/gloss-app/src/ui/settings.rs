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

use std::collections::HashMap;

use egui::{RichText, ScrollArea, Stroke, vec2};
use gloss_core::config::{
    ALL_KINDS, BaseUrlError, CACHE_TTL_MAX_SECS, Config, Language, Theme, validate_base_url,
};
use gloss_core::hotkey::parse_trigger;
use gloss_core::model::Lang;
use gloss_core::task::{HotkeyBinding, TaskKind};

use super::kind_label;
use super::style::{color, font, radius, space};

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

    /// 逐字段校验结果：每帧从草稿重算，不存陈旧错误。未进入校验态时
    /// 返回空表（编辑中不标注）。
    fn errors(&self) -> HashMap<FieldKey, String> {
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

/// 逐字段校验（纯逻辑）：规则单点下沉 gloss-core，这里只做组合与展示
/// 文案映射。
fn validate_draft(draft: &Config) -> HashMap<FieldKey, String> {
    let mut errors = HashMap::new();
    if let Err(err) = validate_base_url(&draft.base_url) {
        errors.insert(FieldKey::BaseUrl, base_url_hint(&err).to_owned());
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
                        format!("与第 {} 行重复", first + 1),
                    );
                } else {
                    seen.insert(canonical, index);
                }
            }
            Err(gloss_core::hotkey::TriggerError::Empty) => {
                errors.insert(FieldKey::Hotkey(index), "触发键不能为空".to_owned());
            }
            Err(_) => {
                errors.insert(
                    FieldKey::Hotkey(index),
                    format!("无法解析触发键「{}」", binding.trigger),
                );
            }
        }
    }
    // 模型名：保存时 trim，禁换行（粘贴事故防护）；空 = 用内置默认，合法。
    for binding in &draft.model_by_kind {
        if binding.model.trim().contains('\n') {
            errors.insert(FieldKey::Model(binding.kind), "不能包含换行".to_owned());
        }
    }
    errors
}

/// Base URL 校验错误 → 就地提示文案。
fn base_url_hint(err: &BaseUrlError) -> &'static str {
    match err {
        BaseUrlError::Empty => "请填写服务地址",
        BaseUrlError::Invalid | BaseUrlError::NotHttps => "不是合法地址：应以 https:// 开头",
        BaseUrlError::EmbeddedCredentials => "不能内嵌账号密码",
        BaseUrlError::QueryOrFragment => "不能携带查询参数或锚点",
    }
}

/// 画一帧设置窗口，返回本帧用户上交的动作。
///
/// 自下而上布局：动作行钉在窗口底部（保存主按钮右对齐），提示在其上，
/// 其余全部区块进滚动区——内容再长也不会把「保存」推出视口。
/// 窗口内边距由 [`WINDOW_PADDING`] 统一给出；整幅先铺 `window_fill`
/// 底色（设置窗不透明，清屏色不随主题，底色必须由 egui 自己画，
/// 深浅主题切换才连同文字一起翻转）。
pub fn draw(ui: &mut egui::Ui, state: &mut SettingsState) -> SettingsAction {
    let mut action = SettingsAction::Idle;
    let errors = state.errors();
    egui::Frame::new()
        .fill(ui.visuals().window_fill)
        .inner_margin(egui::Margin::same(WINDOW_PADDING))
        .show(ui, |ui| {
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                // bottom_up 会渗进子 Ui：滚动内容显式转回 top_down，区块才
                // 从顶部开始排列。
                action_row(ui, state, &mut action);
                notices(ui, state, &errors);
                ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        section(ui, "模型", |ui| connection_section(ui, state, &errors));
                        ui.add_space(space::SECTION);
                        section(ui, "任务", |ui| task_section(ui, state, &errors));
                        ui.add_space(space::SECTION);
                        section(ui, "热键", |ui| hotkey_section(ui, state, &errors));
                        ui.add_space(space::SECTION);
                        section(ui, "通用", |ui| general_section(ui, state));
                    });
                });
            });
        });
    action
}

/// 设置窗口内边距（窗口私有量，docs/14 定版 16）。
const WINDOW_PADDING: i8 = 16;

/// 动作行：取消（次按钮）+ 保存（ACCENT 主按钮），右对齐。
fn action_row(ui: &mut egui::Ui, state: &mut SettingsState, action: &mut SettingsAction) {
    // 先分配固定行高再右对齐：bottom_up 里直接 with_layout(RTL) 的子区域
    // 会撑满剩余整高，按钮被垂直居中到窗口中部，滚动区被挤剩一条。
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), ACTION_ROW_HEIGHT),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            let save = ui.add(
                egui::Button::new(RichText::new("保存").color(egui::Color32::WHITE))
                    .fill(color::ACCENT)
                    .corner_radius(egui::CornerRadius::same(6)),
            );
            if save.clicked() {
                *action = build_save(state);
            }
            if ui.button("取消").clicked() {
                *action = SettingsAction::Close;
            }
        },
    );
}

/// 动作行高度（按钮高 + 上下留白）。
const ACTION_ROW_HEIGHT: f32 = 30.0;

/// 提示行：校验汇总（进入校验态且有错）与壳回写提示（保存失败等）。
fn notices(ui: &mut egui::Ui, state: &SettingsState, errors: &HashMap<FieldKey, String>) {
    if state.validated && !errors.is_empty() {
        ui.add_space(space::TIGHT);
        ui.label(
            RichText::new(format!("有 {} 处输入未通过校验，已就地标红", errors.len()))
                .size(font::CAPTION)
                .color(color::DANGER),
        );
    }
    if let Some(notice) = &state.notice {
        ui.add_space(space::TIGHT);
        ui.label(
            RichText::new(notice.as_str())
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
    errors: &HashMap<FieldKey, String>,
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
fn error_text(ui: &mut egui::Ui, errors: &HashMap<FieldKey, String>, key: FieldKey) {
    if let Some(message) = errors.get(&key) {
        ui.label(
            RichText::new(message.clone())
                .size(font::CAPTION)
                .color(color::DANGER),
        );
    }
}

/// 模型区：Base URL + API Key（写 keychain，不进配置）。
fn connection_section(
    ui: &mut egui::Ui,
    state: &mut SettingsState,
    errors: &HashMap<FieldKey, String>,
) {
    ui.label("Base URL");
    let response = add_input(ui, &mut state.draft.base_url, |e| {
        e.hint_text("OpenAI 兼容服务地址")
            .desired_width(f32::INFINITY)
    });
    underline_if_error(ui, &response, errors, FieldKey::BaseUrl);
    error_text(ui, errors, FieldKey::BaseUrl);
    ui.add_space(space::ITEM);

    ui.label("API Key");
    ui.horizontal(|ui| {
        let input_width = ui.available_width() - CLEAR_BUTTON_RESERVE;
        let typed = add_input(ui, &mut state.api_key, |e| {
            e.password(true)
                .hint_text(if state.clear_key {
                    "保存后清除"
                } else {
                    "留空则不修改"
                })
                .desired_width(input_width)
        })
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
}

/// 清除密钥按钮的占位余量（按钮宽 + 间距；输入框占满剩余宽）。
const CLEAR_BUTTON_RESERVE: f32 = 88.0;

/// 任务区：默认任务、目标语言、任务开关（左右开关钮）、每任务默认模型。
fn task_section(ui: &mut egui::Ui, state: &mut SettingsState, errors: &HashMap<FieldKey, String>) {
    choice_row(ui, "默认任务", |ui| {
        kind_combo(
            ui,
            "default_text_kind",
            &mut state.draft.default_text_kind,
            &TEXT_KINDS,
        );
    });
    choice_hint(ui, "划词触发时使用的任务");
    choice_row(ui, "目标语言", |ui| {
        lang_combo(ui, &mut state.draft.target_lang);
    });

    ui.add_space(space::TIGHT);
    caption(ui, "任务开关");
    caption(ui, "关闭后该任务不再触发（含热键）");
    for kind in ALL_KINDS {
        let enabled = state.draft.is_kind_enabled(kind);
        if switch_row(ui, kind_label(kind), enabled) {
            state.draft.set_kind_enabled(kind, !enabled);
        }
    }

    ui.add_space(space::TIGHT);
    caption(ui, "默认模型");
    caption(ui, "留空使用内置默认模型");
    for kind in ALL_KINDS {
        let mut model = state
            .draft
            .model_for_kind(kind)
            .unwrap_or_default()
            .to_owned();
        ui.label(kind_label(kind));
        let response = add_input(ui, &mut model, |e| {
            let edit = e.desired_width(f32::INFINITY);
            if kind.accepts_text() {
                edit
            } else {
                edit.hint_text("视觉模型（M5）")
            }
        });
        underline_if_error(ui, &response, errors, FieldKey::Model(kind));
        error_text(ui, errors, FieldKey::Model(kind));
        if response.changed() {
            state.draft.set_model_for_kind(kind, &model);
        }
        ui.add_space(space::TIGHT);
    }
}

/// 开关行：任务名左、开关钮右；点击切换。开关的可访问标签是
/// 「启用{任务名}」——与可见文本区分，读屏与测试按它定位且不与裸
/// 任务名重名。返回是否被点击。
fn switch_row(ui: &mut egui::Ui, label: &str, enabled: bool) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (rect, response) = ui.allocate_exact_size(vec2(40.0, 24.0), egui::Sense::click());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("启用{label}"))
            });
            let fill = if enabled {
                color::ACCENT
            } else {
                ui.visuals().widgets.inactive.bg_fill
            };
            ui.painter().rect_filled(rect, 12.0, fill);
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
    errors: &HashMap<FieldKey, String>,
) {
    caption(ui, "选中文字后按下，用指定任务处理当前选区");
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
                kind_combo(ui, &format!("hotkey_kind_{index}"), kind, &ALL_KINDS);
            });
        });
        error_text(ui, errors, key);
        ui.add_space(space::ITEM);
    }
}

/// 通用区：界面语言、界面主题、缓存有效期（上限由控件钳制）。
fn general_section(ui: &mut egui::Ui, state: &mut SettingsState) {
    choice_row(ui, "界面语言", |ui| {
        language_combo(ui, &mut state.draft.language);
    });
    choice_row(ui, "界面主题", |ui| {
        theme_combo(ui, &mut state.draft.theme);
    });
    choice_row(ui, "缓存有效期", |ui| {
        ui.add(
            egui::DragValue::new(&mut state.draft.cache_ttl_secs)
                .range(0..=CACHE_TTL_MAX_SECS)
                .suffix(" 秒"),
        );
    });
    choice_hint(ui, "相同内容的结果直接复用；0 = 永不失效，退出即清空");
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

/// 界面语言标签。
fn language_label(language: &Language) -> &'static str {
    match language {
        Language::System => "跟随系统",
        Language::Zh => "简体中文",
        Language::En => "English",
    }
}

/// 界面语言下拉（接线归 prompt locale 与 UI 文案翻译：本版仅持久化）。
fn language_combo(ui: &mut egui::Ui, current: &mut Language) {
    egui::ComboBox::from_id_salt("ui_language")
        .selected_text(language_label(current))
        .show_ui(ui, |ui| {
            for (language, label) in [
                (Language::System, "跟随系统"),
                (Language::Zh, "简体中文"),
                (Language::En, "English"),
            ] {
                ui.selectable_value(current, language, label);
            }
        });
}

/// 主题标签与下拉（消费在壳侧，见 `app::theme`）。
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

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use egui_kittest::kittest::Queryable;
    use gloss_core::task::TaskKind;

    use super::*;

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
        let errors = state.errors();
        let error = errors
            .get(&FieldKey::BaseUrl)
            .expect("the invalid field must be flagged");
        assert!(error.contains("https"), "hint must name the https rule");
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
        state.report("保存失败".into());
        assert_eq!(state.notice.as_deref(), Some("保存失败"));
    }

    fn harness_for(
        mut state: SettingsState,
    ) -> (egui_kittest::Harness<'static>, Rc<RefCell<SettingsAction>>) {
        let action = Rc::new(RefCell::new(SettingsAction::Idle));
        let sink = Rc::clone(&action);
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            let frame_action = draw(ui, &mut state);
            if frame_action != SettingsAction::Idle {
                *sink.borrow_mut() = frame_action;
            }
        });
        harness.set_size(egui::vec2(460.0, 1200.0));
        (harness, action)
    }

    #[test]
    fn all_sections_render_and_save_submits_the_draft() {
        let (mut harness, action) = harness_for(open(&Config::default()));
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
        let (mut harness, action) = harness_for(open(&Config::default()));
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
        let (mut harness, action) = harness_for({
            let mut state = open(&Config::default());
            state.draft.base_url = "htp://api.example.com".into();
            state
        });
        harness.run();
        harness.get_by_label("保存").click();
        harness.run();
        assert!(
            matches!(&*action.borrow(), SettingsAction::Idle),
            "an invalid draft must not submit a save"
        );
        harness.get_by_label_contains("应以 https:// 开头");
        harness.get_by_label_contains("已就地标红");
    }

    #[test]
    fn cancel_and_clear_key_actions_are_submitted() {
        let (mut harness, action) = harness_for(open(&Config::default()));
        harness.run();
        harness.get_by_label("取消").click();
        harness.run();
        assert_eq!(*action.borrow(), SettingsAction::Close);

        let (mut harness, _action) = harness_for(open(&Config::default()));
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
        let (mut harness, _action) = harness_for(open(&Config::default()));
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
        let (mut harness, _action) = harness_for(open(&Config::default()));
        harness.run();
        harness.snapshot("settings_main");
        results.extend_harness(&mut harness);

        let mut noticed = open(&Config::default());
        noticed.report("保存失败：disk on fire".into());
        let (mut harness, _action) = harness_for(noticed);
        harness.run();
        harness.get_by_label_contains("disk on fire");
        harness.snapshot("settings_notice");
        results.extend_harness(&mut harness);

        let mut invalid = open(&Config::default());
        invalid.draft.base_url = "htp://api.example.com".into();
        let (mut harness, _action) = harness_for(invalid);
        harness.run();
        // 走可观察路径进入错误态：点保存被阻断，等同真实用户操作。
        harness.get_by_label("保存").click();
        harness.run();
        harness.get_by_label_contains("已就地标红");
        harness.snapshot("settings_invalid");
        results.extend_harness(&mut harness);
        results.unwrap();
    }
}
