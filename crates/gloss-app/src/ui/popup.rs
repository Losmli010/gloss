//! 浮层内容：按 [`TaskKind`] 分发的骨架、结果卡与流式/失败视图。
//!
//! 词卡精排（音标/词性/释义/例句），其余任务展示 markdown 正文
//! （egui_commonmark 渲染；OCR 提取文本保持纯文本，不按 markdown 解释）。
//! 划选即复制：全部文本可选中，无独立复制按钮。正文完整渲染不截断，
//! 高度自适应内容（宽度默认 380、上限 480，高度上限按屏幕），超出部分
//! 滚动兜底。流式视图按 [`STRUCTURED_FENCE`] 从**首个**围栏标记起整段
//! 截断（围栏后是模型在写结构化 JSON，一个字节都不该闪现；跨 chunk 切分
//! 出的残缺围栏前缀可能短暂显示，随下一 chunk 自愈）。页头是应用图标 +
//! 品牌名 + 任务标签药丸；动作区常驻设置齿轮与关闭 ×，点击经 draw 返回
//! [`OverlayAction`] 上交壳执行。
//!
//! 敏感信息防护不在这里：两条闸门都不出浮层（见 `gloss_app::machine`），
//! 因此也没有「疑似敏感」这张卡。

use std::cell::{Cell, RefCell};
use std::sync::OnceLock;
use std::time::Duration;

use egui::{CornerRadius, Frame, Margin, RichText, ScrollArea, Stroke, vec2};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use gloss_core::log::{thread, warn};
use gloss_core::prompt::STRUCTURED_FENCE;
use gloss_core::task::OutcomeStructured;

use super::style::{color, font, radius, space, stroke};
use crate::i18n::Text;
use crate::machine::{ErrorAction, FailureCause, OverlayView};

/// 浮层默认宽度（04 §二：默认 380px，长文本自适应，上限 480px）
pub const WIDTH: f32 = 380.0;
/// 长文本自适应的宽度上限
const MAX_WIDTH: f32 = 480.0;
/// 宽度收敛阈值：完整内容高超过此值视为长文本，加宽到上限
const TALL_GROW: f32 = 320.0;
/// 宽度收回阈值：已加宽的浮层内容矮于此值才收回默认宽（与 TALL_GROW
/// 之间的滞回带防逐帧来回切换）
const TALL_SHRINK: f32 = 240.0;
/// 宽度档位判定的浮点容差
const WIDTH_SWITCH_EPSILON: f32 = 0.5;
/// 页头品牌名：应用名不翻译（与文案表「语言名不翻译」同一原则）。
const BRAND_NAME: &str = "Gloss";
/// 页头应用图标的边长
const HEADER_ICON: f32 = 20.0;
/// 应用图标 PNG：与 Dock 图标同一份设计资产（矢量源与画布说明见
/// assets/icons/gloss-app-icon.svg）。
const APP_ICON_PNG: &[u8] = include_bytes!("../../../../assets/icons/gloss-dock-icon.png");
/// 应用图标画布的 Big Sur 规范比例：画布 1024、四周透明边距 100、图形本体
/// 824（SVG 源同值）；页头按此比例裁出图形本体，不显示透明边距。
const ICON_CANVAS: u32 = 1024;
const ICON_MARGIN: u32 = 100;
const ICON_CONTENT: u32 = 824;
/// 头部齿轮的字形尺寸
const ACTION_ICON_SIZE: f32 = 16.0;
/// 头部动作钮的方块边长：齿轮与关闭的命中区统一到这个盒子
const ACTION_BUTTON: f32 = 20.0;
/// 关闭 × 的半臂长与线宽：画出的 × 与齿轮字形等视觉大小（齿轮 14×15）
const CLOSE_ARM: f32 = 6.0;
const CLOSE_STROKE: f32 = 2.0;
/// 页头标签药丸的内边距（水平/垂直）
const TAG_PILL_PADDING_H: i8 = 6;
const TAG_PILL_PADDING_V: i8 = 3;
/// 页头标签药丸圆角（egui 自动钳到半高，等效全圆胶囊）
const TAG_PILL_RADIUS: u8 = 10;
/// 页头标签铺底的透明度（0-255）：约一成不透明度的同色铺底
const TAG_TINT_ALPHA: u8 = 0x1A;
/// 出现动画时长（淡入，秒）：显示/重显后的第一帧从 0 渐进到 1。
const APPEAR_SECONDS: f32 = 0.18;

/// 浮层的跨帧渲染状态（每窗口一份，由渲染管线持有）。
pub(crate) struct RenderState {
    /// markdown 渲染状态（egui_commonmark 要求跨帧持有）。
    pub cache: RefCell<CommonMarkCache>,
    /// 上一帧应用的浮层宽度（宽度收敛的滞回状态）。
    pub last_width: Cell<f32>,
    /// 页头应用图标的纹理（每个 egui 上下文一份，惰性装入）。None＝尚未
    /// 装入或解码失败；失败时每帧重试的成本只有一次常量读取，不再单设
    /// 失败标记。
    icon: RefCell<Option<egui::TextureHandle>>,
}

impl Default for RenderState {
    fn default() -> Self {
        Self {
            cache: RefCell::new(CommonMarkCache::default()),
            last_width: Cell::new(WIDTH),
            icon: RefCell::new(None),
        }
    }
}

impl RenderState {
    /// 页头应用图标纹理；首次调用解码 PNG 并装入当前上下文。
    fn icon_texture(&self, ctx: &egui::Context) -> Option<egui::TextureHandle> {
        let mut slot = self.icon.borrow_mut();
        if slot.is_none()
            && let Some(image) = app_icon_image()
        {
            *slot = Some(ctx.load_texture("gloss_app_icon", image, egui::TextureOptions::LINEAR));
        }
        slot.clone()
    }
}

/// 应用图标的解码结果：进程内只解码一次（含失败）。
fn app_icon_image() -> Option<egui::ColorImage> {
    static DECODED: OnceLock<Option<egui::ColorImage>> = OnceLock::new();
    DECODED.get_or_init(decode_app_icon).clone()
}

/// 解码并按画布比例裁出图形本体；失败走隔离降级——记一条告警，页头退化
/// 为无图标的品牌名行，不影响其余内容。
fn decode_app_icon() -> Option<egui::ColorImage> {
    use image::GenericImageView;
    let decoded = match image::load_from_memory(APP_ICON_PNG) {
        Ok(decoded) => decoded,
        Err(error) => {
            warn!(
                thread = thread::UI,
                error = %error,
                "app icon failed to decode, header renders without it"
            );
            return None;
        }
    };
    let (width, _) = decoded.dimensions();
    let margin = width * ICON_MARGIN / ICON_CANVAS;
    let content = width * ICON_CONTENT / ICON_CANVAS;
    let rgba = decoded
        .crop_imm(margin, margin, content, content)
        .to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [content as usize, content as usize],
        rgba.as_raw(),
    ))
}

/// 一帧浮层绘制的产物：动作上交 + 内容期望的窗口尺寸（逻辑点）。
pub(crate) struct PopupOutput {
    pub action: Option<OverlayAction>,
    pub sizing: OverlaySizing,
}

/// 浮层上交壳执行的动作：失败卡动作（重试/打开设置）与头部动作区
/// （齿轮=打开设置、×=收起）。浮层只渲染、不副作用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayAction {
    /// 失败卡「重试」：同代数同任务重发通道③。
    Retry,
    /// 设置入口（失败卡按钮或头部齿轮）：走壳层统一 `open_settings`。
    OpenSettings,
    /// 头部「×」：收起浮层（壳层放弃在途任务回 Idle）。
    Dismiss,
}

impl From<ErrorAction> for OverlayAction {
    fn from(action: ErrorAction) -> Self {
        match action {
            ErrorAction::Retry => OverlayAction::Retry,
            ErrorAction::OpenSettings => OverlayAction::OpenSettings,
        }
    }
}

/// 内容自适应的期望窗口尺寸（逻辑点，含内边距与框线）。
#[derive(Clone, Copy, Debug)]
pub struct OverlaySizing {
    /// 期望窗口宽度。
    pub width: f32,
    /// 期望窗口高度（完整内容高，壳侧按显示器钳制）。
    pub height: f32,
}

/// 出现动画起点在 egui memory 里的键。
fn appear_t0_id() -> egui::Id {
    egui::Id::new("overlay_appear_t0")
}

/// 淡入进度：给定居内已流逝秒数，返回 0..=1 的不透明度。
fn appear_progress(elapsed_secs: f64) -> f32 {
    (elapsed_secs / f64::from(APPEAR_SECONDS)).clamp(0.0, 1.0) as f32
}

/// 清除出现动画起点：壳在每次显示时调用，让下一帧重新从 0 淡入
/// （隐藏期无帧运行，起点留在居内无副作用；不清除则旧起点让重显直接
/// 落在完成态）。
pub(crate) fn reset_appear_animation(ctx: &egui::Context) {
    ctx.memory_mut(|mem| mem.data.remove_temp::<f64>(appear_t0_id()));
}

/// 画一帧浮层。根 `Ui` 覆盖整个窗口，卡片铺满它，圆角之外由透明窗口露出桌面。
///
/// `view` 为 `None` 时显示渲染自检卡（预热与自检路径）。返回本帧绘制的
/// 产物——失败卡动作与头部动作区（齿轮/×）上交壳执行——浮层只渲染、不
/// 副作用；期望尺寸由壳经窗口管理器应用（内容自适应高度，超出屏幕滚动兜底）。
pub(crate) fn draw(
    ui: &mut egui::Ui,
    view: Option<&OverlayView>,
    state: &RenderState,
    text: &Text,
) -> PopupOutput {
    // 划选即复制路径：全部文本可选中（含跨 widget 连选）
    ui.style_mut().interaction.selectable_labels = true;

    // 出现淡入：起点在每次显示（壳侧清除后）的首帧重新写入；进度未满时
    // 请求短重绘推进动画。kittest 的 animation_time=0 与固定步进让快照
    // 恒为完成态，不受动画影响。
    let now = ui.input(|i| i.time);
    let started_at = ui
        .ctx()
        .memory_mut(|mem| *mem.data.get_temp_mut_or_insert_with(appear_t0_id(), || now));
    let progress = appear_progress(now - started_at);
    if progress < 1.0 {
        // 请求尽快再来一帧推进淡入（生产中被预测帧时长扣减后接近立即
        // 重绘，由呈现管道钳到 vsync 帧率）。
        ui.ctx()
            .request_repaint_after(Duration::from_secs_f32(0.016));
    }
    ui.set_opacity(progress);

    let fill = ui.visuals().window_fill;
    let window_stroke = ui.visuals().window_stroke;
    let mut output = PopupOutput {
        action: None,
        sizing: OverlaySizing {
            width: state.last_width.get(),
            height: 0.0,
        },
    };
    let mut content_h = 0.0;
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(stroke::CARD, window_stroke.color))
        .corner_radius(CornerRadius::same(radius::CARD))
        .inner_margin(Margin::same(space::CARD_PADDING))
        .show(ui, |ui| {
            output.action = render_content(ui, view, state, &mut content_h, text);
            ui.set_min_size(ui.available_size());
        });

    // 宽度按内容体量收敛：矮内容保持默认宽，长文本加宽到上限，滞回带
    // 防两档间来回切换；高度取完整内容高（超出屏幕由壳侧钳制、滚动兜底）。
    let width = resolve_width(state.last_width.get(), content_h);
    state.last_width.set(width);
    output.sizing = OverlaySizing {
        width,
        height: (content_h + 2.0 * space::CARD_PADDING as f32 + stroke::CARD).round(),
    };
    output
}

/// 宽度收敛决策：已加宽的浮层内容矮于 [`TALL_SHRINK`] 才收回默认宽，
/// 默认宽的内容高于 [`TALL_GROW`] 才加宽——两阈值之间的滞回带保持原档，
/// 内容高随宽度变化时不振荡。
fn resolve_width(last_width: f32, content_h: f32) -> f32 {
    if last_width >= MAX_WIDTH - WIDTH_SWITCH_EPSILON {
        if content_h < TALL_SHRINK {
            WIDTH
        } else {
            MAX_WIDTH
        }
    } else if content_h > TALL_GROW {
        MAX_WIDTH
    } else {
        WIDTH
    }
}

/// 浮层内容（头部 + 各视图正文），并把完整内容高记入 `content_h`：
/// 产物与流式正文放进 ScrollArea（完整渲染、超出滚动兜底），其高度取
/// ScrollArea 报告的内容尺寸，不受视口裁剪影响。头部动作区与失败卡
/// 动作按钮的点击结果透传给调用方。
fn render_content(
    ui: &mut egui::Ui,
    view: Option<&OverlayView>,
    state: &RenderState,
    content_h: &mut f32,
    text: &Text,
) -> Option<OverlayAction> {
    match view {
        None => {
            let action = header(
                ui,
                state,
                Some(text.gloss_popup_selfcheck.as_str()),
                TagTint::Brand,
                false,
                text,
            );
            ui.add_space(space::SECTION);
            selfcheck_body(ui);
            *content_h = ui.min_rect().height();
            action
        }
        Some(OverlayView::Acquiring) => {
            // 取材骨架（触发即显）：头部旋转指示器 + 弱色占位行。没有
            // 选区数据可展示，整卡保持紧凑，取材完成即整卡替换。
            let action = header(ui, state, None, TagTint::Brand, true, text);
            ui.add_space(space::SECTION);
            ui.label(
                RichText::new(text.gloss_popup_fetching.as_str())
                    .size(font::NOTICE)
                    .color(ui.visuals().weak_text_color()),
            );
            *content_h = ui.min_rect().height();
            action
        }
        Some(OverlayView::Streaming {
            source,
            body,
            classified,
        }) => {
            let action = header(
                ui,
                state,
                classified.map(|kind| crate::ui::kind_label(kind, text)),
                TagTint::Brand,
                true,
                text,
            );
            ui.add_space(space::PARAGRAPH);
            ui.label(
                RichText::new(source)
                    .size(font::NOTICE)
                    .color(ui.visuals().weak_text_color()),
            );
            ui.add_space(space::PARAGRAPH);
            let visible = stream_visible_body(body);
            // ScrollArea 内容起点 = cursor（egui 的 cursor 停在前序内容底边
            // 加一个 item_spacing 处），从这里起算正文完整高。
            let body_top = ui.cursor().min.y;
            let scrolled = ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                render_markdown(ui, state, visible);
            });
            *content_h = body_top + scrolled.content_size.y;
            action
        }
        Some(OverlayView::Outcome(outcome)) => {
            let action = header(
                ui,
                state,
                Some(crate::ui::kind_label(outcome.kind, text)),
                TagTint::Brand,
                false,
                text,
            );
            ui.add_space(space::SECTION);
            let body_top = ui.cursor().min.y;
            let scrolled = ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                outcome_body(ui, outcome, state);
            });
            *content_h = body_top + scrolled.content_size.y;
            action
        }
        Some(OverlayView::Failed {
            cause,
            action: error_action,
        }) => {
            let mut action = header(
                ui,
                state,
                Some(text.gloss_popup_failed.as_str()),
                TagTint::Warn,
                false,
                text,
            );
            ui.add_space(space::SECTION);
            ui.label(
                RichText::new(failure_message(cause, text))
                    .size(font::NOTICE)
                    .color(ui.visuals().warn_fg_color),
            );
            if let Some(error_action) = *error_action {
                ui.add_space(space::SECTION);
                if ui
                    .button(
                        RichText::new(action_label(error_action, text))
                            .size(font::CAPTION)
                            .color(ui.visuals().strong_text_color()),
                    )
                    .clicked()
                {
                    action = Some(error_action.into());
                }
            }
            *content_h = ui.min_rect().height();
            action
        }
    }
}

/// 失败卡文案：按失败来源映射。
fn failure_message(cause: &FailureCause, text: &Text) -> String {
    match cause {
        FailureCause::Task(error) => text.for_error(error),
        FailureCause::AcquireChannel => text.gloss_errors_acquire_channel.clone(),
        FailureCause::TransportChannel => text.gloss_errors_inference_channel.clone(),
    }
}

/// 动作按钮的文案。
fn action_label(action: ErrorAction, text: &Text) -> &str {
    match action {
        ErrorAction::Retry => text.gloss_popup_retry.as_str(),
        ErrorAction::OpenSettings => text.gloss_popup_open_settings.as_str(),
    }
}

/// 页头标签的着色档：任务/自检标签走品牌蓝，失败标签走警示色。
#[derive(Clone, Copy, PartialEq, Eq)]
enum TagTint {
    Brand,
    Warn,
}

/// 页头：应用图标 + 品牌名 + 任务标签药丸（着色按 [`TagTint`]），右侧动作
/// 区 `[Spinner | ⚙ ×]`——× 最右（最后动作）、齿轮居左，图标默认弱色、
/// hover/按下显色；`busy` 时旋转指示器随行，`tag` 有值时药丸与它并存
/// （自动分类判明后标签出现）。返回动作区点击。
fn header(
    ui: &mut egui::Ui,
    state: &RenderState,
    tag: Option<&str>,
    tint: TagTint,
    busy: bool,
    text: &Text,
) -> Option<OverlayAction> {
    let weak = ui.visuals().weak_text_color();
    let strong = ui.visuals().strong_text_color();
    let mut action = None;
    ui.horizontal(|ui| {
        if let Some(icon) = state.icon_texture(ui.ctx()) {
            ui.add(
                egui::Image::from_texture(&icon).fit_to_exact_size(vec2(HEADER_ICON, HEADER_ICON)),
            );
            ui.add_space(space::PARAGRAPH);
        }
        ui.label(RichText::new(BRAND_NAME).size(font::BODY).color(strong));
        if let Some(tag) = tag {
            ui.add_space(space::PARAGRAPH);
            tag_pill(ui, tag, tint);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // 图标钮的文字颜色交给 widget 状态笔刷（不写死在字形上），
            // 才有「默认弱色、hover 显色」；只换色，线宽保持出厂值。
            ui.visuals_mut().widgets.inactive.fg_stroke.color = weak;
            ui.visuals_mut().widgets.hovered.fg_stroke.color = strong;
            ui.visuals_mut().widgets.active.fg_stroke.color = strong;
            let close = close_button(ui, text);
            if close.clicked() {
                action = Some(OverlayAction::Dismiss);
            }
            let gear = ui.add(icon_button("⚙"));
            gear.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    text.gloss_popup_settings_label.as_str(),
                )
            });
            if gear.clicked() {
                action = Some(OverlayAction::OpenSettings);
            }
            if busy {
                ui.add(egui::Spinner::new().size(font::TAG + 5.0));
            }
        });
    });
    action
}

/// 头部动作钮（关闭 ×）：与齿轮同尺寸的方块命中区，× 本体用两条圆头线段
/// 绘制——内置字体里 × 字形只有 ⚙ 的四成大，靠字号拉平会撑破按钮盒。
fn close_button(ui: &mut egui::Ui, text: &Text) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ACTION_BUTTON, ACTION_BUTTON), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            text.gloss_popup_close_label.as_str(),
        )
    });
    let color = if response.hovered() || response.is_pointer_button_down_on() {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    let stroke = Stroke::new(CLOSE_STROKE, color);
    let center = rect.center();
    ui.painter().line_segment(
        [
            center - vec2(CLOSE_ARM, CLOSE_ARM),
            center + vec2(CLOSE_ARM, CLOSE_ARM),
        ],
        stroke,
    );
    ui.painter().line_segment(
        [
            center - vec2(CLOSE_ARM, -CLOSE_ARM),
            center + vec2(CLOSE_ARM, -CLOSE_ARM),
        ],
        stroke,
    );
    response
}

/// 页头标签药丸：品牌蓝/警示色的低透明度铺底 + 同色文字，明暗主题各取
/// 可读变体。
fn tag_pill(ui: &mut egui::Ui, label: &str, tint: TagTint) {
    let (fg, bg) = match tint {
        TagTint::Brand => {
            let fg = if ui.visuals().dark_mode {
                color::TAG_TEXT_DARK
            } else {
                color::TAG_TEXT_LIGHT
            };
            (fg, tag_tint(color::TAG_BLUE))
        }
        TagTint::Warn => {
            let fg = ui.visuals().warn_fg_color;
            (fg, tag_tint(fg))
        }
    };
    egui::Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(TAG_PILL_RADIUS))
        .inner_margin(Margin::symmetric(TAG_PILL_PADDING_H, TAG_PILL_PADDING_V))
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(font::TAG).color(fg));
        });
}

/// 同色低透明度铺底。
fn tag_tint(base: egui::Color32) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), TAG_TINT_ALPHA)
}

/// 无边框的动作图标钮（glyph 字形，颜色由 widget 状态笔刷决定）；方块
/// min_size 让齿轮与关闭钮命中区等大。
fn icon_button(glyph: &'static str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(glyph).size(ACTION_ICON_SIZE))
        .frame(false)
        .min_size(vec2(ACTION_BUTTON, ACTION_BUTTON))
}

/// 流式正文的可见部分：从**首个**结构化围栏标记起整段截断。模型按契约
/// 先写完正文再写围栏 JSON，围栏一出现其后全是结构化载荷；取首个而不是
/// 末个，模型跑偏（正文里提前出现围栏标记后继续写正文）时同样被拦在
/// 围栏外。与完成态 core 侧的剥离（`finalize_outcome`）共用
/// [`STRUCTURED_FENCE`] 单点，两侧各一处实现。
fn stream_visible_body(body: &str) -> &str {
    match body.find(STRUCTURED_FENCE) {
        Some(pos) => &body[..pos],
        None => body,
    }
}

/// markdown 正文：完整渲染（egui_commonmark 解析绘制），缓存跨帧持有。
fn render_markdown(ui: &mut egui::Ui, state: &RenderState, text: &str) {
    let mut cache = state.cache.borrow_mut();
    CommonMarkViewer::new().show(ui, &mut cache, text);
}

/// 产物正文：词卡精排，其余任务展示 markdown 正文/提取文本。
fn outcome_body(ui: &mut egui::Ui, outcome: &gloss_core::task::TaskOutcome, state: &RenderState) {
    match &outcome.structured {
        OutcomeStructured::WordCard {
            word,
            phonetic,
            senses,
        } => word_card(ui, word, phonetic.as_deref(), senses),
        OutcomeStructured::Plain { title } => {
            if let Some(title) = title {
                ui.label(
                    RichText::new(title.as_str())
                        .size(font::TITLE)
                        .strong()
                        .color(ui.visuals().strong_text_color()),
                );
                ui.add_space(space::PARAGRAPH);
            }
            render_markdown(ui, state, &outcome.body);
        }
        OutcomeStructured::Extracted { text } => plain_body(ui, text),
    }
}

/// 词卡精排：词条 + 音标行，按词性分组的释义与弱化例句。
fn word_card(
    ui: &mut egui::Ui,
    word: &str,
    phonetic: Option<&str>,
    senses: &[gloss_core::task::Sense],
) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(word)
                .size(font::WORD)
                .strong()
                .color(ui.visuals().strong_text_color()),
        );
        if let Some(phonetic) = phonetic {
            ui.label(
                RichText::new(phonetic)
                    .size(font::NOTICE)
                    .color(ui.visuals().weak_text_color()),
            );
        }
    });
    ui.add_space(space::GROUP);
    let strong = ui.visuals().strong_text_color();
    let weak = ui.visuals().weak_text_color();
    for sense in senses {
        ui.horizontal_wrapped(|ui| {
            if let Some(pos) = &sense.pos {
                ui.label(RichText::new(pos.as_str()).size(font::CAPTION).color(weak));
            }
            ui.label(
                RichText::new(sense.meaning.as_str())
                    .size(font::BODY)
                    .color(strong),
            );
        });
        for example in &sense.examples {
            ui.add_space(space::INLINE);
            ui.indent("example", |ui| {
                ui.label(
                    RichText::new(format!("· {example}"))
                        .size(font::CAPTION)
                        .color(weak),
                );
            });
        }
        ui.add_space(space::ITEM);
    }
}

/// 可选中、自动换行的纯文本正文（OCR 提取文本不按 markdown 解释）。
fn plain_body(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .size(font::BODY)
                .color(ui.visuals().strong_text_color()),
        )
        .wrap()
        .selectable(true),
    );
}

/// 自检卡正文（预热与显隐自检路径）：中英混排一眼可辨字体链路健康。
fn selfcheck_body(ui: &mut egui::Ui) {
    let strong = ui.visuals().strong_text_color();
    let weak = ui.visuals().weak_text_color();
    ui.label(
        RichText::new("The quick brown fox jumps over the lazy dog.")
            .size(font::NOTICE)
            .color(weak),
    );
    ui.add_space(space::SECTION);
    ui.label(
        RichText::new("敏捷的棕色狐狸从懒狗身上跳过。")
            .size(font::TITLE)
            .strong()
            .color(strong),
    );
    ui.add_space(space::SECTION);
    ui.label(
        RichText::new("中文渲染自检：划词翻译、代码解释、图片识别。")
            .size(font::NOTICE)
            .color(weak),
    );
}

#[cfg(test)]
mod tests {
    use super::{MAX_WIDTH, WIDTH, resolve_width, stream_visible_body};

    #[test]
    fn width_hysteresis_does_not_oscillate_between_frames() {
        let grown = resolve_width(WIDTH, 400.0);
        assert_eq!(grown, MAX_WIDTH, "长内容必须加宽");

        assert_eq!(resolve_width(MAX_WIDTH, 400.0), MAX_WIDTH);
        assert_eq!(
            resolve_width(MAX_WIDTH, 300.0),
            MAX_WIDTH,
            "滞回带内保持已加宽档"
        );

        assert_eq!(
            resolve_width(MAX_WIDTH, 200.0),
            WIDTH,
            "明显变矮才收回默认档"
        );
        assert_eq!(resolve_width(WIDTH, 200.0), WIDTH, "矮内容保持默认档");
    }

    #[test]
    fn width_hysteresis_band_bounds_are_symmetric() {
        assert_eq!(resolve_width(WIDTH, 321.0), MAX_WIDTH);
        assert_eq!(resolve_width(WIDTH, 319.0), WIDTH, "阈值之下不加宽");
        assert_eq!(resolve_width(MAX_WIDTH, 241.0), MAX_WIDTH);
        assert_eq!(resolve_width(MAX_WIDTH, 239.0), WIDTH, "阈值之下才收回");
    }

    #[test]
    fn stream_visible_body_truncates_from_the_first_fence() {
        assert_eq!(
            stream_visible_body("正文一\n```gloss\n{\"title\":\"x\"}\n```\n正文二"),
            "正文一\n",
            "everything from the first fence on is hidden, including later prose"
        );
        assert_eq!(
            stream_visible_body("没有围栏的正文"),
            "没有围栏的正文",
            "no fence means the whole body is visible"
        );
        assert_eq!(stream_visible_body(""), "");
        assert_eq!(
            stream_visible_body("```gloss\n{\"title\":\"x\"}"),
            "",
            "a body that opens with the fence shows nothing"
        );
        assert_eq!(
            stream_visible_body("正文\n```glossparticular\n不该显示"),
            "正文\n",
            "the marker matches by prefix, matching the core-side stripper"
        );
    }
}

#[cfg(test)]
mod kittest_tests {

    use std::cell::RefCell;
    use std::rc::Rc;

    use egui_kittest::{Harness, kittest::Queryable};

    use super::*;
    use crate::machine::OverlayView;
    use gloss_core::model::{GlossError, Locale};
    use gloss_core::task::{Sense, TaskKind, TaskOutcome};

    fn word_card_view() -> OverlayView {
        OverlayView::Outcome(TaskOutcome {
            kind: TaskKind::TranslateWord,
            body: "markdown 正文".into(),
            structured: OutcomeStructured::WordCard {
                word: "gloss".into(),
                phonetic: Some("/ɡlɒs/".into()),
                senses: vec![Sense {
                    pos: Some("n.".into()),
                    meaning: "光泽；注释".into(),
                    examples: vec!["a gloss of silk".into()],
                }],
            },
        })
    }

    fn streaming_view() -> OverlayView {
        OverlayView::Streaming {
            source: "选中的原文".into(),
            body: "已流式到达的正文\n```gloss\n{\"title\":\"摘要\"}\n```".into(),
            classified: Some(TaskKind::TranslateWord),
        }
    }

    fn acquiring_view() -> OverlayView {
        OverlayView::Acquiring
    }

    fn failed_view() -> OverlayView {
        OverlayView::Failed {
            cause: FailureCause::Task(GlossError::EngineNetwork),
            action: Some(ErrorAction::Retry),
        }
    }

    fn auth_failed_view() -> OverlayView {
        OverlayView::Failed {
            cause: FailureCause::Task(GlossError::EngineAuth),
            action: Some(ErrorAction::OpenSettings),
        }
    }

    fn bare_failed_view() -> OverlayView {
        OverlayView::Failed {
            cause: FailureCause::Task(GlossError::EngineResponse("HTTP 400 Bad Request".into())),
            action: None,
        }
    }

    fn failed_view_with(cause: FailureCause) -> OverlayView {
        OverlayView::Failed {
            cause,
            action: None,
        }
    }

    fn long_body_view() -> OverlayView {
        OverlayView::Outcome(TaskOutcome {
            kind: TaskKind::TranslateSentence,
            body: "很长的正文段落。".repeat(1000) + "尾部标记",
            structured: OutcomeStructured::Plain { title: None },
        })
    }

    type Clicked = Rc<RefCell<Option<OverlayAction>>>;

    fn harness_for(view: OverlayView) -> (Harness<'static>, Clicked) {
        harness_for_locale(view, Locale::Zh)
    }

    fn harness_for_locale(view: OverlayView, locale: Locale) -> (Harness<'static>, Clicked) {
        let clicked: Clicked = Rc::new(RefCell::new(None));
        let sink = Rc::clone(&clicked);
        let state = RenderState::default();
        let text = Text::get(locale);
        let harness = Harness::new_ui(move |ui| {
            let output = draw(ui, Some(&view), &state, text);
            if let Some(action) = output.action {
                *sink.borrow_mut() = Some(action);
            }
        });
        (harness, clicked)
    }

    #[test]
    fn failure_card_words_each_cause() {
        let errors = Text::get(Locale::Zh);
        for (cause, expected) in [
            (
                FailureCause::Task(GlossError::EngineNetwork),
                errors.gloss_errors_engine_network.as_str(),
            ),
            (
                FailureCause::Task(GlossError::EngineResponse("HTTP 400".into())),
                "服务返回异常：HTTP 400",
            ),
            (FailureCause::AcquireChannel, "任务失败：取材通道不可用"),
            (FailureCause::TransportChannel, "任务失败：推理通道不可用"),
        ] {
            let (mut harness, _clicked) = harness_for(failed_view_with(cause));
            harness.run();
            harness.get_by_label_contains(expected);
        }
    }

    #[test]
    fn failure_card_follows_the_locale() {
        let (mut harness, _clicked) = harness_for_locale(failed_view(), Locale::En);
        harness.run();
        harness.get_by_label_contains("Network error.");
        harness.get_by_label("Retry");
    }

    #[test]
    fn word_card_exposes_entries_to_accesskit() {
        let (mut harness, _clicked) = harness_for(word_card_view());
        harness.run();
        harness.get_by_label("gloss");
        harness.get_by_label("/ɡlɒs/");
        harness.get_by_label("光泽；注释");
        harness.get_by_label("· a gloss of silk");
    }

    #[test]
    fn long_body_is_rendered_in_full() {
        let (mut harness, _clicked) = harness_for(long_body_view());
        harness.run();
        assert!(
            harness
                .query_all_by_label_contains("尾部标记")
                .next()
                .is_some(),
            "完整渲染不得截断尾部内容"
        );
    }

    #[test]
    fn acquiring_view_shows_the_fetching_skeleton() {
        let (mut harness, _clicked) = harness_for(acquiring_view());
        harness.run_steps(3);
        harness.get_by_label_contains("正在读取选区");
        let fence_visible = harness.query_all_by_label_contains("原文").next().is_some();
        assert!(
            !fence_visible,
            "the skeleton carries no source or streaming content"
        );
    }

    #[test]
    fn streaming_view_hides_structured_block() {
        let (mut harness, _clicked) = harness_for(streaming_view());
        harness.run_steps(3);
        harness.get_by_label_contains("已流式到达的正文");
        harness.get_by_label_contains("选中的原文");
        let fence_visible = harness
            .query_all_by_label_contains("```gloss")
            .next()
            .is_some();
        assert!(
            !fence_visible,
            "structured fence must be filtered out of the streaming view"
        );
    }

    #[test]
    fn failed_view_shows_retry_hint() {
        let (mut harness, clicked) = harness_for(failed_view());
        harness.run();
        harness.get_by_label_contains("网络错误");
        harness.get_by_label("重试").click();
        harness.run();
        assert_eq!(
            *clicked.borrow(),
            Some(OverlayAction::Retry),
            "clicking retry must surface the action via draw's return value"
        );
    }

    #[test]
    fn auth_failed_view_offers_open_settings() {
        let (mut harness, clicked) = harness_for(auth_failed_view());
        harness.run();
        harness.get_by_label_contains("API Key");
        harness.get_by_label("打开设置").click();
        harness.run();
        assert_eq!(
            *clicked.borrow(),
            Some(OverlayAction::OpenSettings),
            "clicking open-settings must surface the action"
        );
    }

    #[test]
    fn close_button_submits_dismiss() {
        let (mut harness, clicked) = harness_for(word_card_view());
        harness.run();
        harness.get_by_label("关闭浮层").click();
        harness.run();
        assert_eq!(
            *clicked.borrow(),
            Some(OverlayAction::Dismiss),
            "the header close button must dismiss via the shell"
        );
    }

    #[test]
    fn gear_button_submits_open_settings() {
        let (mut harness, clicked) = harness_for(word_card_view());
        harness.run();
        harness.get_by_label("设置").click();
        harness.run();
        assert_eq!(
            *clicked.borrow(),
            Some(OverlayAction::OpenSettings),
            "the header gear must open settings via the shared entry"
        );
    }

    #[test]
    fn selfcheck_view_exposes_texts_to_accesskit() {
        let state = RenderState::default();
        let text = Text::get(Locale::Zh);
        let mut harness = Harness::new_ui(move |ui| {
            let _ = draw(ui, None, &state, text);
        });
        harness.run();
        harness.get_by_label_contains("quick brown fox");
        harness.get_by_label_contains("中文渲染自检");
    }

    #[test]
    fn bare_failed_view_has_no_action_button() {
        let (mut harness, clicked) = harness_for(bare_failed_view());
        harness.run();
        harness.get_by_label_contains("服务返回异常");
        assert!(
            harness.query_all_by_label_contains("重试").next().is_none(),
            "no retry button without an action"
        );
        assert_eq!(*clicked.borrow(), None, "no click without a button");
    }

    #[test]
    fn snapshots_match_baseline() {
        let mut results = egui_kittest::SnapshotResults::new();

        let (mut harness, _clicked) = harness_for(word_card_view());
        harness.run();
        harness.snapshot("popup_word_card");
        results.extend_harness(&mut harness);

        let (mut harness, _clicked) = harness_for(acquiring_view());
        harness.run_steps(3);
        harness.snapshot("popup_acquiring");
        results.extend_harness(&mut harness);

        let (mut harness, _clicked) = harness_for(streaming_view());
        harness.run_steps(3);
        harness.snapshot("popup_streaming");
        results.extend_harness(&mut harness);

        let (mut harness, _clicked) = harness_for(failed_view());
        harness.run();
        harness.snapshot("popup_failed");
        results.extend_harness(&mut harness);

        let (mut harness, _clicked) = harness_for(auth_failed_view());
        harness.run();
        harness.snapshot("popup_failed_auth");
        results.extend_harness(&mut harness);

        let state = RenderState::default();
        let text = Text::get(Locale::Zh);
        let mut harness = Harness::new_ui(move |ui| {
            let _ = draw(ui, None, &state, text);
        });
        harness.run();
        harness.snapshot("popup_selfcheck");
        results.extend_harness(&mut harness);

        results.unwrap();
    }
}
