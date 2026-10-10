//! 浮层内容：按视图分发的骨架、经注疏结果卡与流式/失败视图。
//!
//! 内容按「经 · 注 · 疏」三层组织，视觉定稿以 docs/demo/popup-redesign.html
//! 为准（本地文件，不进 git）：
//! - **经**＝选区原文/词条/提取文本（宋体；代码任务为单层代码面板
//!   ——专属底色、`gloss-mono` 等宽体、不换行横滚、左上角语言标签行）；
//! - **注**＝译文/释义/概要（楷体，系统缺楷体时随 demo 回退链落宋体；
//!   朱丝栏左线 + 朱印）；
//! - **疏**＝译注/例句/小记（小号宋体、弱色、上缘虚线 + 疏印）。
//!
//! 三分区的正文列都显式声明垂直布局：egui 的 Frame/子 Ui 会继承父级的
//! 水平布局（`new_child` 不带 layout 参数），横排父级里多条目正文会从左往
//! 右流（释义并排、溢出右缘）。分区的间距是本模块私有量，字号规格与宋楷
//! 字体助手在 [`content`]（都是 demo 定稿值），不进 style 阶梯。
//!
//! 决策纯函数不进绘制模板，按职责拆在子模块：[`note`]（流式 JSON 渐进
//! 提取）、[`sizing`]（宽度滞回）、[`icon`]（图标解码裁剪）、[`content`]
//! （文案整形）；本模块只留绘制模板与面板编排，依赖方向恒为绘制 → 决策。
//!
//! 词卡精排（词条/音标落经位，释义逐行落注位，例句落疏位），其余任务正文
//! 走 markdown（egui_commonmark 渲染；注区统一改写文本样式为楷体字号）。
//! 划选即复制：全部文本可选中，无独立复制按钮。正文完整渲染不截断，高度
//! 自适应内容（宽度默认 380、上限 480，高度上限按屏幕），超出部分滚动兜底。
//! 流式视图对累积的原始流做**转义感知**的 `note`（注）渐进提取（现行输出
//! 契约是纯 JSON 对象，见 `gloss_core::prompt`）：`note` 键未到齐时正文区
//! 落骨架、页脚保留「正在注解」；旧契约（markdown + 围栏）不含 `note` 键，
//! 全程进度态，完成态由 finalize 的围栏 fallback 兜住。注文一律换行排版：
//! 代码面板等宽折行、markdown 由 egui_commonmark 按可用宽折行，横滚不进
//! 弹窗。
//!
//! 页头回归品牌：只有应用图标与动作区（⚙/×），任务与状态由内容层自明，
//! 页头不带任何标签药丸；行下发丝线与页脚上缘线呼应成卡片的上下界。
//! 页头行同时是拖动热区：按住拖动即移动浮层（壳侧逐帧平移窗口），热区
//! 与动作钮零重叠，按钮点击不受影响。
//! 页脚常驻
//! 一条窄带（带高取 [`FOOTER_HEIGHT`]）：推理中左端是呼吸点 + 「正在注解」（生成指示唯一落点），右端
//! 恒为 Gloss 水印（品牌名不翻译，与窗口标题同一原则）；滚动区按页脚带宽
//! 预留视口，页脚不被内容挤出窗外。「经显注未至」时正文区是纯骨架
//! （脉动条），无占位文字。动作点击经 draw 返回 [`OverlayAction`] 上交壳执行；
//! 拖动热区的指针位移经 [`PopupOutput::drag`] 上交壳换算。
//!
//! 敏感信息防护不在这里：两条闸门都不出浮层（见 `gloss_app::machine`），
//! 因此也没有「疑似敏感」这张卡。

mod content;
mod icon;
mod image;
mod note;
mod sizing;

use std::sync::Arc;

use content::{
    JING_FONT, PHON_FONT, SEAL_FONT, SHU_FONT, WORD_FONT, ZHU_FONT, example_lines, extract_note,
    kaiti_font, serif_font, watermark,
};
use icon::app_icon_image;
use image::decode_attached;
use note::{StreamFields, stream_fields};
pub use sizing::WIDTH;
use sizing::resolve_width;

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::time::Duration;

use egui::{
    Align, CornerRadius, FontFamily, FontId, Frame, Margin, RichText, ScrollArea, Shape, Stroke,
    TextStyle, vec2,
};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use gloss_core::task::{OutcomeStructured, TaskKind};

use super::code_hl;
use super::fonts;
use super::style::{color, font, radius, space, stroke};
use crate::machine::{ErrorAction, FailureCause, OverlayView};
use crate::ui::i18n::Text;

/// 页头应用图标的边长
const HEADER_ICON: f32 = 20.0;
/// 头部齿轮的字形尺寸
const ACTION_ICON_SIZE: f32 = 16.0;
/// 头部动作钮的方块边长：齿轮与关闭的命中区统一到这个盒子
const ACTION_BUTTON: f32 = 20.0;
/// 关闭 × 的半臂长与线宽：× 本体 10×10，较齿轮字形偏小
const CLOSE_ARM: f32 = 5.0;
const CLOSE_STROKE: f32 = 2.0;
/// 页头拖动热区右缘与最左动作钮的间隙：热区不碰到按钮命中盒，按压
/// 永远落在其中之一，不存在「点按钮变成拖动」的边界。
const DRAG_STRIP_INSET: f32 = 6.0;
/// 出现动画时长（淡入，秒）：显示/重显后的第一帧从 0 渐进到 1。
const APPEAR_SECONDS: f32 = 0.18;
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
/// 代码面板的圆角、边宽与内边距（demo code 面板：radius 10、1px 边、
/// 11px 13px）；代码底色直接铺满经位区块，无外层界栏框。
const CODE_PANEL_RADIUS: u8 = 10;
const CODE_PANEL_STROKE: f32 = 1.0;
const CODE_PANEL_PADDING_V: i8 = 11;
const CODE_PANEL_PADDING_H: i8 = 13;
/// 代码正文的字号与行高（demo 定稿：12px、行高 1.65）。
const CODE_FONT: f32 = 12.0;
const CODE_LINE_HEIGHT: f32 = CODE_FONT * 1.65;
/// 语言标签的字号（图样定稿：弱色小标，面板内独立行）。
const CODE_BADGE_FONT: f32 = 10.0;
/// 骨架条高（「经显注未至」的占位行）
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
pub struct RenderState {
    /// markdown 渲染状态（egui_commonmark 要求跨帧持有）。
    pub cache: RefCell<CommonMarkCache>,
    /// 上一帧应用的浮层宽度（宽度收敛的滞回状态）。
    pub last_width: Cell<f32>,
    /// 页头拖动的按压点（**物理像素**，按压帧的窗口内逻辑坐标乘当下
    /// `pixels_per_point`）：拖动开始帧记录，结束即清。存物理口径是为
    /// 跨 DPI 显示器的拖动——逻辑坐标随新显示器的缩放比例重标定，逻辑
    /// 按压点会混尺；物理位移按当下比例还原成当下尺度的逻辑位移，与壳
    /// 侧落点换算（`apply_overlay_drag`）同口径。清零后 egui 侧若仍有
    /// 残留拖动所有权（见 [`drag_strip`]），热区保持惰性。
    drag_press_phys: Cell<Option<egui::Pos2>>,
    /// 页头应用图标的纹理（每个 egui 上下文一份，惰性装入）。None＝尚未
    /// 装入或解码失败；失败时每帧重试的成本只有一次常量读取，不再单设
    /// 失败标记。
    icon: RefCell<Option<egui::TextureHandle>>,
    /// 经位附件图像的跨帧纹理缓存槽（见 [`AttachedTexture`]）。
    attached: RefCell<Option<AttachedTexture>>,
}

/// 经位附件图像的纹理缓存：键是源字节的 `Arc` 指针身份——同图跨帧与
/// 重试恒复用同一纹理，绝不逐帧解码；纹理 `None`＝这份字节解码失败
/// （同样只试一次，占位降级），换图（指针不同）即重新解码。同时至多
/// 驻留一份图像纹理（新图即替换；会话离开图像卡即清空，见
/// `RenderState::drop_attached`）。
struct AttachedTexture {
    source: Arc<[u8]>,
    texture: Option<egui::TextureHandle>,
}

impl Default for RenderState {
    fn default() -> Self {
        Self {
            cache: RefCell::new(CommonMarkCache::default()),
            last_width: Cell::new(WIDTH),
            drag_press_phys: Cell::new(None),
            icon: RefCell::new(None),
            attached: RefCell::new(None),
        }
    }
}

impl RenderState {
    /// 清除页头拖动的按压点：壳在收起/显示浮层的边界调用（见
    /// `app::overlay`）。跨显示残留的拖动状态由此失效——egui 的拖动
    /// 所有权可能因隐藏期丢失的鼠标释放而残留（按压点已清，热区保持
    /// 惰性）；残留所有权要到下一次释放事件才清零，其间的第一次按压
    /// 拖动不产生 `drag_started`（所有权无 None→Some 迁移），整段惰性，
    /// 松开即自愈——之后（所有权已清）的按压才重新握点。
    pub(crate) fn reset_drag_state(&self) {
        self.drag_press_phys.set(None);
    }

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

    /// 经位附件图像的纹理；首次见到这份字节时解码并装入当前上下文，之后
    /// 跨帧复用（解码失败按 `None` 缓存，同字节不重试；换字节即重解码）。
    fn attached_texture(
        &self,
        ctx: &egui::Context,
        png: &Arc<[u8]>,
    ) -> Option<egui::TextureHandle> {
        let mut slot = self.attached.borrow_mut();
        if let Some(cached) = slot.as_ref()
            && Arc::ptr_eq(&cached.source, png)
        {
            return cached.texture.clone();
        }
        let texture = decode_attached(png).map(|image| {
            ctx.load_texture("gloss_attached_image", image, egui::TextureOptions::LINEAR)
        });
        *slot = Some(AttachedTexture {
            source: Arc::clone(png),
            texture: texture.clone(),
        });
        texture
    }

    /// 清空经位附件图像的缓存槽：源字节 `Arc` 与解码纹理一并释放。壳在
    /// 无附件的帧调用（图像会话已被收起/替换/失败）——图像字节属敏感数
    /// 据，生命周期不得长于会话；同图下一帧重新解码即回（一次解码，不
    /// 在会话内重复发生）。
    fn drop_attached(&self) {
        *self.attached.borrow_mut() = None;
    }
}

/// 一帧浮层绘制的产物：动作上交 + 内容期望的窗口尺寸（逻辑点）。
pub struct PopupOutput {
    /// 本帧上交壳执行的动作；`None`＝无动作。
    pub action: Option<OverlayAction>,
    /// 本帧内容期望的浮层尺寸（壳据此调整窗口）。
    pub sizing: OverlaySizing,
    /// 页头拖动热区的状态：`Some(offset)`＝拖动进行中，offset 是指针自
    /// 按压点起的**累计**位移（窗口内相对坐标，逻辑点；按下帧为零），
    /// `None`＝本帧不在拖动。壳以它换算窗口落点（见
    /// `WindowManager::apply_overlay_drag`）。
    pub drag: Option<egui::Vec2>,
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
/// `view` 为 `None` 时显示渲染自检卡（预热与自检路径）。`attached` 是
/// 本会话经位附件图像（图像任务的原文，状态机留存，经访问器随行——
/// 不进 [`OverlayView`] 变体形状），仅图像卡渲染它。返回本帧绘制的
/// 产物——失败卡动作与头部动作区（齿轮/×）上交壳执行——浮层只渲染、不
/// 副作用；期望尺寸由壳经窗口管理器应用（内容自适应高度，超出屏幕滚动兜底）。
pub fn draw(
    ui: &mut egui::Ui,
    view: Option<&OverlayView>,
    attached: Option<&Arc<[u8]>>,
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

    // 本帧无附件＝会话已离开图像卡（收起、换任务或失败）：立即清空纹理
    // 缓存槽——图像字节属敏感数据，源字节与解码纹理都不得驻留到会话之外
    // （机器侧清 attached_image 的渲染侧镜像，见 `RenderState::drop_attached`）。
    if attached.is_none() {
        state.drop_attached();
    }

    let fill = ui.visuals().window_fill;
    let window_stroke = ui.visuals().window_stroke;
    let mut output = PopupOutput {
        action: None,
        sizing: OverlaySizing {
            width: state.last_width.get(),
            height: 0.0,
        },
        drag: None,
    };
    let mut content_h = 0.0;
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(stroke::CARD, window_stroke.color))
        .corner_radius(CornerRadius::same(radius::CARD))
        .inner_margin(Margin::same(space::CARD_PADDING))
        .show(ui, |ui| {
            let (action, drag) = render_content(ui, view, attached, state, &mut content_h, text);
            output.action = action;
            output.drag = drag;
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

/// 滚动区视口上限：可用高扣掉页脚带预留，极矮窗口下仍有滚动下限。
fn viewport_max(ui: &egui::Ui) -> f32 {
    (ui.available_height() - FOOTER_RESERVE).max(MIN_BODY_VIEWPORT)
}

/// 流式正文（注位渐进 markdown + 疏位已到达的解读条目）：两字段全空落
/// 骨架（经已回显、注未至——无注印，还没有可注的内容）。
fn streamed_sections(ui: &mut egui::Ui, fields: &StreamFields, state: &RenderState, text: &Text) {
    if fields.note.is_empty() && fields.interpretation.is_empty() {
        shimmer_bars(ui);
        return;
    }
    if !fields.note.is_empty() {
        zhu_section(ui, text, |ui| {
            apply_zhu_typography(ui);
            render_markdown(ui, state, &fields.note);
        });
    }
    if !fields.interpretation.is_empty() {
        ui.add_space(space::PARAGRAPH);
        shu_examples(ui, &fields.interpretation, text);
    }
}

/// 经位附件图像：纹理经 [`RenderState`] 跨帧复用（同字节恒同纹理，绝不
/// 逐帧解码），宽度随卡宽等比收缩；解码失败落占位降级（字节进不了纹理
/// 不是 panic 面）。
fn attached_image(ui: &mut egui::Ui, png: &Arc<[u8]>, state: &RenderState, text: &Text) {
    match state.attached_texture(ui.ctx(), png) {
        Some(texture) => {
            ui.add(egui::Image::new(&texture).max_width(ui.available_width()));
        }
        None => image_unavailable(ui, text),
    }
}

/// 经位图像的占位降级：图像缺席（无可渲染的字节）或解码失败时，经位落
/// 一行弱色占位，注/疏照常。
fn image_unavailable(ui: &mut egui::Ui, text: &Text) {
    ui.label(
        RichText::new(&text.gloss_popup_image_unavailable)
            .font(serif_font(SHU_FONT))
            .weak(),
    );
}

/// 浮层内容（头部 + 各视图正文），并把完整内容高记入 `content_h`：
/// 产物与流式正文放进 ScrollArea（完整渲染、超出滚动兜底），其高度取
/// ScrollArea 报告的内容尺寸，不受视口裁剪影响；页脚带恒在（滚动区按
/// [`FOOTER_RESERVE`] 预留视口，页脚不被内容挤出窗外）。头部动作区与
/// 失败卡动作按钮的点击结果、页头拖动热区的位移一并透传给调用方。
fn render_content(
    ui: &mut egui::Ui,
    view: Option<&OverlayView>,
    attached: Option<&Arc<[u8]>>,
    state: &RenderState,
    content_h: &mut f32,
    text: &Text,
) -> (Option<OverlayAction>, Option<egui::Vec2>) {
    match view {
        None => {
            let (action, drag) = header(ui, state, text);
            ui.add_space(space::SECTION);
            selfcheck_body(ui);
            ui.add_space(space::PARAGRAPH);
            footer(ui, false, text);
            *content_h = ui.min_rect().height();
            (action, drag)
        }
        Some(OverlayView::Streaming {
            source,
            raw,
            classified,
            code_lang,
        }) => {
            let (action, drag) = header(ui, state, text);
            ui.add_space(space::SECTION);
            let code = is_code(*classified);
            let fields = stream_fields(raw);
            match attached {
                // 图像卡：经位图像与注/疏一起进滚动区——高图由滚动区吞
                // 高，窗口按完整内容高申请、壳侧钳到屏，超出滚动兜底。
                Some(png) => {
                    let scrolled = ScrollArea::new([false, true])
                        .auto_shrink([false, true])
                        .max_height(viewport_max(ui))
                        .show(ui, |ui| {
                            jing_section(ui, text, |ui| attached_image(ui, png, state, text));
                            ui.add_space(space::PARAGRAPH);
                            streamed_sections(ui, &fields, state, text);
                        });
                    ui.add_space(space::PARAGRAPH);
                    footer(ui, true, text);
                    record_scrolled_height(ui, content_h, &scrolled);
                }
                None => {
                    jing_section(ui, text, |ui| {
                        source_block(ui, source, code, code_lang.as_deref())
                    });
                    ui.add_space(space::PARAGRAPH);
                    // ScrollArea 内容起点 = cursor（egui 的 cursor 停在
                    // 前序内容底边加一个 item_spacing 处），从这里起算
                    // 正文完整高。
                    let scrolled = ScrollArea::new([false, true])
                        .auto_shrink([false, true])
                        .max_height(viewport_max(ui))
                        .show(ui, |ui| streamed_sections(ui, &fields, state, text));
                    ui.add_space(space::PARAGRAPH);
                    footer(ui, true, text);
                    record_scrolled_height(ui, content_h, &scrolled);
                }
            }
            (action, drag)
        }
        Some(OverlayView::Outcome {
            source,
            outcome,
            code_lang,
        }) => {
            let (action, drag) = header(ui, state, text);
            ui.add_space(space::SECTION);
            let viewport_max = viewport_max(ui);
            // 只纵向滚动：与流式视图同规，代码正文按可用宽折行。
            let scrolled = ScrollArea::new([false, true])
                .auto_shrink([false, true])
                .max_height(viewport_max)
                .show(ui, |ui| {
                    outcome_body(
                        ui,
                        source,
                        outcome,
                        code_lang.as_deref(),
                        attached,
                        state,
                        text,
                    );
                });
            ui.add_space(space::PARAGRAPH);
            footer(ui, false, text);
            record_scrolled_height(ui, content_h, &scrolled);
            (action, drag)
        }
        Some(OverlayView::Failed {
            cause,
            action: error_action,
        }) => {
            let (mut action, drag) = header(ui, state, text);
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
            (action, drag)
        }
    }
}

/// 失败卡文案：按失败来源映射（静态词条借自常驻文案表，带占位符的
/// 变体才拼新串）。
fn failure_message<'a>(cause: &'a FailureCause, text: &'a Text) -> Cow<'a, str> {
    match cause {
        FailureCause::Task(error) => text.for_error(error),
        FailureCause::TransportChannel => Cow::Borrowed(&text.gloss_errors_inference_channel),
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
/// 隔开。行身同时是拖动热区（见 [`RenderState::drag_press_phys`] 与
/// [`drag_strip`]），返回（动作区点击，拖动热区状态）。
fn header(
    ui: &mut egui::Ui,
    state: &RenderState,
    text: &Text,
) -> (Option<OverlayAction>, Option<egui::Vec2>) {
    let weak = ui.visuals().weak_text_color();
    let strong = ui.visuals().strong_text_color();
    let mut action = None;
    let mut drag = None;
    // 最左动作钮（齿轮）的左缘：拖动热区到它为止，动作钮的点击不被
    // 热区覆盖。
    let mut gear_left = f32::INFINITY;
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
            gear_left = gear.rect.left();
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
        drag = drag_strip(ui, state, gear_left, text);
    });
    // 发丝线与行内容（图标/动作钮）之间留一档间距，不贴着字形底边。
    ui.add_space(space::ITEM);
    let line_y = ui.cursor().top();
    ui.painter()
        .hline(ui.max_rect().x_range(), line_y, hairline(ui));
    (action, drag)
}

/// 拖动热区的指针位移上交：按下帧确立握点（位移为零），之后每帧
/// 上报指针自按压点起的累计位移（按压点按物理像素记录，跨 DPI 显示器
/// 不混尺）；松开/未拖动返回 `None` 并清按压点。
///
/// 残留所有权保持惰性：egui 的拖动所有权只经按压事件授予（`egui`
/// `interaction.rs` 的 `PointerEvent::Pressed` 分支），浮层隐藏期间丢失
/// 的鼠标释放会让它跨显示残留；此时按压点已被壳在显隐边界清掉（
/// [`RenderState::reset_drag_state`]），本函数对无按压点的拖动一概不
/// 上交位移——窗口不会跟着无按键的指针走。残留所有权在下一个释放
/// 事件清零，其间的第一次按压拖动整段惰性（无所有权迁移、
/// `drag_started` 不触发），松开即自愈，之后的按压才重新握点。
fn drag_strip(
    ui: &mut egui::Ui,
    state: &RenderState,
    gear_left: f32,
    text: &Text,
) -> Option<egui::Vec2> {
    let row = ui.min_rect();
    let right = (gear_left - DRAG_STRIP_INSET).max(row.left());
    let rect = egui::Rect::from_min_max(
        egui::pos2(row.left(), row.top()),
        egui::pos2(right, row.bottom()),
    );
    let response = ui.interact(
        rect,
        egui::Id::new("overlay_header_drag"),
        egui::Sense::drag(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Other,
            true,
            text.gloss_popup_drag_label.as_str(),
        )
    });
    let cursor = if response.dragged() {
        egui::CursorIcon::Grabbing
    } else {
        egui::CursorIcon::Grab
    };
    let response = response.on_hover_cursor(cursor);
    let (pointer, pixels_per_point) = ui.input(|i| (i.pointer.interact_pos(), i.pixels_per_point));
    if response.drag_started() {
        state
            .drag_press_phys
            .set(pointer.map(|pos| pos * pixels_per_point));
        return Some(egui::Vec2::ZERO);
    }
    if !response.dragged() {
        state.drag_press_phys.set(None);
        return None;
    }
    let press_phys = state.drag_press_phys.get()?;
    let pointer_phys = pointer? * pixels_per_point;
    Some((pointer_phys - press_phys) / pixels_per_point)
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

/// 经区块的正文：原文/词条随任务形制。代码任务为单层代码面板——代码
/// 底色直接铺满经位区块（用户反馈定稿：无外层底色的界栏框），配等宽
/// 体、不换行、左上角语言标签行与单趟正则的六类语法着色；其余为宋体
/// 原文。
fn source_block(ui: &mut egui::Ui, source: &str, code: bool, code_lang: Option<&str>) {
    let strong = ui.visuals().strong_text_color();
    if code {
        Frame::new()
            .fill(code_bg(ui))
            .stroke(Stroke::new(CODE_PANEL_STROKE, code_border(ui)))
            .corner_radius(CornerRadius::same(CODE_PANEL_RADIUS))
            .inner_margin(Margin::symmetric(
                CODE_PANEL_PADDING_H,
                CODE_PANEL_PADDING_V,
            ))
            .show(ui, |ui| {
                // 面板底色铺满弹窗可用宽（Frame 默认随内容收缩，这里把
                // 内容列定到可用宽，外框连同底色一起撑满）。
                ui.set_width(ui.available_width());
                code_badge(ui, code_lang);
                // 等宽折行：超宽按可用宽换行（经位在正文 ScrollArea 之外，
                // 没有外层滚动区兜底，横滚不进弹窗）。
                let dark = ui.visuals().dark_mode;
                let job = code_job(source, code_lang, dark, ui.available_width());
                let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                ui.add(egui::Label::new(egui::WidgetText::Galley(galley)).selectable(true));
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

/// 代码正文的着色排版：单趟正则切类别，逐段出 [`egui::text::LayoutJob`]
/// （六类色 + 注释斜体，明暗随主题取 demo token）。按可用宽折行（长 token
/// 允许任意字符处断行，保证不超面板宽）、按行断开、行高 1.65；标记间的
/// 间隙与尾部都补平文段，正文整段可被选中复制。
fn code_job(
    source: &str,
    code_lang: Option<&str>,
    dark: bool,
    wrap_width: f32,
) -> egui::text::LayoutJob {
    let font_id = FontId::new(CODE_FONT, fonts::mono_family());
    let plain = egui::TextFormat {
        line_height: Some(CODE_LINE_HEIGHT),
        ..egui::TextFormat::simple(font_id.clone(), strong_code_color(dark))
    };
    let mut job = egui::text::LayoutJob {
        text: source.to_owned(),
        wrap: egui::text::TextWrapping {
            max_width: wrap_width,
            break_anywhere: true,
            ..egui::text::TextWrapping::no_max_width()
        },
        break_on_newline: true,
        ..egui::text::LayoutJob::default()
    };
    let mut cursor = 0;
    for (range, class) in code_hl::tokenize(source, code_lang) {
        if range.start > cursor {
            job.sections.push(egui::text::LayoutSection {
                leading_space: 0.0,
                byte_range: egui::text::ByteIndex(cursor)..egui::text::ByteIndex(range.start),
                format: plain.clone(),
            });
        }
        job.sections.push(egui::text::LayoutSection {
            leading_space: 0.0,
            byte_range: egui::text::ByteIndex(range.start)..egui::text::ByteIndex(range.end),
            format: egui::TextFormat {
                color: class.color(dark),
                italics: class.italic(),
                ..plain.clone()
            },
        });
        cursor = range.end;
    }
    if cursor < source.len() {
        job.sections.push(egui::text::LayoutSection {
            leading_space: 0.0,
            byte_range: egui::text::ByteIndex(cursor)..egui::text::ByteIndex(source.len()),
            format: plain,
        });
    }
    job
}

/// 代码平文字的底色：明暗主题下各取面板底上的主文字色。
fn strong_code_color(dark: bool) -> egui::Color32 {
    if dark {
        egui::Color32::from_rgb(0xE6, 0xE6, 0xE6)
    } else {
        egui::Color32::from_rgb(0x24, 0x29, 0x2E)
    }
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

/// 骨架条（「经显注未至」的正文占位）：三根不同长度的圆角条随时间
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

/// 无边框的动作图标钮（glyph 字形，颜色由 widget 状态笔刷决定）；方块
/// min_size 让齿轮与关闭钮命中区等大。
fn icon_button(glyph: &'static str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(glyph).size(ACTION_ICON_SIZE))
        .frame(false)
        .min_size(vec2(ACTION_BUTTON, ACTION_BUTTON))
}

/// markdown 正文：完整渲染（egui_commonmark 解析绘制），缓存跨帧持有。
fn render_markdown(ui: &mut egui::Ui, state: &RenderState, text: &str) {
    let mut cache = state.cache.borrow_mut();
    CommonMarkViewer::new().show(ui, &mut cache, text);
}

/// 该任务是否按代码排版（经位走代码面板）：流式视图看自动分类的
/// 判定，完成态看产物任务的类型。
fn is_code(kind: Option<TaskKind>) -> bool {
    kind == Some(TaskKind::ExplainCode)
}

/// 产物正文（经注疏排布）：经（原文；提取任务为 note 提取文本；图像
/// 任务为附件图像本身，缺席或解码失败落占位）、注（markdown 注文，词卡
/// 的义/句译的译文/讲解的正文）、疏（examples 疏证逐条，图像解读为
/// interpretation 逐条；提取任务为凡 N 言小记）。
fn outcome_body(
    ui: &mut egui::Ui,
    source: &str,
    outcome: &gloss_core::task::TaskOutcome,
    code_lang: Option<&str>,
    attached: Option<&Arc<[u8]>>,
    state: &RenderState,
    text: &Text,
) {
    match &outcome.structured {
        OutcomeStructured::WordCard { phonetic, examples } => {
            if !source.trim().is_empty() {
                jing_section(ui, text, |ui| word_head(ui, source, phonetic.as_deref()));
                ui.add_space(space::PARAGRAPH);
            }
            zhu_section(ui, text, |ui| {
                apply_zhu_typography(ui);
                render_markdown(ui, state, &outcome.note);
            });
            if !examples.is_empty() {
                ui.add_space(space::PARAGRAPH);
                shu_examples(ui, examples, text);
            }
        }
        OutcomeStructured::Plain { examples } => {
            if !source.trim().is_empty() {
                jing_section(ui, text, |ui| {
                    source_block(ui, source, is_code(Some(outcome.kind)), code_lang)
                });
                ui.add_space(space::PARAGRAPH);
            }
            zhu_section(ui, text, |ui| {
                apply_zhu_typography(ui);
                render_markdown(ui, state, &outcome.note);
            });
            if !examples.is_empty() {
                ui.add_space(space::PARAGRAPH);
                shu_examples(ui, examples, text);
            }
        }
        OutcomeStructured::Extracted => {
            jing_section(ui, text, |ui| plain_body(ui, &outcome.note));
            ui.add_space(space::PARAGRAPH);
            shu_section(ui, text, |ui| {
                ui.label(extract_note(text, &outcome.note));
            });
        }
        OutcomeStructured::ImageCommentary { interpretation } => {
            // 经位：图像本身是经（布局开关＝附件在场，不依赖 classified
            // 到达时序）；缺图回落原文经位（缓存命中等会话边缘），再缺落占位。
            match attached {
                Some(png) => jing_section(ui, text, |ui| attached_image(ui, png, state, text)),
                None if !source.trim().is_empty() => {
                    jing_section(ui, text, |ui| source_block(ui, source, false, code_lang));
                }
                None => jing_section(ui, text, |ui| image_unavailable(ui, text)),
            }
            ui.add_space(space::PARAGRAPH);
            zhu_section(ui, text, |ui| {
                apply_zhu_typography(ui);
                render_markdown(ui, state, &outcome.note);
            });
            if !interpretation.is_empty() {
                ui.add_space(space::PARAGRAPH);
                shu_examples(ui, interpretation, text);
            }
        }
    }
}

/// 词卡经位（说文体例的字头行）：原文（选区词条）+ 音标（读若）随行。
fn word_head(ui: &mut egui::Ui, word: &str, phonetic: Option<&str>) {
    ui.horizontal_wrapped(|ui| {
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
}

/// 疏位疏证（examples 逐条）：例句/展开讲解拆行（demo w-ex 的 `.en` 行
/// + `.zh` 块），楷体弱色。
fn shu_examples(ui: &mut egui::Ui, examples: &[String], text: &Text) {
    shu_section(ui, text, |ui| {
        let weak = ui.visuals().weak_text_color();
        for example in examples {
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
    });
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
mod kittest_tests {

    use std::cell::RefCell;
    use std::rc::Rc;

    use egui_kittest::{Harness, kittest::NodeT, kittest::Queryable};

    use super::*;
    use crate::machine::OverlayView;
    use gloss_core::model::{GlossError, Locale};
    use gloss_core::task::{TaskKind, TaskOutcome};

    fn word_card_view() -> OverlayView {
        OverlayView::Outcome {
            source: "gloss".into(),
            outcome: TaskOutcome {
                kind: TaskKind::TranslateWord,
                note: "**光泽**；注释：表面的一层光亮。".into(),
                code_language: None,
                structured: OutcomeStructured::WordCard {
                    phonetic: Some("/ɡlɒs/".into()),
                    examples: vec!["a gloss of silk 丝绸的光泽".into()],
                },
            },
            code_lang: None,
        }
    }

    fn streaming_view() -> OverlayView {
        OverlayView::Streaming {
            source: "选中的原文".into(),
            raw: r#"{"note":"已流式到达的正文"}"#.into(),
            classified: Some(TaskKind::TranslateWord),
            code_lang: None,
        }
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

    fn image_too_large_view() -> OverlayView {
        OverlayView::Failed {
            cause: FailureCause::Task(GlossError::ImageTooLarge),
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
                note: "很长的正文段落。".repeat(1000) + "尾部标记",
                code_language: None,
                structured: OutcomeStructured::Plain {
                    examples: Vec::new(),
                },
            },
            code_lang: None,
        }
    }

    fn extract_view() -> OverlayView {
        OverlayView::Outcome {
            source: String::new(),
            outcome: TaskOutcome {
                kind: TaskKind::ImageOcr,
                note: "会议纪要\n参会：产品组、评测组".into(),
                code_language: None,
                structured: OutcomeStructured::Extracted,
            },
            code_lang: None,
        }
    }

    fn word_card_view_en() -> OverlayView {
        OverlayView::Outcome {
            source: "gloss".into(),
            outcome: TaskOutcome {
                kind: TaskKind::TranslateWord,
                note: "A surface shine; luster. To add a gloss or commentary.".into(),
                code_language: None,
                structured: OutcomeStructured::WordCard {
                    phonetic: Some("/ɡlɒs/".into()),
                    examples: vec!["The polished wood had a deep gloss.".into()],
                },
            },
            code_lang: None,
        }
    }

    fn streaming_view_en() -> OverlayView {
        OverlayView::Streaming {
            source: "It is not that I am so smart.".into(),
            raw: r#"{"note":"Partial note already streamed.""#.into(),
            classified: Some(TaskKind::TranslateSentence),
            code_lang: None,
        }
    }

    fn code_streaming_view_en() -> OverlayView {
        OverlayView::Streaming {
            source: "fn main() {\n    let gloss = \"光\";\n    println!(\"{gloss}\");\n}".into(),
            raw: r#"{"note":"Partial explanation already streamed.""#.into(),
            classified: Some(TaskKind::ExplainCode),
            code_lang: Some("rust".into()),
        }
    }

    fn code_outcome_view_en() -> OverlayView {
        OverlayView::Outcome {
            source: "fn main() {\n    let gloss = \"光\";\n    println!(\"{gloss}\");\n}".into(),
            outcome: TaskOutcome {
                kind: TaskKind::ExplainCode,
                note: "### What it does\n\nPrints the CJK word for *gloss*.".into(),
                code_language: Some("rust".into()),
                structured: OutcomeStructured::Plain {
                    examples: Vec::new(),
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
                note: "Meeting notes\nAttendees: product, client, eval".into(),
                code_language: None,
                structured: OutcomeStructured::Extracted,
            },
            code_lang: None,
        }
    }

    /// 超长无空格行的产物夹具：markdown 段落与围栏代码块各一段长行——
    /// 「不超出窗口可用宽度」的快照与逐节点断言共用。正文用 ASCII 无空格
    /// 长串（快照 harness 绑内置字形，无 CJK 字面）。
    fn long_line_view() -> OverlayView {
        let long_prose = "glossary".repeat(60);
        let long_code = "let value = compute(someVeryLongIdentifierChain).expect();\n".repeat(6);
        OverlayView::Outcome {
            source: String::new(),
            outcome: TaskOutcome {
                kind: TaskKind::TranslateSentence,
                note: format!("{long_prose}\n\n```rust\n{long_code}```"),
                code_language: None,
                structured: OutcomeStructured::Plain {
                    examples: Vec::new(),
                },
            },
            code_lang: None,
        }
    }

    /// 超长代码原文夹具：经位代码面板的单 token 长行（等宽折行的断言面）。
    fn long_code_source_view() -> OverlayView {
        let long_token = "compute".repeat(40);
        OverlayView::Outcome {
            source: format!("fn main() {{ let x = \"{long_token}\"; }}"),
            outcome: TaskOutcome {
                kind: TaskKind::ExplainCode,
                note: "### Summary\n\nShort body.".into(),
                code_language: Some("rust".into()),
                structured: OutcomeStructured::Plain {
                    examples: Vec::new(),
                },
            },
            code_lang: Some("rust".into()),
        }
    }

    fn image_commentary_view_en() -> OverlayView {
        OverlayView::Outcome {
            source: String::new(),
            outcome: TaskOutcome {
                kind: TaskKind::ImageExplain,
                note: "A gradient panel with the caption **gloss**.".into(),
                code_language: None,
                structured: OutcomeStructured::ImageCommentary {
                    interpretation: vec![
                        "The picture is a generated placeholder, not a screenshot.".into(),
                        "Its palette suggests a dark-mode product shot.".into(),
                    ],
                },
            },
            code_lang: None,
        }
    }

    fn image_streaming_view_en() -> OverlayView {
        OverlayView::Streaming {
            source: String::new(),
            raw: r#"{"note":"A gradient panel with","interpretation":["The picture is a generated placeholder","Its palette suggests a dark-mo"#.into(),
            classified: Some(TaskKind::ImageExplain),
            code_lang: None,
        }
    }

    fn fixture_png() -> Arc<[u8]> {
        let image = ::image::RgbaImage::from_fn(120, 80, |x, y| {
            ::image::Rgba([(x * 2) as u8, (y * 3) as u8, ((x + y) * 2) as u8, 255])
        });
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                ::image::ImageFormat::Png,
            )
            .expect("fixture png encodes");
        bytes.into()
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
            let output = draw(ui, Some(&view), None, &state, text);
            if let Some(action) = output.action {
                *sink.borrow_mut() = Some(action);
            }
        });
        (harness, clicked)
    }

    fn snapshot_harness(view: Option<OverlayView>) -> Harness<'static> {
        snapshot_harness_with(view, None)
    }

    fn snapshot_harness_with(
        view: Option<OverlayView>,
        attached: Option<Arc<[u8]>>,
    ) -> Harness<'static> {
        let state = RenderState::default();
        let text = Text::get(Locale::En);
        let installed = Cell::new(false);
        Harness::builder()
            .with_theme(egui::Theme::Light)
            .build_ui(move |ui| {
                if font_first_frame(&installed, ui.ctx()) {
                    return;
                }
                let _ = draw(ui, view.as_ref(), attached.as_ref(), &state, text);
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
        harness.get_by_label("· a gloss of silk");
    }

    #[test]
    fn word_card_sections_stack_vertically() {
        let (mut harness, _clicked) = harness_for(word_card_view());
        harness.run();
        let head = harness.get_by_label("/ɡlɒs/").rect();
        let example = harness.get_by_label("· a gloss of silk").rect();
        assert!(
            example.top() > head.top(),
            "经→疏必须自上而下排（分区显式垂直布局），不能横向并排: {head:?} {example:?}"
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
        for view in [streaming_view(), word_card_view()] {
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
                sink.set(draw(ui, Some(&view), None, &state, text).sizing);
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
    fn streaming_view_shows_only_the_extracted_note() {
        let (mut harness, _clicked) = harness_for(streaming_view());
        harness.run_steps(3);
        harness.get_by_label_contains("已流式到达的正文");
        harness.get_by_label_contains("选中的原文");
        for artifact in ["title", "\"note\"", "```gloss"] {
            assert!(
                harness
                    .query_all_by_label_contains(artifact)
                    .next()
                    .is_none(),
                "raw JSON or fence artifacts must stay out of the streaming view: {artifact}"
            );
        }
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

    type DragLog = Rc<RefCell<Vec<egui::Vec2>>>;

    fn drag_harness(view: OverlayView) -> (Harness<'static>, DragLog) {
        let drags: DragLog = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&drags);
        let state = RenderState::default();
        let text = Text::get(Locale::Zh);
        let installed = Cell::new(false);
        let harness = Harness::new_ui(move |ui| {
            if font_first_frame(&installed, ui.ctx()) {
                return;
            }
            let output = draw(ui, Some(&view), None, &state, text);
            if let Some(drag) = output.drag {
                sink.borrow_mut().push(drag);
            }
        });
        (harness, drags)
    }

    #[test]
    fn header_drag_strip_is_exposed_to_accesskit() {
        let (mut harness, _drags) = drag_harness(word_card_view());
        harness.run();
        let strip = harness.get_by_label("拖动浮层").rect();
        assert!(
            strip.height() >= HEADER_ICON,
            "热区与页头行同高，覆盖图标与动作区之间的整段行身: {strip:?}"
        );
        let gear = harness.get_by_label("设置").rect();
        let close = harness.get_by_label("关闭浮层").rect();
        assert!(
            strip.right() <= gear.left(),
            "热区止于最左动作钮左缘（收 DRAG_STRIP_INSET），与齿轮零重叠: {strip:?} vs {gear:?}"
        );
        assert!(
            strip.left() <= close.left(),
            "热区左缘在关闭钮左侧（水平区间不与任一动作钮相交）: {strip:?} vs {close:?}"
        );
    }

    #[test]
    fn header_drag_reports_cumulative_offset_and_ends_on_release() {
        let (mut harness, drags) = drag_harness(word_card_view());
        harness.run();
        let center = harness.get_by_label("拖动浮层").rect().center();

        harness.drag_at(center);
        harness.run();
        assert_eq!(
            drags.borrow().last(),
            Some(&egui::Vec2::ZERO),
            "按下帧确立握点，位移从零起算"
        );

        harness.hover_at(center + egui::vec2(20.0, 10.0));
        harness.run();
        harness.hover_at(center + egui::vec2(30.0, 15.0));
        harness.run();
        assert!(
            drags.borrow().contains(&egui::vec2(20.0, 10.0)),
            "位移按按压点累计: {:?}",
            drags.borrow()
        );
        assert_eq!(
            drags.borrow().last(),
            Some(&egui::vec2(30.0, 15.0)),
            "offset 累计自按压点（而非逐帧增量，否则第二段是 (10, 5)）"
        );

        let length_before_release = drags.borrow().len();
        harness.drop_at(center + egui::vec2(30.0, 15.0));
        harness.run();
        harness.hover_at(center + egui::vec2(40.0, 20.0));
        harness.run();
        assert_eq!(
            drags.borrow().len(),
            length_before_release,
            "松开后不再上交拖动状态（释放帧与后续移动帧都是 None）"
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
            let _ = draw(ui, None, None, &state, text);
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
    fn long_lines_never_exceed_the_window_width() {
        for view in [long_line_view(), long_code_source_view()] {
            let state = RenderState::default();
            let text = Text::get(Locale::En);
            let installed = Cell::new(false);
            let mut harness =
                Harness::builder()
                    .with_theme(egui::Theme::Light)
                    .build_ui(move |ui| {
                        if font_first_frame(&installed, ui.ctx()) {
                            return;
                        }
                        let _ = draw(ui, Some(&view), None, &state, text);
                    });
            harness.set_size(egui::vec2(WIDTH, 800.0));
            harness.run();
            let overflowing: Vec<_> = harness
                .query_all_by(|_| true)
                .filter(|node| node.accesskit_node().bounding_box().is_some())
                .map(|node| node.rect())
                .filter(|rect| rect.right() > WIDTH + 0.5)
                .collect();
            assert!(
                overflowing.is_empty(),
                "横滚不进弹窗：全部内容节点不得超出窗口可用宽 {WIDTH}: {overflowing:?}"
            );
        }
    }

    #[test]
    fn image_card_exposes_the_commentary_to_accesskit() {
        let png = fixture_png();
        let state = RenderState::default();
        let text = Text::get(Locale::En);
        let installed = Cell::new(false);
        let view = image_commentary_view_en();
        let mut harness = Harness::new_ui(move |ui| {
            if font_first_frame(&installed, ui.ctx()) {
                return;
            }
            let _ = draw(ui, Some(&view), Some(&png), &state, text);
        });
        harness.run();
        harness.get_by_label_contains("A gradient panel with");
        harness.get_by_label_contains("The picture is a generated placeholder");
        harness.get_by_label_contains("Its palette suggests a dark-mode product shot.");
        let note = harness
            .get_by_label_contains("A gradient panel with")
            .rect();
        let item = harness.get_by_label_contains("Its palette suggests").rect();
        assert!(
            item.top() > note.top(),
            "注→疏必须自上而下排（经=图在上，解读条目随后）: {note:?} {item:?}"
        );
    }

    #[test]
    fn image_streaming_card_shows_the_interpretation_items_as_they_arrive() {
        let png = fixture_png();
        let state = RenderState::default();
        let text = Text::get(Locale::En);
        let installed = Cell::new(false);
        let view = image_streaming_view_en();
        let mut harness = Harness::new_ui(move |ui| {
            if font_first_frame(&installed, ui.ctx()) {
                return;
            }
            let _ = draw(ui, Some(&view), Some(&png), &state, text);
        });
        harness.run_steps(3);
        harness.get_by_label_contains("A gradient panel with");
        harness.get_by_label_contains("The picture is a generated placeholder");
        harness.get_by_label_contains("Its palette suggests a dark-mo");
        assert!(
            harness.query_by_label_contains("interpretation").is_none(),
            "raw JSON scaffolding must not reach the card"
        );
    }

    #[test]
    fn image_streaming_card_without_a_note_keeps_the_skeleton() {
        let png = fixture_png();
        let state = RenderState::default();
        let text = Text::get(Locale::Zh);
        let installed = Cell::new(false);
        let view = OverlayView::Streaming {
            source: String::new(),
            raw: String::new(),
            classified: Some(TaskKind::ImageExplain),
            code_lang: None,
        };
        let mut harness = Harness::new_ui(move |ui| {
            if font_first_frame(&installed, ui.ctx()) {
                return;
            }
            let _ = draw(ui, Some(&view), Some(&png), &state, text);
        });
        harness.run_steps(3);
        harness.get_by_label_contains("正在注解");
    }

    #[test]
    fn image_card_degrades_to_a_placeholder_when_the_bytes_do_not_decode() {
        let broken: Arc<[u8]> = Arc::from(&b"not a png"[..]);
        let state = RenderState::default();
        let text = Text::get(Locale::En);
        let installed = Cell::new(false);
        let view = image_commentary_view_en();
        let mut harness = Harness::new_ui(move |ui| {
            if font_first_frame(&installed, ui.ctx()) {
                return;
            }
            let _ = draw(ui, Some(&view), Some(&broken), &state, text);
        });
        harness.run();
        harness.get_by_label_contains("The image could not be displayed.");
        harness.get_by_label_contains("A gradient panel with");
    }

    #[test]
    fn attached_image_texture_is_reused_across_frames() {
        let png = fixture_png();
        let state = RenderState::default();
        let text = Text::get(Locale::En);
        let installed = Cell::new(false);
        let view = image_commentary_view_en();
        let mut harness = Harness::new_ui(move |ui| {
            if font_first_frame(&installed, ui.ctx()) {
                return;
            }
            let _ = draw(ui, Some(&view), Some(&png), &state, text);
        });
        harness.run();
        harness.run();
        let settled = harness.ctx.tex_manager().read().num_allocated();
        for _ in 0..5 {
            harness.run();
        }
        assert_eq!(
            harness.ctx.tex_manager().read().num_allocated(),
            settled,
            "同图逐帧复用同一纹理：帧数增长不得新装纹理（逐帧解码即失败）"
        );
    }

    #[test]
    fn attached_image_slot_is_dropped_once_an_imageless_frame_renders() {
        let png = fixture_png();
        let state = RenderState::default();
        let text = Text::get(Locale::En);
        let installed = Cell::new(false);
        let view = image_commentary_view_en();
        let mode = Rc::new(Cell::new(0u8));
        let frame_mode = Rc::clone(&mode);
        let mut harness = Harness::new_ui(move |ui| {
            if font_first_frame(&installed, ui.ctx()) {
                return;
            }
            let attached = if frame_mode.get() == 0 {
                Some(&png)
            } else {
                None
            };
            let _ = draw(ui, Some(&view), attached, &state, text);
        });

        harness.run();
        harness.run();
        let with_image = harness.ctx.tex_manager().read().num_allocated();

        mode.set(1);
        harness.run();
        let after_drop = harness.ctx.tex_manager().read().num_allocated();
        assert!(
            after_drop < with_image,
            "会话离开图像卡即清槽：源字节与纹理同帧释放（{after_drop} < {with_image}）"
        );

        mode.set(0);
        harness.run();
        assert_eq!(
            harness.ctx.tex_manager().read().num_allocated(),
            with_image,
            "同图重新上卡走重新解码：驻留恢复但不复用已释放的纹理"
        );
    }

    #[test]
    fn snapshots_match_baseline() {
        let mut results = egui_kittest::SnapshotResults::new();

        let mut harness = snapshot_harness(Some(word_card_view_en()));
        harness.run();
        harness.snapshot("popup_word_card");
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

        let mut harness =
            snapshot_harness_with(Some(image_commentary_view_en()), Some(fixture_png()));
        harness.run();
        harness.snapshot("popup_image_outcome");
        results.extend_harness(&mut harness);

        let mut harness =
            snapshot_harness_with(Some(image_streaming_view_en()), Some(fixture_png()));
        harness.run_steps(3);
        harness.snapshot("popup_image_streaming");
        results.extend_harness(&mut harness);

        let mut harness = snapshot_harness(Some(image_too_large_view()));
        harness.run();
        harness.snapshot("popup_image_failed");
        results.extend_harness(&mut harness);

        let mut harness = snapshot_harness(None);
        harness.run();
        harness.snapshot("popup_selfcheck");
        results.extend_harness(&mut harness);

        let mut harness = snapshot_harness(Some(long_line_view()));
        harness.run();
        harness.snapshot("popup_long_line");
        results.extend_harness(&mut harness);

        results.unwrap();
    }
}
