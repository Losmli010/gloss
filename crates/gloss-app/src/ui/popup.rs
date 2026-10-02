//! 浮层内容：按视图分发的骨架、经注疏结果卡与流式/失败视图。
//!
//! 内容按「经 · 注 · 疏」三层组织，视觉定稿以 docs/demo/popup-redesign.html
//! 为准（本地文件，不进 git）：
//! - **经**＝选区原文/词条/提取文本（宋体；代码任务为界栏框内嵌代码面板
//!   ——专属底色、`gloss-mono` 等宽体、不换行横滚、右上角语言角标）；
//! - **注**＝译文/释义/概要（楷体，系统缺楷体时随 demo 回退链落宋体；
//!   朱丝栏左线 + 朱印）；
//! - **疏**＝译注/例句/小记（小号宋体、弱色、上缘虚线 + 疏印）。
//!
//! 三分区的正文列都显式声明垂直布局：egui 的 Frame/子 Ui 会继承父级的
//! 水平布局（`new_child` 不带 layout 参数），横排父级里多条目正文会从左往
//! 右流（释义并排、溢出右缘）。分区的字号/间距是本模块私有量（demo 定稿
//! 值），不进 style 阶梯。
//!
//! 词卡精排（词条/音标落经位，释义逐行落注位，例句落疏位），其余任务正文
//! 走 markdown（egui_commonmark 渲染；注区统一改写文本样式为楷体字号）。
//! 划选即复制：全部文本可选中，无独立复制按钮。正文完整渲染不截断，高度
//! 自适应内容（宽度默认 380、上限 480，高度上限按屏幕），超出部分滚动兜底。
//! 流式视图按 [`STRUCTURED_FENCE`] 从**首个**围栏标记起整段截断（围栏后是
//! 模型在写结构化 JSON，一个字节都不该闪现；跨 chunk 切分出的残缺围栏前缀
//! 可能短暂显示，随下一 chunk 自愈）。
//!
//! 页头回归品牌：只有应用图标与动作区（⚙/×），任务与状态由内容层自明，
//! 页头不带任何标签药丸；行下发丝线与页脚上缘线呼应成卡片的上下界。
//! 页脚常驻
//! 一条窄带（带高取 [`FOOTER_HEIGHT`]）：推理中左端是呼吸点 + 「正在注解」（生成指示唯一落点），右端
//! 恒为 Gloss 水印（品牌名不翻译，与窗口标题同一原则）；滚动区按页脚带宽
//! 预留视口，页脚不被内容挤出窗外。取材中是纯骨架（脉动条），全程无
//! 「正在读取选区」类文字。动作点击经 draw 返回 [`OverlayAction`] 上交壳执行。
//!
//! 敏感信息防护不在这里：两条闸门都不出浮层（见 `gloss_app::machine`），
//! 因此也没有「疑似敏感」这张卡。

use std::cell::{Cell, RefCell};
use std::sync::OnceLock;
use std::time::Duration;

use egui::{
    Align, CornerRadius, FontFamily, FontId, Frame, Margin, RichText, ScrollArea, Shape, Stroke,
    TextStyle, vec2,
};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use gloss_core::log::{thread, warn};
use gloss_core::prompt::STRUCTURED_FENCE;
use gloss_core::task::{OutcomeStructured, TaskKind};

use super::fonts;
use super::style::{color, font, radius, space, stroke};
use crate::i18n::{Text, fill};
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
/// 关闭 × 的半臂长与线宽：× 本体 10×10，较齿轮字形偏小
const CLOSE_ARM: f32 = 5.0;
const CLOSE_STROKE: f32 = 2.0;
/// 出现动画时长（淡入，秒）：显示/重显后的第一帧从 0 渐进到 1。
const APPEAR_SECONDS: f32 = 0.18;
/// 经注疏排版的字号（demo 定稿值）：经 15.5、注 15、疏 12.5；词条 25，
/// 音标/词性 13，印章字 12。
const JING_FONT: f32 = 15.5;
const ZHU_FONT: f32 = 15.0;
const SHU_FONT: f32 = 12.5;
const WORD_FONT: f32 = 25.0;
const PHON_FONT: f32 = 13.0;
const SEAL_FONT: f32 = 12.0;
/// 印章方块的边长与圆角（demo 定稿：21px、5px 圆角）；en 缩写按文本宽度
/// 撑宽，方块边长是下限。
const SEAL_SIZE: f32 = 21.0;
const SEAL_RADIUS: u8 = 5;
/// 印章字在方块内的水平余量（单侧）：单字居中即方块本身，缩写按文本撑宽。
const SEAL_TEXT_PADDING: f32 = 5.0;
/// 印章列与正文列之间的空隙（demo .sec gap 10px）。
const SEAL_GAP: f32 = 10.0;
/// 朱丝栏（注区左线）的线宽与不透明度（demo：2px、朱砂 55% 混透明）。
const ZHU_LINE_WIDTH: f32 = 2.0;
const ZHU_LINE_ALPHA: u8 = 140;
/// 朱丝栏到注正文之间的空隙（demo padding-left 11px，含线宽）。
const ZHU_TEXT_GAP: i8 = 11;
/// 疏区上缘虚线到疏正文的空隙（demo padding-top 9px）。
const SHU_TEXT_GAP: i8 = 9;
/// 疏区虚线的段长与空隙。
const SHU_DASH_LENGTH: f32 = 4.0;
const SHU_GAP_LENGTH: f32 = 3.0;
/// 经区块（界栏框）的圆角与内边距（demo jing-frame：radius 10、10px 13px）。
const JING_FRAME_RADIUS: u8 = 10;
/// 界栏框内边距（水平/垂直）。
const JING_FRAME_PADDING_H: i8 = 13;
const JING_FRAME_PADDING_V: i8 = 10;
/// 代码面板（界栏框内嵌层）的圆角、边宽与内边距（demo code 面板：
/// radius 10、1px 边、11px 13px）。
const CODE_PANEL_RADIUS: u8 = 10;
const CODE_PANEL_STROKE: f32 = 1.0;
const CODE_PANEL_PADDING_V: i8 = 11;
const CODE_PANEL_PADDING_H: i8 = 13;
/// 代码正文的字号与行高（demo 定稿：12px、行高 1.65）。
const CODE_FONT: f32 = 12.0;
const CODE_LINE_HEIGHT: f32 = CODE_FONT * 1.65;
/// 语言标签的字号（图样定稿：弱色小标，面板内独立行）。
const CODE_BADGE_FONT: f32 = 10.0;
/// 骨架条高（取材骨架与「经显注未至」的占位行同款）
const SHIMMER_BAR_HEIGHT: f32 = 12.0;
/// 骨架条圆角
const SHIMMER_BAR_RADIUS: u8 = 4;
/// 骨架条与页脚呼吸点的脉动周期（秒）
const PULSE_PERIOD_SECS: f64 = 1.2;
/// 骨架条铺底的透明度档（0-255）：脉动在这个下限与峰值之间摆动
const SHIMMER_ALPHA_MIN: u8 = 38;
const SHIMMER_ALPHA_MAX: u8 = 96;
/// 页脚呼吸点的半径与个数
const FOOTER_DOT_RADIUS: f32 = 2.0;
const FOOTER_DOT_COUNT: usize = 3;
/// 页脚水印字串与槽缘的余量（单侧）：槽宽按实测字宽加此余量。
const WATERMARK_PADDING: f32 = 6.0;
/// 页脚带高：水印字形（`font::TAG` 11px）加合理上下余量的下限档。
const FOOTER_HEIGHT: f32 = 20.0;
/// 页脚与正文之间的空隙；滚动区按这条带宽预留视口
/// （auto_shrink(false) 的滚动区会吃光剩余空间，不预留页脚就被挤出窗外）。
const FOOTER_RESERVE: f32 = FOOTER_HEIGHT + space::PARAGRAPH;
/// 滚动区视口的下限：窗口被压得极矮时正文至少还能滚出这么多。
const MIN_BODY_VIEWPORT: f32 = 48.0;

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
    DECODED
        .get_or_init(|| decode_app_icon(APP_ICON_PNG))
        .clone()
}

/// 解码给定的 PNG 字节并按画布比例裁出图形本体；失败走隔离降级——记一条
/// 告警，页头退化为无图标的动作行，不影响其余内容。参数化 PNG 来源，
/// 裁剪数学与降级分支可经 L1 测试直接驱动。
fn decode_app_icon(png: &[u8]) -> Option<egui::ColorImage> {
    use image::GenericImageView;
    let decoded = match image::load_from_memory(png) {
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
    let (width, height) = decoded.dimensions();
    // 裁剪数学假设正方形画布与四边等边距（SVG 源即如此）；资产若改版失衡，
    // debug 构建里第一时间显形。
    debug_assert_eq!(width, height, "app icon canvas is expected to be square");
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

/// 滚动视图的完整内容高：实测布局高（视口收缩到内容时即真实高度）加上
/// 视口放不下的溢出量（内容高于视口时窗口按完整内容高申请，壳侧钳到屏）。
/// 公式口径（body_top + content_size + 预留）与 egui 的行距累计差一个
/// item_spacing，页脚会悬在窗外 17px——以实测为准。
fn record_scrolled_height(
    ui: &egui::Ui,
    content_h: &mut f32,
    scrolled: &egui::scroll_area::ScrollAreaOutput<()>,
) {
    let overflow = (scrolled.content_size.y - scrolled.inner_rect.height()).max(0.0);
    *content_h = ui.min_rect().height() + overflow;
}

/// 浮层内容（头部 + 各视图正文），并把完整内容高记入 `content_h`：
/// 产物与流式正文放进 ScrollArea（完整渲染、超出滚动兜底），其高度取
/// ScrollArea 报告的内容尺寸，不受视口裁剪影响；页脚带恒在（滚动区按
/// [`FOOTER_RESERVE`] 预留视口，页脚不被内容挤出窗外）。头部动作区与
/// 失败卡动作按钮的点击结果透传给调用方。
fn render_content(
    ui: &mut egui::Ui,
    view: Option<&OverlayView>,
    state: &RenderState,
    content_h: &mut f32,
    text: &Text,
) -> Option<OverlayAction> {
    match view {
        None => {
            let action = header(ui, state, text);
            ui.add_space(space::SECTION);
            selfcheck_body(ui);
            ui.add_space(space::PARAGRAPH);
            footer(ui, false, text);
            *content_h = ui.min_rect().height();
            action
        }
        Some(OverlayView::Acquiring) => {
            // 取材骨架（触发即显）：纯脉动条，无任何取材文字。没有选区
            // 数据可展示，整卡保持紧凑，取材完成即整卡替换。
            let action = header(ui, state, text);
            ui.add_space(space::SECTION);
            shimmer_bars(ui);
            ui.add_space(space::PARAGRAPH);
            footer(ui, true, text);
            *content_h = ui.min_rect().height();
            action
        }
        Some(OverlayView::Streaming {
            source,
            body,
            classified,
            code_lang,
        }) => {
            let action = header(ui, state, text);
            ui.add_space(space::SECTION);
            let code = is_code(*classified);
            jing_section(ui, text, |ui| {
                source_block(ui, source, code, code_lang.as_deref())
            });
            ui.add_space(space::PARAGRAPH);
            // ScrollArea 内容起点 = cursor（egui 的 cursor 停在前序内容底边
            // 加一个 item_spacing 处），从这里起算正文完整高。
            let visible = stream_visible_body(body);
            let viewport_max = (ui.available_height() - FOOTER_RESERVE).max(MIN_BODY_VIEWPORT);
            let scrolled = ScrollArea::new([code, true])
                .auto_shrink([false, true])
                .max_height(viewport_max)
                .show(ui, |ui| {
                    if visible.is_empty() {
                        // 经已回显、注未至：正文保持骨架（无注印——还没有可注的内容）。
                        shimmer_bars(ui);
                    } else {
                        zhu_section(ui, text, |ui| {
                            apply_zhu_typography(ui);
                            render_markdown(ui, state, visible);
                        });
                    }
                });
            ui.add_space(space::PARAGRAPH);
            footer(ui, true, text);
            record_scrolled_height(ui, content_h, &scrolled);
            action
        }
        Some(OverlayView::Outcome {
            source,
            outcome,
            code_lang,
        }) => {
            let action = header(ui, state, text);
            ui.add_space(space::SECTION);
            let viewport_max = (ui.available_height() - FOOTER_RESERVE).max(MIN_BODY_VIEWPORT);
            let scrolled = ScrollArea::new([is_code(Some(outcome.kind)), true])
                .auto_shrink([false, true])
                .max_height(viewport_max)
                .show(ui, |ui| {
                    outcome_body(ui, source, outcome, code_lang.as_deref(), state, text);
                });
            ui.add_space(space::PARAGRAPH);
            footer(ui, false, text);
            record_scrolled_height(ui, content_h, &scrolled);
            action
        }
        Some(OverlayView::Failed {
            cause,
            action: error_action,
        }) => {
            let mut action = header(ui, state, text);
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
            ui.add_space(space::PARAGRAPH);
            footer(ui, false, text);
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

/// 卡片内分隔发丝线的共享规格（页头下缘与页脚上缘同一条线）：0.5 宽、
/// noninteractive 边色。
fn hairline(ui: &egui::Ui) -> Stroke {
    Stroke::new(
        stroke::CARD,
        ui.visuals().widgets.noninteractive.bg_stroke.color,
    )
}

/// 页头：应用图标 + 右侧动作区 `[⚙ ×]`——× 最右（最后动作）、齿轮居左，
/// 图标默认弱色、hover/按下显色。任务与状态都由内容层自明（经注疏分区、
/// 失败卡正文），页头不带任何标签药丸（demo 定稿：页头回归品牌，只有
/// 图标与动作区；生成指示移交页脚，见 [`footer`]）。行内容之下隔开
/// `space::ITEM` 画一条与页脚上缘同规格的发丝线，横贯内容宽（取容器
/// 全宽，与图标装没装无关），线与正文之间仍由各视图的 `space::SECTION`
/// 隔开。返回动作区点击。
fn header(ui: &mut egui::Ui, state: &RenderState, text: &Text) -> Option<OverlayAction> {
    let weak = ui.visuals().weak_text_color();
    let strong = ui.visuals().strong_text_color();
    let mut action = None;
    ui.horizontal(|ui| {
        if let Some(icon) = state.icon_texture(ui.ctx()) {
            ui.add(
                egui::Image::from_texture(&icon).fit_to_exact_size(vec2(HEADER_ICON, HEADER_ICON)),
            );
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
            let gear = ui.add(icon_button("⚙")); // i18n:allow 图标字形，非 locale 文案
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
        });
    });
    // 发丝线与行内容（图标/动作钮）之间留一档间距，不贴着字形底边。
    ui.add_space(space::ITEM);
    let line_y = ui.cursor().top();
    ui.painter()
        .hline(ui.max_rect().x_range(), line_y, hairline(ui));
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

/// 印章着色档：墨印（纸色实心 + 墨色字，经用）、朱印（朱砂实心 + 纸色字，
/// 注用）与疏印（描边 + 弱色字，疏用）——demo 三印各成一体。
#[derive(Clone, Copy, PartialEq, Eq)]
enum SealTint {
    Ink,
    Zhu,
    Wei,
}

/// 经注疏印章：小圆角方块 + 单字（en 缩写按文本撑宽，方块边长是下限）。
/// 几何与配色按 demo 定稿（21px 方块、5px 圆角、宋体字）。
fn seal(ui: &mut egui::Ui, label: &str, tint: SealTint) {
    let (fg, fill, line) = match tint {
        SealTint::Ink => (
            paper_color(ui),
            ui.visuals().strong_text_color(),
            Stroke::NONE,
        ),
        SealTint::Zhu => (paper_color(ui), zhu_color(ui), Stroke::NONE),
        SealTint::Wei => {
            let dim = ui.visuals().weak_text_color();
            (dim, egui::Color32::TRANSPARENT, Stroke::new(1.0, dim))
        }
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), serif_font(SEAL_FONT), fg);
    let width = SEAL_SIZE.max(galley.size().x + 2.0 * SEAL_TEXT_PADDING);
    let (rect, response) = ui.allocate_exact_size(vec2(width, SEAL_SIZE), egui::Sense::hover());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, label.to_owned()));
    ui.painter()
        .rect(rect, SEAL_RADIUS, fill, line, egui::StrokeKind::Inside);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, fg);
}

/// 朱印上印章字的纸色：明暗主题各取接近卡片底的浅字色，保证与朱砂的
/// 对比度。
fn paper_color(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode {
        egui::Color32::from_rgb(0xF2, 0xE9, 0xE4)
    } else {
        egui::Color32::from_rgb(0xFF, 0xF6, 0xF2)
    }
}

/// 朱砂线色（朱丝栏与朱印共用色源）。
fn zhu_color(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode {
        color::SEAL_ZHU_DARK
    } else {
        color::SEAL_ZHU_LIGHT
    }
}

/// 朱丝栏的线色：朱砂按 demo 的 55% 透明档。
fn zhu_line_color(ui: &egui::Ui) -> egui::Color32 {
    let zhu = zhu_color(ui);
    egui::Color32::from_rgba_unmultiplied(zhu.r(), zhu.g(), zhu.b(), ZHU_LINE_ALPHA)
}

/// 宋体字（经/疏/印章）。
fn serif_font(size: f32) -> FontId {
    FontId::new(size, fonts::serif_family())
}

/// 楷体字（注）。
fn kaiti_font(size: f32) -> FontId {
    FontId::new(size, fonts::zhu_family())
}

/// 经区行：墨印 + 正文列（`body` 在列内绘制，显式垂直布局）。
fn jing_section(ui: &mut egui::Ui, text: &Text, body: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal_top(|ui| {
        seal(ui, &text.gloss_popup_seal_jing, SealTint::Ink);
        ui.add_space(SEAL_GAP);
        ui.with_layout(egui::Layout::top_down(Align::LEFT), body);
    });
}

/// 注区行：朱印 + 朱丝栏左线的直接解释（译文/释义/概要）。朱丝栏画在
/// 正文列左缘，线高随正文（跨行延续，demo 的疏密语义）。
fn zhu_section(ui: &mut egui::Ui, text: &Text, body: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal_top(|ui| {
        seal(ui, &text.gloss_popup_seal_zhu, SealTint::Zhu);
        ui.add_space(SEAL_GAP);
        let line_top = ui.cursor().top();
        let column = ui
            .with_layout(egui::Layout::top_down(Align::LEFT), |ui| {
                Frame::new()
                    .inner_margin(Margin {
                        left: ZHU_TEXT_GAP,
                        ..Margin::ZERO
                    })
                    .show(ui, body)
                    .response
            })
            .response
            .rect;
        ui.painter().line_segment(
            [
                egui::pos2(column.min.x, line_top),
                egui::pos2(column.min.x, column.max.y),
            ],
            Stroke::new(ZHU_LINE_WIDTH, zhu_line_color(ui)),
        );
    });
}

/// 疏区行：疏印 + 上缘虚线的小字衍说（译注/例句/小记）。虚线只横贯正文
/// 列（demo shu-wrap 的 border-top 在文字列上，不过印章列）。
fn shu_section(ui: &mut egui::Ui, text: &Text, body: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal_top(|ui| {
        seal(ui, &text.gloss_popup_seal_shu, SealTint::Wei);
        ui.add_space(SEAL_GAP);
        let column = ui
            .with_layout(egui::Layout::top_down(Align::LEFT), |ui| {
                Frame::new()
                    .inner_margin(Margin {
                        top: SHU_TEXT_GAP,
                        ..Margin::ZERO
                    })
                    .show(ui, body)
                    .response
            })
            .response
            .rect;
        let dashes = Shape::dashed_line(
            &[
                egui::pos2(column.min.x, column.min.y),
                egui::pos2(column.max.x, column.min.y),
            ],
            Stroke::new(1.0, ui.visuals().weak_text_color()),
            SHU_DASH_LENGTH,
            SHU_GAP_LENGTH,
        );
        ui.painter().extend(dashes);
    });
}

/// 经区块的正文：原文/词条随任务形制。代码任务为界栏框内嵌代码面板
/// （图样定稿的双层结构：专属底色、等宽体、不换行、左上角语言标签行），
/// 其余为宋体原文。
fn source_block(ui: &mut egui::Ui, source: &str, code: bool, code_lang: Option<&str>) {
    let strong = ui.visuals().strong_text_color();
    if code {
        Frame::new()
            .fill(ui.visuals().faint_bg_color)
            .stroke(Stroke::new(
                stroke::CARD,
                ui.visuals().widgets.noninteractive.bg_stroke.color,
            ))
            .corner_radius(CornerRadius::same(JING_FRAME_RADIUS))
            .inner_margin(Margin::symmetric(
                JING_FRAME_PADDING_H,
                JING_FRAME_PADDING_V,
            ))
            .show(ui, |ui| {
                Frame::new()
                    .fill(code_bg(ui))
                    .stroke(Stroke::new(CODE_PANEL_STROKE, code_border(ui)))
                    .corner_radius(CornerRadius::same(CODE_PANEL_RADIUS))
                    .inner_margin(Margin::symmetric(
                        CODE_PANEL_PADDING_H,
                        CODE_PANEL_PADDING_V,
                    ))
                    .show(ui, |ui| {
                        code_badge(ui, code_lang);
                        // 不换行：超宽由面板内横向滚动兜底（流式视图的经位
                        // 在正文 ScrollArea 之外，横向滚动必须自己带）。
                        ScrollArea::horizontal()
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(source)
                                            .font(FontId::new(CODE_FONT, fonts::mono_family()))
                                            .line_height(Some(CODE_LINE_HEIGHT))
                                            .color(strong),
                                    )
                                    .wrap_mode(egui::TextWrapMode::Extend)
                                    .selectable(true),
                                );
                            });
                    });
            });
        return;
    }
    ui.add(
        egui::Label::new(
            RichText::new(source)
                .font(serif_font(JING_FONT))
                .color(strong),
        )
        .wrap()
        .selectable(true),
    );
}

/// 代码面板底色（明暗随主题，demo code-bg 双档）。
fn code_bg(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode {
        color::CODE_BG_DARK
    } else {
        color::CODE_BG_LIGHT
    }
}

/// 代码面板边色（明暗随主题，demo card-border 的近隐形档）。
fn code_border(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode {
        color::CODE_BORDER_DARK
    } else {
        color::CODE_BORDER_LIGHT
    }
}

/// 语言标签：面板内首行左上的原样小写弱色小字（图样定稿：与浅色一致
/// 的统一面板底，不做标签带；标签行与代码之间空一个代码行高），未知
/// 语言不显示。Label 落字自带无障碍标签（语言进树可检索）。
fn code_badge(ui: &mut egui::Ui, lang: Option<&str>) {
    let Some(lang) = lang else {
        return;
    };
    ui.label(
        RichText::new(lang)
            .font(FontId::new(CODE_BADGE_FONT, FontFamily::Proportional))
            .color(ui.visuals().weak_text_color()),
    )
    .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, lang.to_owned()));
    ui.add_space(CODE_LINE_HEIGHT);
}

/// 注区的文本样式改写：markdown 正文与列表项落到楷体字号（egui_commonmark
/// 经 `ui.style().text_styles` 取字体，改写即生效；代码块仍走等宽族）。
fn apply_zhu_typography(ui: &mut egui::Ui) {
    for (style, size) in [
        (TextStyle::Body, ZHU_FONT),
        (TextStyle::Button, ZHU_FONT),
        (TextStyle::Small, SHU_FONT),
        (TextStyle::Heading, JING_FONT),
    ] {
        ui.style_mut().text_styles.insert(style, kaiti_font(size));
    }
}

/// 骨架条（取材中与「经显注未至」共用）：三根不同长度的圆角条随时间
/// 脉动，无任何文字与无障碍标签——「正在注解」由页脚唯一携带，双标签
/// 会让查询歧义。动画期间请求短重绘；kittest 的固定时间零点让快照
/// 恒定在同一相位。
fn shimmer_bars(ui: &mut egui::Ui) {
    let now = ui.input(|i| i.time);
    let ratios = [0.95, 0.78, 0.6];
    let full = ui.available_width();
    let gap = space::ITEM;
    let total_h = ratios.len() as f32 * SHIMMER_BAR_HEIGHT + (ratios.len() - 1) as f32 * gap;
    let (rect, _response) = ui.allocate_exact_size(vec2(full, total_h), egui::Sense::hover());
    let weak = ui.visuals().weak_text_color();
    for (index, ratio) in ratios.iter().enumerate() {
        let phase = (index as f64) * 0.9;
        let wave = ((now / PULSE_PERIOD_SECS + phase).sin() + 1.0) / 2.0;
        let alpha =
            SHIMMER_ALPHA_MIN as f64 + wave * f64::from(SHIMMER_ALPHA_MAX - SHIMMER_ALPHA_MIN);
        let alpha = (alpha as f32).round().clamp(0.0, 255.0) as u8;
        let bar = egui::Rect::from_min_size(
            egui::pos2(
                rect.min.x,
                rect.min.y + index as f32 * (SHIMMER_BAR_HEIGHT + gap),
            ),
            vec2(full * ratio, SHIMMER_BAR_HEIGHT),
        );
        ui.painter().rect_filled(
            bar,
            SHIMMER_BAR_RADIUS,
            egui::Color32::from_rgba_unmultiplied(weak.r(), weak.g(), weak.b(), alpha),
        );
    }
    ui.ctx()
        .request_repaint_after(Duration::from_secs_f32(0.016));
}

/// 页脚带（常驻）：上缘细线，推理中左端是呼吸点 + 「正在注解」（生成指示
/// 唯一落点，无障碍标签也在这里），右端恒为 Gloss 水印。动画与骨架同源
/// （时间驱动 alpha + 短重绘），仅在推理中请求。
fn footer(ui: &mut egui::Ui, streaming: bool, text: &Text) {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), FOOTER_HEIGHT),
        egui::Sense::hover(),
    );
    let dim = ui.visuals().weak_text_color();
    ui.painter().line_segment(
        [
            egui::pos2(rect.min.x, rect.min.y),
            egui::pos2(rect.max.x, rect.min.y),
        ],
        hairline(ui),
    );
    if streaming {
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::ProgressIndicator,
                true,
                text.gloss_popup_annotating.as_str(),
            )
        });
        let now = ui.input(|i| i.time);
        let zhu = zhu_color(ui);
        let mut dot_center = egui::pos2(
            rect.min.x + FOOTER_DOT_RADIUS,
            rect.center().y + stroke::CARD,
        );
        for index in 0..FOOTER_DOT_COUNT {
            let wave = ((now / PULSE_PERIOD_SECS + index as f64 * 0.9).sin() + 1.0) / 2.0;
            let alpha = (SHIMMER_ALPHA_MIN as f64
                + wave * f64::from(SHIMMER_ALPHA_MAX - SHIMMER_ALPHA_MIN))
                as f32;
            let alpha = alpha.round().clamp(0.0, 255.0) as u8;
            ui.painter().circle_filled(
                dot_center,
                FOOTER_DOT_RADIUS,
                egui::Color32::from_rgba_unmultiplied(zhu.r(), zhu.g(), zhu.b(), alpha),
            );
            dot_center.x += FOOTER_DOT_RADIUS * 3.0;
        }
        ui.painter().text(
            egui::pos2(dot_center.x + space::TIGHT, rect.center().y + stroke::CARD),
            egui::Align2::LEFT_CENTER,
            &text.gloss_popup_annotating,
            FontId::proportional(font::TAG),
            dim,
        );
        ui.ctx()
            .request_repaint_after(Duration::from_secs_f32(0.016));
    }
    // 水印走 Label 而不是 painter 文字：进无障碍树，可检索、可测。槽宽按
    // 水印字串实测宽 + 余量（与印章同一模式），右对齐贴页脚右缘。
    let galley =
        ui.painter()
            .layout_no_wrap(watermark().to_owned(), FontId::proportional(font::TAG), dim);
    let slot = galley.size().x + 2.0 * WATERMARK_PADDING;
    ui.put(
        egui::Rect::from_min_max(
            egui::pos2(rect.max.x - slot, rect.min.y),
            egui::pos2(rect.max.x, rect.max.y),
        ),
        egui::Label::new(RichText::new(watermark()).size(font::TAG).color(dim))
            .selectable(false)
            .halign(Align::RIGHT),
    );
}

/// 页脚水印的品牌名：应用名不翻译（与窗口标题同一原则）。
fn watermark() -> &'static str {
    "Gloss"
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

/// 该任务是否按代码排版（经位走等宽体 + 界栏框）：流式视图看自动分类的
/// 判定，完成态看产物任务的类型。
fn is_code(kind: Option<TaskKind>) -> bool {
    kind == Some(TaskKind::ExplainCode)
}

/// 产物正文（经注疏排布）：词卡三分（词条/释义/例句），句译与代码解释
/// 经（原文）+ 注（markdown 正文），提取任务经（提取文本）+ 疏（小记）。
fn outcome_body(
    ui: &mut egui::Ui,
    source: &str,
    outcome: &gloss_core::task::TaskOutcome,
    code_lang: Option<&str>,
    state: &RenderState,
    text: &Text,
) {
    match &outcome.structured {
        OutcomeStructured::WordCard {
            word,
            phonetic,
            senses,
        } => word_card(ui, word, phonetic.as_deref(), senses, text),
        OutcomeStructured::Plain { title } => {
            if !source.trim().is_empty() {
                jing_section(ui, text, |ui| {
                    source_block(ui, source, is_code(Some(outcome.kind)), code_lang)
                });
                ui.add_space(space::PARAGRAPH);
            }
            zhu_section(ui, text, |ui| {
                apply_zhu_typography(ui);
                if let Some(title) = title {
                    ui.label(
                        RichText::new(title.as_str())
                            .font(kaiti_font(JING_FONT))
                            .strong()
                            .color(ui.visuals().strong_text_color()),
                    );
                    ui.add_space(space::PARAGRAPH);
                }
                render_markdown(ui, state, &outcome.body);
            });
        }
        OutcomeStructured::Extracted { text: extracted } => {
            jing_section(ui, text, |ui| plain_body(ui, extracted));
            ui.add_space(space::PARAGRAPH);
            shu_section(ui, text, |ui| {
                ui.label(extract_note(text, extracted));
            });
        }
    }
}

/// 提取小记（疏）：「凡 N 言 · N 行」，字数按去空白计、行数按换行计，
/// 与 demo 定稿的口径一致。
fn extract_note(catalog: &Text, text: &str) -> RichText {
    let chars = text.chars().filter(|ch| !ch.is_whitespace()).count();
    let lines = text.lines().count().max(1);
    let chars = chars.to_string();
    let lines = lines.to_string();
    RichText::new(fill(
        &catalog.gloss_popup_seal_note,
        &[("chars", &chars), ("lines", &lines)],
    ))
    .font(serif_font(SHU_FONT))
    .weak()
}

/// 例句拆分：在首个 CJK 字形处切成「原文 / 译文」两行（demo w-ex 的
/// `.en` 行 + `.zh` 块）；没有 CJK 段的原样单行返回。
fn example_lines(example: &str) -> (&str, Option<&str>) {
    fn is_cjk(ch: char) -> bool {
        matches!(ch as u32,
            0x3000..=0x303F // CJK 符号与标点
            | 0x3400..=0x4DBF // 扩展 A
            | 0x4E00..=0x9FFF // 基本区
            | 0xF900..=0xFAFF // 兼容表意
            | 0xFF00..=0xFFEF // 全角形式
        )
    }
    match example.char_indices().find(|(_, ch)| is_cjk(*ch)) {
        Some((byte, _)) if !example[..byte].trim().is_empty() => (
            example[..byte].trim_end(),
            Some(example[byte..].trim_start()),
        ),
        _ => (example, None),
    }
}

/// 词卡精排（经注疏三分）：词条 + 音标行落经位（demo w-head），释义逐行
/// 落注位（朱丝栏，楷体，词性朱砂），例句拆行落疏位。
fn word_card(
    ui: &mut egui::Ui,
    word: &str,
    phonetic: Option<&str>,
    senses: &[gloss_core::task::Sense],
    text: &Text,
) {
    jing_section(ui, text, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(word)
                    .font(serif_font(WORD_FONT))
                    .color(ui.visuals().strong_text_color()),
            );
            if let Some(phonetic) = phonetic {
                ui.label(
                    RichText::new(phonetic)
                        .font(FontId::new(PHON_FONT, fonts::mono_family()))
                        .color(ui.visuals().weak_text_color()),
                );
            }
        });
    });
    ui.add_space(space::PARAGRAPH);
    zhu_section(ui, text, |ui| {
        let strong = ui.visuals().strong_text_color();
        for sense in senses {
            ui.horizontal_wrapped(|ui| {
                if let Some(pos) = &sense.pos {
                    ui.label(
                        RichText::new(pos.as_str())
                            .font(kaiti_font(PHON_FONT))
                            .color(zhu_color(ui)),
                    );
                    ui.add_space(space::TIGHT);
                }
                ui.label(
                    RichText::new(sense.meaning.as_str())
                        .font(kaiti_font(ZHU_FONT))
                        .color(strong),
                );
            });
            ui.add_space(space::ITEM);
        }
    });
    let has_examples = senses.iter().any(|sense| !sense.examples.is_empty());
    if has_examples {
        ui.add_space(space::PARAGRAPH);
        shu_section(ui, text, |ui| {
            let weak = ui.visuals().weak_text_color();
            for sense in senses {
                for example in &sense.examples {
                    let (source, translation) = example_lines(example);
                    ui.label(
                        RichText::new(format!("· {source}")) // i18n:allow 列表符号，非 locale 文案
                            .font(serif_font(SHU_FONT))
                            .italics()
                            .color(weak),
                    );
                    if let Some(translation) = translation {
                        ui.label(
                            RichText::new(translation)
                                .font(serif_font(SHU_FONT))
                                .color(weak),
                        );
                    }
                    ui.add_space(space::INLINE);
                }
            }
        });
    }
}

/// 可选中、自动换行的纯文本正文（OCR 提取文本不按 markdown 解释）。
fn plain_body(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .font(serif_font(JING_FONT))
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
        RichText::new("敏捷的棕色狐狸从懒狗身上跳过。") // i18n:allow 自检字样，字体链路夹具
            .size(font::TITLE)
            .strong()
            .color(strong),
    );
    ui.add_space(space::SECTION);
    ui.label(
        RichText::new("中文渲染自检：划词翻译、代码解释、图片识别。") // i18n:allow 自检字样，字体链路夹具
            .size(font::NOTICE)
            .color(weak),
    );
}

#[cfg(test)]
mod tests {
    use super::{
        APP_ICON_PNG, MAX_WIDTH, WIDTH, decode_app_icon, example_lines, resolve_width,
        stream_visible_body, watermark,
    };

    #[test]
    fn decode_app_icon_rejects_bad_bytes() {
        assert!(decode_app_icon(b"not a png").is_none());
    }

    #[test]
    fn decode_app_icon_crops_to_the_content_square() {
        let image = decode_app_icon(APP_ICON_PNG).expect("embedded icon must decode");
        assert_eq!(image.width(), 206, "256 * 824 / 1024");
        assert_eq!(image.height(), 206, "256 * 824 / 1024");
    }

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

    #[test]
    fn example_lines_split_at_the_first_cjk_glyph() {
        let (en, zh) =
            example_lines("The polished wood had a deep gloss. 那块抛光的木料泛着深沉的光泽。");
        assert_eq!(en, "The polished wood had a deep gloss.");
        assert_eq!(zh, Some("那块抛光的木料泛着深沉的光泽。"));

        let (en, zh) = example_lines("a gloss of silk");
        assert_eq!(en, "a gloss of silk");
        assert_eq!(zh, None);

        let (en, zh) = example_lines("光泽");
        assert_eq!(en, "光泽");
        assert_eq!(zh, None);

        let (en, zh) = example_lines("英译。中译");
        assert_eq!(en, "英译。中译");
        assert_eq!(zh, None, "开头即 CJK 的例句不拆");
    }

    #[test]
    fn watermark_is_the_untranslated_brand_name() {
        assert_eq!(watermark(), "Gloss");
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
        OverlayView::Outcome {
            source: String::new(),
            outcome: TaskOutcome {
                kind: TaskKind::TranslateWord,
                body: "markdown 正文".into(),
                structured: OutcomeStructured::WordCard {
                    word: "gloss".into(),
                    phonetic: Some("/ɡlɒs/".into()),
                    senses: vec![
                        Sense {
                            pos: Some("n.".into()),
                            meaning: "光泽；注释".into(),
                            examples: vec!["a gloss of silk".into()],
                        },
                        Sense {
                            pos: Some("v.".into()),
                            meaning: "作注解".into(),
                            examples: vec![],
                        },
                    ],
                },
            },
            code_lang: None,
        }
    }

    fn streaming_view() -> OverlayView {
        OverlayView::Streaming {
            source: "选中的原文".into(),
            body: "已流式到达的正文\n```gloss\n{\"title\":\"摘要\"}\n```".into(),
            classified: Some(TaskKind::TranslateWord),
            code_lang: None,
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
        OverlayView::Outcome {
            source: String::new(),
            outcome: TaskOutcome {
                kind: TaskKind::TranslateSentence,
                body: "很长的正文段落。".repeat(1000) + "尾部标记",
                structured: OutcomeStructured::Plain { title: None },
            },
            code_lang: None,
        }
    }

    fn extract_view() -> OverlayView {
        OverlayView::Outcome {
            source: String::new(),
            outcome: TaskOutcome {
                kind: TaskKind::ImageOcr,
                body: String::new(),
                structured: OutcomeStructured::Extracted {
                    text: "会议纪要\n参会：产品组、评测组".into(),
                },
            },
            code_lang: None,
        }
    }

    fn word_card_view_en() -> OverlayView {
        OverlayView::Outcome {
            source: String::new(),
            outcome: TaskOutcome {
                kind: TaskKind::TranslateWord,
                body: String::new(),
                structured: OutcomeStructured::WordCard {
                    word: "gloss".into(),
                    phonetic: Some("/ɡlɒs/".into()),
                    senses: vec![
                        Sense {
                            pos: Some("n.".into()),
                            meaning: "a surface shine; luster".into(),
                            examples: vec!["The polished wood had a deep gloss.".into()],
                        },
                        Sense {
                            pos: Some("v.".into()),
                            meaning: "to add a gloss or commentary".into(),
                            examples: vec![],
                        },
                    ],
                },
            },
            code_lang: None,
        }
    }

    fn streaming_view_en() -> OverlayView {
        OverlayView::Streaming {
            source: "It is not that I am so smart.".into(),
            body: "Partial body already streamed.\n```gloss\n{\"title\":\"Summary\"}\n```".into(),
            classified: Some(TaskKind::TranslateSentence),
            code_lang: None,
        }
    }

    fn code_streaming_view_en() -> OverlayView {
        OverlayView::Streaming {
            source: "fn main() {\n    let gloss = \"光\";\n    println!(\"{gloss}\");\n}".into(),
            body: "Partial explanation already streamed.\n```gloss\n{\"title\":\"Rust\"}\n```"
                .into(),
            classified: Some(TaskKind::ExplainCode),
            code_lang: Some("rust".into()),
        }
    }

    fn code_outcome_view_en() -> OverlayView {
        OverlayView::Outcome {
            source: "fn main() {\n    let gloss = \"光\";\n    println!(\"{gloss}\");\n}".into(),
            outcome: TaskOutcome {
                kind: TaskKind::ExplainCode,
                body: "### What it does\n\nPrints the CJK word for *gloss*.".into(),
                structured: OutcomeStructured::Plain {
                    title: Some("Rust snippet".into()),
                },
            },
            code_lang: Some("rust".into()),
        }
    }

    fn extract_view_en() -> OverlayView {
        OverlayView::Outcome {
            source: String::new(),
            outcome: TaskOutcome {
                kind: TaskKind::ImageOcr,
                body: String::new(),
                structured: OutcomeStructured::Extracted {
                    text: "Meeting notes\nAttendees: product, client, eval".into(),
                },
            },
            code_lang: None,
        }
    }

    type Clicked = Rc<RefCell<Option<OverlayAction>>>;

    fn font_first_frame(installed: &Cell<bool>, ctx: &egui::Context) -> bool {
        if installed.replace(true) {
            return false;
        }
        crate::ui::context::install_kittest_fonts(ctx);
        ctx.request_repaint();
        true
    }

    fn harness_for(view: OverlayView) -> (Harness<'static>, Clicked) {
        harness_for_locale(view, Locale::Zh)
    }

    fn harness_for_locale(view: OverlayView, locale: Locale) -> (Harness<'static>, Clicked) {
        let clicked: Clicked = Rc::new(RefCell::new(None));
        let sink = Rc::clone(&clicked);
        let state = RenderState::default();
        let text = Text::get(locale);
        let installed = Cell::new(false);
        let harness = Harness::new_ui(move |ui| {
            if font_first_frame(&installed, ui.ctx()) {
                return;
            }
            let output = draw(ui, Some(&view), &state, text);
            if let Some(action) = output.action {
                *sink.borrow_mut() = Some(action);
            }
        });
        (harness, clicked)
    }

    fn snapshot_harness(view: Option<OverlayView>) -> Harness<'static> {
        let state = RenderState::default();
        let text = Text::get(Locale::En);
        let installed = Cell::new(false);
        Harness::builder()
            .with_theme(egui::Theme::Light)
            .build_ui(move |ui| {
                if font_first_frame(&installed, ui.ctx()) {
                    return;
                }
                let _ = draw(ui, view.as_ref(), &state, text);
            })
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
        harness.get_by_label("作注解");
        harness.get_by_label("· a gloss of silk");
    }

    #[test]
    fn word_card_senses_stack_vertically() {
        let (mut harness, _clicked) = harness_for(word_card_view());
        harness.run();
        let first = harness.get_by_label("光泽；注释").rect();
        let second = harness.get_by_label("作注解").rect();
        assert!(
            second.top() > first.top(),
            "释义必须逐行向下排（正文列显式垂直布局），不能横向并排: {first:?} {second:?}"
        );
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
    fn acquiring_view_shows_a_bare_skeleton() {
        let (mut harness, _clicked) = harness_for(acquiring_view());
        harness.run_steps(3);
        harness.get_by_label_contains("正在注解");
        let fetching_copy = harness
            .query_all_by_label_contains("正在读取选区")
            .next()
            .is_some();
        assert!(
            !fetching_copy,
            "the skeleton carries no fetching copy anywhere"
        );
        let fence_visible = harness.query_all_by_label_contains("原文").next().is_some();
        assert!(
            !fence_visible,
            "the skeleton carries no source or streaming content"
        );
    }

    #[test]
    fn streaming_view_shows_the_annotating_footer() {
        let (mut harness, _clicked) = harness_for(streaming_view());
        harness.run_steps(3);
        harness.get_by_label_contains("正在注解");
        let seals = [
            harness.query_all_by_label_contains("经").next().is_some(),
            harness.query_all_by_label_contains("注").next().is_some(),
        ];
        assert!(
            seals.iter().all(|present| *present),
            "the streaming card marks the source and the body sections"
        );
    }

    #[test]
    fn watermark_sits_inside_the_computed_window_height() {
        for view in [streaming_view(), word_card_view(), acquiring_view()] {
            let state = RenderState::default();
            let text = Text::get(Locale::Zh);
            let sizing: Rc<Cell<OverlaySizing>> = Rc::new(Cell::new(OverlaySizing {
                width: 0.0,
                height: 0.0,
            }));
            let sink = Rc::clone(&sizing);
            let installed = Cell::new(false);
            let mut harness = Harness::new_ui(move |ui| {
                if font_first_frame(&installed, ui.ctx()) {
                    return;
                }
                sink.set(draw(ui, Some(&view), &state, text).sizing);
            });
            harness.run_steps(2);
            harness.set_size(vec2(sizing.get().width, sizing.get().height));
            harness.run_steps(1);
            let watermark = harness.get_by_label("Gloss").rect();
            assert!(
                watermark.bottom() <= sizing.get().height + 0.5,
                "页脚水印必须落在期望窗口高度之内（页脚被内容挤出窗外即此断言失败）: \
                 watermark_bottom={} sizing_height={}",
                watermark.bottom(),
                sizing.get().height
            );
        }
    }

    #[test]
    fn word_card_marks_the_three_sections_per_locale() {
        let (mut harness, _clicked) = harness_for(word_card_view());
        harness.run();
        for seal in ["经", "注", "疏"] {
            harness.get_by_label(seal);
        }

        let (mut harness, _clicked) = harness_for_locale(word_card_view(), Locale::En);
        harness.run();
        for seal in ["SRC", "NOTE", "EXP"] {
            harness.get_by_label(seal);
        }
    }

    #[test]
    fn extract_view_notes_the_measurement() {
        let (mut harness, _clicked) = harness_for(extract_view());
        harness.run();
        harness.get_by_label_contains("会议纪要");
        harness.get_by_label_contains("凡 14 言 · 2 行");
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
    fn code_views_expose_the_language_badge_and_prose_untouched() {
        let (mut harness, _clicked) = harness_for(code_streaming_view_en());
        harness.run_steps(3);
        harness.get_by_label("rust");

        let (mut harness, _clicked) = harness_for(streaming_view_en());
        harness.run_steps(3);
        assert!(
            harness.query_all_by_label_contains("rust").next().is_none(),
            "non-code tasks carry no language badge"
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
        let installed = Cell::new(false);
        let mut harness = Harness::new_ui(move |ui| {
            if font_first_frame(&installed, ui.ctx()) {
                return;
            }
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

        let mut harness = snapshot_harness(Some(word_card_view_en()));
        harness.run();
        harness.snapshot("popup_word_card");
        results.extend_harness(&mut harness);

        let mut harness = snapshot_harness(Some(acquiring_view()));
        harness.run_steps(3);
        harness.snapshot("popup_loading");
        results.extend_harness(&mut harness);

        let mut harness = snapshot_harness(Some(extract_view_en()));
        harness.run();
        harness.snapshot("popup_extract");
        results.extend_harness(&mut harness);

        let mut harness = snapshot_harness(Some(streaming_view_en()));
        harness.run_steps(3);
        harness.snapshot("popup_streaming");
        results.extend_harness(&mut harness);

        let mut harness = snapshot_harness(Some(failed_view()));
        harness.run();
        harness.snapshot("popup_failed");
        results.extend_harness(&mut harness);

        let mut harness = snapshot_harness(Some(auth_failed_view()));
        harness.run();
        harness.snapshot("popup_failed_auth");
        results.extend_harness(&mut harness);

        let mut harness = snapshot_harness(Some(code_streaming_view_en()));
        harness.run_steps(3);
        harness.snapshot("popup_code_streaming");
        results.extend_harness(&mut harness);

        let mut harness = snapshot_harness(Some(code_outcome_view_en()));
        harness.run();
        harness.snapshot("popup_code_outcome");
        results.extend_harness(&mut harness);

        let mut harness = snapshot_harness(None);
        harness.run();
        harness.snapshot("popup_selfcheck");
        results.extend_harness(&mut harness);

        results.unwrap();
    }
}
