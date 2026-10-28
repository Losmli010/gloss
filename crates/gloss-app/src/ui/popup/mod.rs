//! 浮层内容：按视图分发的骨架、经注疏结果卡与流式、失败视图。
//!
//! 内容按“经 · 注 · 疏”三层组织，视觉以设计定稿的浮层稿为准：
//! - “经”＝选区原文、词条或提取文本，用宋体；代码任务为单层代码面板
//!   ——专属底色、应用内置等宽字体、按可用宽折行、左上角语言标签行；
//! - “注”＝译文、释义或概要，用楷体，系统缺楷体时随设计稿回退链落宋体；
//!   配朱丝栏左线与朱印；
//! - “疏”＝译注、例句或小记，正文为标记文档（Markdown）的楷体色排版，
//!   配上缘虚线与疏印。
//!
//! 三分区的正文列都显式声明垂直布局：界面库（egui）的分区容器会继承父级的
//! 水平布局（新建子区域默认继承父方向），横排父级里多条目正文会从左往
//! 右流（释义并排、溢出右缘）。分区的间距是弹窗私有量（随
//! 经注疏排布模块存放），字号规格与宋楷字体助手在文案整形模块（都是设计稿基准
//! 的应用定稿值），不进样式阶梯。
//!
//! 决策纯函数不进绘制模板，按职责拆在子模块：流式注渐进
//! 提取、图标矢量栅格化、文案整形；绘制模板
//! 按职责拆在页头、页脚、印章等卡片固定件、视图
//! 分发与卡骨架、经注疏正文排布，依赖方向恒为视图 →
//! 排布 → 固定件 → 决策；本模块留面板编排、跨帧状态与各文件共享的
//! 常量。
//!
//! 词卡精排（词条与音标落经位，释义逐行落注位，例句落疏位），其余任务正文
//! 走标记文档渲染（egui_commonmark），注区统一改写文本样式为楷体字号。
//! 完成卡页脚左端有复制钮——复制完整注疏（注 + 疏，见注疏组装纯函数），
//! 这是对早期“划选即复制、不设独立复制按钮”原则的显式偏离：选中复制
//! 仍然全域可用，复制钮只是给了完成卡一个一步到位的出口。正文完整渲染不截断，
//! 宽度固定（浮层宽度常量，不随内容变化），高度自适应内容（上限按屏幕），
//! 超出部分滚动兜底。
//! 流式视图对累积的原始流做“转义感知”的注渐进提取——现行输出
//! 契约是纯对象数据（JSON），见核心层契约模块：注键未到齐时正文区
//! 落骨架、页脚保留“正在注疏”；疏证例句字段随流下钻渐进出。
//! 降级产物（契约校验失败）不出注疏——降级卡渲染
//! “输出异常”与重试按钮，不渲染注内容（原文垃圾不展示不留存）。
//! 注文一律换行排版：代码面板等宽折行、标记文档由通用标记渲染库（egui_commonmark）按
//! 可用宽折行，横滚不进弹窗。
//!
//! 页头回归品牌：只有应用图标与设置、关闭两枚动作图标，任务与状态由内容层自明，
//! 页头不带任何标签药丸；行下发丝线与页脚上缘线呼应成卡片的上下界。
//! 页头行同时是拖动热区：按住拖动即移动浮层（壳侧逐帧平移窗口），热区
//! 与动作钮零重叠，按钮点击不受影响。
//! 卡片下缘、右缘与右下角是缩放热区（斜纹握把示意）：按住拖动即向壳侧
//! 逐帧上交期望宽高，壳侧钳制落窗；拖过一次即本段会话接管浮层宽高——
//! 内容高度自适应退位，显隐重置后回到默认尺寸。
//! 页脚常驻
//! 一条窄带（带高取页脚高度常量）：推理中左端是“正在注疏”与呼吸点（生成指示唯一落点），
//! 待确认卡左端是确认主按钮与取消次按钮（发送前把关的唯一出口），
//! 完成卡左端是复制钮（点击后短暂显示已复制反馈），右端
//! 恒为本品牌（Gloss）水印——品牌名不翻译，与窗口标题同一原则；滚动区按页脚带宽
//! 预留视口，页脚不被内容挤出窗外。“经显注未至”时正文区是纯骨架
//! （脉动条），无占位文字。动作点击在绘制帧折成动作上交壳执行；
//! 拖动热区的指针位移经拖动出口上交壳换算。
//!
//! 敏感信息防护不在这里：两条闸门都不出浮层（见应用层状态机），
//! 因此也没有“疑似敏感”这张卡。

mod card;
mod content;
mod footer;
mod icon;
mod image;
mod note;
mod outcome;
mod views;

pub(crate) use card::clear_cached_text_galleys;
pub(crate) use outcome::annotation_text;

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use egui::{Frame, Stroke};
use egui_commonmark::CommonMarkCache;
use icon::app_icon_image;
use image::decode_attached;

use super::style::{geometry, motion, radius, space, stroke};
use crate::machine::{ErrorAction, OverlayView};
use crate::ui::i18n::Text;

struct RenderCtx<'a> {
    state: &'a RenderState,
    text: &'static Text,
    attached: Option<&'a Arc<[u8]>>,
}

pub struct RenderState {
    pub cache: RefCell<CommonMarkCache>,
    drag_press_phys: Cell<Option<egui::Pos2>>,
    resize_press: Cell<Option<(egui::Pos2, egui::Vec2)>>,
    user_size: Cell<Option<egui::Vec2>>,
    icon: RefCell<Option<(u32, Option<egui::TextureHandle>)>>,
    attached: RefCell<Option<AttachedTexture>>,
    copied_at: Cell<Option<f64>>,
    copied_source: Cell<Option<String>>,
}

struct AttachedTexture {
    source: Arc<[u8]>,
    texture: Option<egui::TextureHandle>,
}

impl Default for RenderState {
    fn default() -> Self {
        Self {
            cache: RefCell::new(CommonMarkCache::default()),
            drag_press_phys: Cell::new(None),
            resize_press: Cell::new(None),
            user_size: Cell::new(None),
            icon: RefCell::new(None),
            attached: RefCell::new(None),
            copied_at: Cell::new(None),
            copied_source: Cell::new(None),
        }
    }
}

impl RenderState {
    pub(crate) fn reset_drag_state(&self) {
        self.drag_press_phys.set(None);
        self.resize_press.set(None);
        self.user_size.set(None);
    }

    fn icon_texture(&self, ctx: &egui::Context) -> Option<egui::TextureHandle> {
        let side_px = (geometry::overlay::HEADER_ICON * ctx.pixels_per_point())
            .round()
            .max(1.0) as u32;
        let mut slot = self.icon.borrow_mut();
        if slot
            .as_ref()
            .is_none_or(|(rendered, _)| *rendered != side_px)
        {
            let texture = app_icon_image(side_px).map(|image| {
                ctx.load_texture("gloss_app_icon", image, egui::TextureOptions::LINEAR)
            });
            *slot = Some((side_px, texture));
        }
        slot.as_ref().and_then(|(_, texture)| texture.clone())
    }

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

    fn drop_attached(&self) {
        *self.attached.borrow_mut() = None;
    }
}

pub struct PopupOutput {
    pub action: Option<OverlayAction>,
    pub sizing: OverlaySizing,
    pub drag: Option<egui::Vec2>,
    pub resize: Option<egui::Vec2>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayAction {
    Confirm,
    CopyAnnotation,
    Retry,
    OpenSettings,
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

#[derive(Clone, Copy, Debug)]
pub struct OverlaySizing {
    pub height: f32,
}

fn appear_t0_id() -> egui::Id {
    egui::Id::new("overlay_appear_t0")
}

fn appear_progress(elapsed_secs: f64) -> f32 {
    (elapsed_secs / f64::from(motion::APPEAR)).clamp(0.0, 1.0) as f32
}

fn appear_fade_in(ui: &mut egui::Ui) {
    let now = ui.input(|i| i.time);
    let started_at = ui
        .ctx()
        .memory_mut(|mem| *mem.data.get_temp_mut_or_insert_with(appear_t0_id(), || now));
    let progress = appear_progress(now - started_at);
    if progress < 1.0 {
        ui.ctx().request_repaint_after(motion::FRAME_BEAT);
    }
    ui.set_opacity(progress);
}

pub(crate) fn reset_appear_animation(ctx: &egui::Context) {
    ctx.memory_mut(|mem| mem.data.remove_temp::<f64>(appear_t0_id()));
}

pub fn draw(
    ui: &mut egui::Ui,
    view: Option<&OverlayView>,
    attached: Option<&Arc<[u8]>>,
    state: &RenderState,
    text: &'static Text,
) -> PopupOutput {
    ui.style_mut().interaction.selectable_labels = true;
    appear_fade_in(ui);

    if attached.is_none() {
        state.drop_attached();
    }
    let prev_copied_source = state.copied_source.take();
    match view {
        Some(OverlayView::Outcome { source, .. }) => {
            if prev_copied_source.as_deref() == Some(source.as_str()) {
                state.copied_source.set(prev_copied_source);
            } else {
                state.copied_at.set(None);
                state.copied_source.set(Some(source.clone()));
            }
        }
        _ => state.copied_at.set(None),
    }

    let fill = ui.visuals().window_fill;
    let window_stroke = ui.visuals().window_stroke;
    let mut output = PopupOutput {
        action: None,
        sizing: OverlaySizing { height: 0.0 },
        drag: None,
        resize: None,
    };
    let mut content_h = 0.0;
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(stroke::HAIRLINE, window_stroke.color))
        .corner_radius(radius::CARD)
        .inner_margin(space::CARD_PADDING)
        .show(ui, |ui| {
            let ctx = RenderCtx {
                state,
                text,
                attached,
            };
            let (action, drag) = views::render_content(ui, &ctx, view, &mut content_h);
            output.action = action;
            output.drag = drag;
            ui.set_min_size(ui.available_size());
        });

    output.sizing = OverlaySizing {
        height: (content_h + 2.0 * space::CARD_PADDING + stroke::HAIRLINE).round(),
    };
    output.resize = resize_handle(ui, state, text);
    output
}

type ResizeZone = (&'static str, egui::Rect, egui::CursorIcon);

fn resize_zones(card: egui::Rect) -> [ResizeZone; 3] {
    let hot = geometry::resize::HOTSPOT;
    let corner = geometry::resize::CORNER;
    [
        (
            "overlay_resize_south",
            egui::Rect::from_min_max(
                egui::pos2(card.left() + corner, card.bottom() - hot),
                egui::pos2(card.right(), card.bottom()),
            ),
            egui::CursorIcon::ResizeSouth,
        ),
        (
            "overlay_resize_east",
            egui::Rect::from_min_max(
                egui::pos2(card.right() - hot, card.top() + corner),
                egui::pos2(card.right(), card.bottom() - corner),
            ),
            egui::CursorIcon::ResizeEast,
        ),
        (
            "overlay_resize_corner",
            egui::Rect::from_min_max(
                egui::pos2(card.right() - corner, card.bottom() - corner),
                egui::pos2(card.right(), card.bottom()),
            ),
            egui::CursorIcon::ResizeSouthEast,
        ),
    ]
}

fn resize_handle(
    ui: &mut egui::Ui,
    state: &RenderState,
    text: &'static Text,
) -> Option<egui::Vec2> {
    let card = ui.min_rect();
    paint_resize_grip(ui, card);
    let (pointer, pixels_per_point, window) = ui.input(|i| {
        (
            i.pointer.interact_pos(),
            i.pixels_per_point,
            i.viewport_rect().size(),
        )
    });
    let mut dragging = false;
    for (id, rect, icon) in resize_zones(card) {
        let response = ui.interact(rect, egui::Id::new(id), egui::Sense::drag());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Other,
                true,
                text.gloss_popup_resize_label.as_str(),
            )
        });
        let response = response.on_hover_cursor(icon);
        if response.drag_started() {
            state
                .resize_press
                .set(pointer.map(|pos| (pos * pixels_per_point, window)));
            state.user_size.set(Some(window));
        }
        if !response.dragged() {
            continue;
        }
        dragging = true;
        if let (Some(pos), Some((press, base))) = (pointer, state.resize_press.get()) {
            let pointer_phys = pos * pixels_per_point;
            state
                .user_size
                .set(Some(base + (pointer_phys - press) / pixels_per_point));
        }
    }
    if !dragging {
        state.resize_press.set(None);
    }
    state.user_size.get()
}

fn paint_resize_grip(ui: &egui::Ui, card: egui::Rect) {
    let stroke = Stroke::new(stroke::LINE, ui.visuals().weak_text_color());
    let corner = egui::pos2(card.right(), card.bottom());
    for index in 0..geometry::resize::GRIP_COUNT {
        let offset = geometry::resize::GRIP_INSET + index as f32 * geometry::resize::GRIP_GAP;
        ui.painter().line_segment(
            [
                corner - egui::vec2(offset + geometry::resize::GRIP_LENGTH, offset),
                corner - egui::vec2(offset, offset + geometry::resize::GRIP_LENGTH),
            ],
            stroke,
        );
    }
}

#[cfg(test)]
mod kittest_tests {

    use std::cell::RefCell;
    use std::rc::Rc;

    const WIDTH: f32 = crate::ui::style::geometry::window::OVERLAY_WIDTH as f32;

    use egui::vec2;
    use egui_kittest::{Harness, kittest::NodeT, kittest::Queryable};

    use super::*;

    use crate::machine::{FailureCause, OverlayView};
    use gloss_core::model::{GlossError, Locale};
    use gloss_core::task::{OutcomeStructured, TaskInput, TaskKind, TaskOptions, TaskOutcome};

    fn word_card_view() -> OverlayView {
        OverlayView::Outcome {
            source: "gloss".into(),
            outcome: TaskOutcome {
                kind: TaskKind::TranslateWord,
                note: "**光泽**；注释：表面的一层光亮。".into(),
                structured: OutcomeStructured::WordCard {
                    phonetic: Some("/ɡlɒs/".into()),
                    examples: vec!["a gloss of silk 丝绸的光泽".into()],
                },
                degraded: false,
            },
        }
    }

    fn streaming_view() -> OverlayView {
        OverlayView::Streaming {
            source: "选中的原文".into(),
            raw: r#"{"note":"已流式到达的正文"}"#.into(),
            classified: Some(TaskKind::TranslateWord),
            repairing: false,
        }
    }

    fn pending_text_view() -> OverlayView {
        OverlayView::Pending {
            input: TaskInput::Text {
                text: "选中的原文".into(),
            },
            options: TaskOptions::default(),
        }
    }

    fn pending_text_view_en() -> OverlayView {
        OverlayView::Pending {
            input: TaskInput::Text {
                text: "The selected source text.".into(),
            },
            options: TaskOptions::default(),
        }
    }

    fn pending_image_view() -> OverlayView {
        OverlayView::Pending {
            input: TaskInput::Image {
                png: Arc::from(&b"png"[..]),
            },
            options: TaskOptions::default(),
        }
    }

    fn repairing_streaming_view() -> OverlayView {
        OverlayView::Streaming {
            source: "选中的原文".into(),
            raw: String::new(),
            classified: Some(TaskKind::TranslateWord),
            repairing: true,
        }
    }

    fn long_selection_streaming_view() -> OverlayView {
        OverlayView::Streaming {
            source: "The selected passage is long enough to overflow the card body. ".repeat(80),
            raw: r#"{"note":"已流式到达的正文"}"#.into(),
            classified: Some(TaskKind::TranslateSentence),
            repairing: false,
        }
    }

    fn long_code_streaming_view() -> OverlayView {
        OverlayView::Streaming {
            source: "let value = compute(someVeryLongIdentifierChain).expect();\n".repeat(60),
            raw: r#"{"note":"已流式到达的正文"}"#.into(),
            classified: Some(TaskKind::ExplainCode),
            repairing: false,
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

    fn degraded_outcome_view_en() -> OverlayView {
        OverlayView::Outcome {
            source: "fn main() {}".into(),
            outcome: TaskOutcome {
                kind: TaskKind::ExplainCode,
                note: "untrustworthy raw reply".into(),
                structured: OutcomeStructured::Code {
                    examples: Vec::new(),
                    code_language: None,
                },
                degraded: true,
            },
        }
    }

    fn general_outcome_view_en() -> OverlayView {
        OverlayView::Outcome {
            source: "any content at all".into(),
            outcome: TaskOutcome {
                kind: TaskKind::General,
                note: "A direct answer to arbitrary content.".into(),
                structured: OutcomeStructured::Plain {
                    examples: vec![
                        "first expansion item".into(),
                        "second expansion item".into(),
                    ],
                },
                degraded: false,
            },
        }
    }

    fn long_body_view() -> OverlayView {
        OverlayView::Outcome {
            source: String::new(),
            outcome: TaskOutcome {
                kind: TaskKind::TranslateSentence,
                note: "很长的正文段落。".repeat(1000) + "尾部标记",
                structured: OutcomeStructured::Plain {
                    examples: Vec::new(),
                },
                degraded: false,
            },
        }
    }

    fn word_card_view_en() -> OverlayView {
        OverlayView::Outcome {
            source: "gloss".into(),
            outcome: TaskOutcome {
                kind: TaskKind::TranslateWord,
                note: "A surface shine; luster. To add a gloss or commentary.".into(),
                structured: OutcomeStructured::WordCard {
                    phonetic: Some("/ɡlɒs/".into()),
                    examples: vec!["The polished wood had a deep gloss.".into()],
                },
                degraded: false,
            },
        }
    }

    fn streaming_view_en() -> OverlayView {
        OverlayView::Streaming {
            source: "It is not that I am so smart.".into(),
            raw: r#"{"note":"Partial note already streamed.""#.into(),
            classified: Some(TaskKind::TranslateSentence),
            repairing: false,
        }
    }

    fn code_streaming_view_en() -> OverlayView {
        OverlayView::Streaming {
            source: "fn main() {\n    let gloss = \"光\";\n    println!(\"{gloss}\");\n}".into(),
            raw: r#"{"note":"Partial explanation already streamed.""#.into(),
            classified: Some(TaskKind::ExplainCode),
            repairing: false,
        }
    }

    fn code_outcome_view_en() -> OverlayView {
        OverlayView::Outcome {
            source: "fn main() {\n    let gloss = \"光\";\n    println!(\"{gloss}\");\n}".into(),
            outcome: TaskOutcome {
                kind: TaskKind::ExplainCode,
                note: "### What it does\n\nPrints the CJK word for *gloss*.".into(),
                structured: OutcomeStructured::Code {
                    examples: Vec::new(),
                    code_language: Some("rust".into()),
                },
                degraded: false,
            },
        }
    }

    fn long_line_view() -> OverlayView {
        let long_prose = "glossary".repeat(60);
        let long_code = "let value = compute(someVeryLongIdentifierChain).expect();\n".repeat(6);
        OverlayView::Outcome {
            source: String::new(),
            outcome: TaskOutcome {
                kind: TaskKind::TranslateSentence,
                note: format!("{long_prose}\n\n```rust\n{long_code}```"),
                structured: OutcomeStructured::Plain {
                    examples: Vec::new(),
                },
                degraded: false,
            },
        }
    }

    fn long_code_source_view() -> OverlayView {
        let long_token = "compute".repeat(40);
        OverlayView::Outcome {
            source: format!("fn main() {{ let x = \"{long_token}\"; }}"),
            outcome: TaskOutcome {
                kind: TaskKind::ExplainCode,
                note: "### Summary\n\nShort body.".into(),
                structured: OutcomeStructured::Code {
                    examples: Vec::new(),
                    code_language: Some("rust".into()),
                },
                degraded: false,
            },
        }
    }

    fn image_commentary_view_en() -> OverlayView {
        OverlayView::Outcome {
            source: String::new(),
            outcome: TaskOutcome {
                kind: TaskKind::ImageExplain,
                note: "A gradient panel with the caption **gloss**.".into(),
                structured: OutcomeStructured::Plain {
                    examples: vec![
                        "The picture is a generated placeholder, not a screenshot.".into(),
                        "Its palette suggests a dark-mode product shot.".into(),
                    ],
                },
                degraded: false,
            },
        }
    }

    fn image_streaming_view_en() -> OverlayView {
        OverlayView::Streaming {
            source: String::new(),
            raw: r#"{"note":"A gradient panel with","structured":{"examples":["The picture is a generated placeholder","Its palette suggests a dark-mo"#.into(),
            classified: Some(TaskKind::ImageExplain),
            repairing: false,
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

    #[test]
    fn pending_card_exposes_the_source_and_its_two_exits() {
        let (mut harness, clicked) = harness_for(pending_text_view());
        harness.run();
        harness.get_by_label_contains("选中的原文");
        harness.get_by_label("经");
        harness.get_by_label("注疏");
        harness.get_by_label("取消");
        harness.get_by_label("注疏").click();
        harness.run();
        assert_eq!(
            *clicked.borrow(),
            Some(OverlayAction::Confirm),
            "the primary button confirms the dispatch"
        );

        let (mut harness, clicked) = harness_for(pending_text_view());
        harness.run();
        harness.get_by_label("取消").click();
        harness.run();
        assert_eq!(
            *clicked.borrow(),
            Some(OverlayAction::Dismiss),
            "取消 collapses the pending card via the shared dismiss path"
        );
    }

    #[test]
    fn the_done_card_copy_button_submits_copy_annotation_then_flips_to_copied() {
        let (mut harness, clicked) = harness_for(word_card_view());
        harness.run();
        harness.get_by_label("复制注疏").click();
        harness.run_steps(2);
        assert_eq!(
            *clicked.borrow(),
            Some(OverlayAction::CopyAnnotation),
            "the footer copy button surfaces the copy action"
        );
        harness.get_by_label_contains("已复制");
    }

    #[test]
    fn a_card_with_a_different_source_clears_the_copied_feedback() {
        let view = Rc::new(RefCell::new(word_card_view()));
        let (mut harness, _clicked) = harness_for_switchable(Rc::clone(&view));
        harness.run();
        harness.get_by_label("复制注疏").click();
        harness.run_steps(2);
        harness.get_by_label_contains("已复制");

        *view.borrow_mut() = OverlayView::Outcome {
            source: "silk".into(),
            outcome: TaskOutcome {
                kind: TaskKind::TranslateWord,
                note: "A surface shine; luster. To add a gloss or commentary.".into(),
                structured: OutcomeStructured::WordCard {
                    phonetic: Some("/ɡlɒs/".into()),
                    examples: vec!["The polished wood had a deep gloss.".into()],
                },
                degraded: false,
            },
        };
        harness.run();
        assert!(
            harness
                .query_all_by_label_contains("已复制")
                .next()
                .is_none(),
            "the copied feedback must not leak onto a card with a different source"
        );
        harness.get_by_label("复制注疏");
    }

    #[test]
    fn a_rerender_of_the_same_source_keeps_the_copied_feedback() {
        let view = Rc::new(RefCell::new(word_card_view()));
        let (mut harness, _clicked) = harness_for_switchable(Rc::clone(&view));
        harness.run();
        harness.get_by_label("复制注疏").click();
        harness.run_steps(2);
        harness.get_by_label_contains("已复制");

        *view.borrow_mut() = word_card_view();
        harness.run_steps(2);
        harness.get_by_label_contains("已复制");
    }

    #[test]
    fn degraded_and_streaming_cards_take_no_copy_button() {
        for view in [degraded_outcome_view_en(), streaming_view()] {
            let (mut harness, _clicked) = harness_for(view);
            harness.run_steps(3);
            assert!(
                harness.query_all_by_label("复制注疏").next().is_none(),
                "only the settled outcome card carries the copy button"
            );
        }
    }

    type Clicked = Rc<RefCell<Option<OverlayAction>>>;

    fn font_first_frame(installed: &Cell<bool>, ctx: &egui::Context) -> bool {
        if installed.replace(true) {
            return false;
        }
        crate::ui::context::install_kittest_fonts(ctx);
        crate::ui::context::apply_visuals(ctx);
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

    fn harness_for_switchable(view: Rc<RefCell<OverlayView>>) -> (Harness<'static>, Clicked) {
        let clicked: Clicked = Rc::new(RefCell::new(None));
        let sink = Rc::clone(&clicked);
        let state = RenderState::default();
        let text = Text::get(Locale::Zh);
        let installed = Cell::new(false);
        let harness = Harness::new_ui(move |ui| {
            if font_first_frame(&installed, ui.ctx()) {
                return;
            }
            let output = draw(ui, Some(&view.borrow()), None, &state, text);
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
                FailureCause::Task(GlossError::JsonModeUnsupported),
                errors.gloss_errors_json_mode_unsupported.as_str(),
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
        harness.get_by_label_contains("a gloss of silk");
    }

    #[test]
    fn word_card_sections_stack_vertically() {
        let (mut harness, _clicked) = harness_for(word_card_view());
        harness.run();
        let head = harness.get_by_label("/ɡlɒs/").rect();
        let example = harness.get_by_label_contains("a gloss of silk").rect();
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
    fn arrow_keys_scroll_the_overflowing_body() {
        let (mut harness, _clicked) = harness_for(long_body_view());
        harness.run();
        let before = harness.get_by_label_contains("很长的正文段落").rect();
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        let after = harness.get_by_label_contains("很长的正文段落").rect();
        assert!(
            after.top() < before.top(),
            "↓ 必须把内容往上推（视口下移）: {before:?} -> {after:?}"
        );
        harness.key_press(egui::Key::ArrowUp);
        harness.run();
        let restored = harness.get_by_label_contains("很长的正文段落").rect();
        assert!(
            (restored.top() - before.top()).abs() < 1.0,
            "↑ 必须把视口推回原位: {before:?} -> {restored:?}"
        );
    }

    #[test]
    fn streaming_view_shows_the_annotating_footer() {
        let (mut harness, _clicked) = harness_for(streaming_view());
        harness.run_steps(3);
        harness.get_by_label_contains("正在注疏");
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
    fn a_repairing_streaming_view_switches_the_footer_wording() {
        let (mut harness, _clicked) = harness_for(repairing_streaming_view());
        harness.run_steps(3);
        harness.get_by_label_contains("重新注疏");
        assert!(
            harness
                .query_all_by_label_contains("正在注疏")
                .next()
                .is_none(),
            "the repair marker replaces the annotating label, no banner, no counter"
        );
        let (mut harness, _clicked) = harness_for_locale(repairing_streaming_view(), Locale::En);
        harness.run_steps(3);
        harness.get_by_label_contains("Re-annotating");
    }

    #[test]
    fn watermark_sits_inside_the_computed_window_height() {
        for view in [streaming_view(), word_card_view()] {
            let state = RenderState::default();
            let text = Text::get(Locale::Zh);
            let sizing: Rc<Cell<OverlaySizing>> = Rc::new(Cell::new(OverlaySizing { height: 0.0 }));
            let sink = Rc::clone(&sizing);
            let installed = Cell::new(false);
            let mut harness = Harness::new_ui(move |ui| {
                if font_first_frame(&installed, ui.ctx()) {
                    return;
                }
                sink.set(draw(ui, Some(&view), None, &state, text).sizing);
            });
            harness.run_steps(2);
            harness.set_size(vec2(WIDTH, sizing.get().height));
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
    fn a_long_selection_keeps_the_footer_inside_the_window() {
        let capped_height = 420.0;
        for view in [long_selection_streaming_view(), long_code_streaming_view()] {
            let state = RenderState::default();
            let text = Text::get(Locale::Zh);
            let sizing: Rc<Cell<OverlaySizing>> = Rc::new(Cell::new(OverlaySizing { height: 0.0 }));
            let sink = Rc::clone(&sizing);
            let installed = Cell::new(false);
            let mut harness =
                Harness::builder()
                    .with_size(vec2(WIDTH, 600.0))
                    .build_ui(move |ui| {
                        if font_first_frame(&installed, ui.ctx()) {
                            return;
                        }
                        sink.set(draw(ui, Some(&view), None, &state, text).sizing);
                    });
            harness.run_steps(2);
            assert!(
                sizing.get().height > 2.0 * capped_height,
                "完整内容高必须上交壳侧钳制（只报视口高会让长选区窗口长不高）: {}",
                sizing.get().height
            );

            harness.set_size(vec2(WIDTH, capped_height));
            harness.run_steps(2);
            for label in ["正在注疏", "Gloss"] {
                let node = harness.get_by_label_contains(label).rect();
                assert!(
                    node.bottom() <= capped_height + 0.5,
                    "长选区不得把页脚挤出窗外（壳侧钳高后生成指示仍须可见）: \
                     {label} bottom={} window_height={capped_height}",
                    node.bottom()
                );
            }

            let seal_before = harness.get_by_label("经").rect();
            let footer_before = harness.get_by_label_contains("正在注疏").rect();
            harness.key_press(egui::Key::ArrowDown);
            harness.run_steps(8);
            let seal_after = harness.get_by_label("经").rect();
            let footer_after = harness.get_by_label_contains("正在注疏").rect();
            assert!(
                seal_after.top() < seal_before.top() - 1.0,
                "经位必须随滚动区上移（长原文尾部可达）: {seal_before:?} -> {seal_after:?}"
            );
            assert!(
                (footer_after.top() - footer_before.top()).abs() < 0.5,
                "页脚不随正文滚动: {footer_before:?} -> {footer_after:?}"
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
    fn the_language_badge_arrives_only_with_the_outcome_verdict() {
        let (mut harness, _clicked) = harness_for(code_streaming_view_en());
        harness.run_steps(3);
        assert!(
            harness.query_all_by_label_contains("rust").next().is_none(),
            "the streaming view carries no verdict: no badge"
        );

        let (mut harness, _clicked) = harness_for(code_outcome_view_en());
        harness.run();
        harness.get_by_label("rust");

        let (mut harness, _clicked) = harness_for(streaming_view_en());
        harness.run_steps(3);
        assert!(
            harness.query_all_by_label_contains("rust").next().is_none(),
            "non-code tasks carry no language badge"
        );
    }

    #[test]
    fn degraded_outcome_card_shows_only_the_abnormal_notice_and_retry() {
        let (mut harness, clicked) = harness_for_locale(degraded_outcome_view_en(), Locale::En);
        harness.run();
        harness.get_by_label_contains("Abnormal output");
        harness.get_by_label("Retry");
        for residue in ["untrustworthy", "rust"] {
            assert!(
                harness
                    .query_all_by_label_contains(residue)
                    .next()
                    .is_none(),
                "degraded card must not render note content: {residue}"
            );
        }
        harness.get_by_label("Retry").click();
        harness.run();
        assert_eq!(
            *clicked.borrow(),
            Some(OverlayAction::Retry),
            "the retry button reuses the failed-card action path"
        );
    }

    #[test]
    fn degraded_outcome_card_never_takes_the_source_jing_section() {
        let state = RenderState::default();
        let text = Text::get(Locale::En);
        let installed = Cell::new(false);
        let view = degraded_outcome_view_en();
        let mut harness = Harness::new_ui(move |ui| {
            if font_first_frame(&installed, ui.ctx()) {
                return;
            }
            let _ = draw(ui, Some(&view), None, &state, text);
        });
        harness.run();
        assert!(
            harness
                .query_all_by_label_contains("fn main()")
                .next()
                .is_none(),
            "the degraded card keeps no source section"
        );
    }

    #[test]
    fn general_outcome_card_renders_the_plain_layout() {
        let (mut harness, _clicked) = harness_for_locale(general_outcome_view_en(), Locale::En);
        harness.run();
        harness.get_by_label_contains("any content at all");
        harness.get_by_label_contains("A direct answer to arbitrary content.");
        harness.get_by_label_contains("first expansion item");
        harness.get_by_label_contains("second expansion item");
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
        harness.get_by_label_contains("密钥无效");
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
            strip.height() >= geometry::overlay::HEADER_ICON,
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

    type ResizeLog = Rc<RefCell<Vec<egui::Vec2>>>;

    fn resize_harness(view: OverlayView) -> (Harness<'static>, ResizeLog) {
        let resizes: ResizeLog = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&resizes);
        let state = RenderState::default();
        let text = Text::get(Locale::Zh);
        let installed = Cell::new(false);
        let harness = Harness::new_ui(move |ui| {
            if font_first_frame(&installed, ui.ctx()) {
                return;
            }
            let output = draw(ui, Some(&view), None, &state, text);
            if let Some(size) = output.resize {
                sink.borrow_mut().push(size);
            }
        });
        (harness, resizes)
    }

    #[test]
    fn resize_zones_are_exposed_to_accesskit() {
        let (mut harness, _resizes) = resize_harness(word_card_view());
        harness.run();
        let zones: Vec<_> = harness
            .query_all_by_label_contains("调整浮层大小")
            .collect();
        assert_eq!(zones.len(), 3, "下缘、右缘与右下角三段热区");
    }

    #[test]
    fn corner_drag_reports_window_size_grown_from_the_press() {
        let (mut harness, resizes) = resize_harness(word_card_view());
        harness.set_size(vec2(WIDTH, 400.0));
        harness.run();
        let corner = harness
            .query_all_by_label_contains("调整浮层大小")
            .map(|node| node.rect())
            .max_by(|a, b| {
                a.right()
                    .total_cmp(&b.right())
                    .then(a.bottom().total_cmp(&b.bottom()))
            })
            .expect("corner resize zone");

        harness.drag_at(corner.center());
        harness.run();
        assert_eq!(
            resizes.borrow().last(),
            Some(&vec2(WIDTH, 400.0)),
            "按下帧确立基点，期望尺寸从当前窗口起算"
        );

        harness.hover_at(corner.center() + vec2(60.0, 40.0));
        harness.run();
        harness.hover_at(corner.center() + vec2(100.0, 80.0));
        harness.run();
        assert_eq!(
            resizes.borrow().last(),
            Some(&vec2(WIDTH + 100.0, 480.0)),
            "期望尺寸按按压点累计（而非逐帧增量）"
        );

        harness.drop_at(corner.center() + vec2(100.0, 80.0));
        harness.run();
        harness.hover_at(corner.center() + vec2(140.0, 100.0));
        harness.run();
        assert_eq!(
            resizes.borrow().last(),
            Some(&vec2(WIDTH + 100.0, 480.0)),
            "松开后粘性保持：壳侧按最后一次期望尺寸接管宽高"
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
            repairing: false,
        };
        let mut harness = Harness::new_ui(move |ui| {
            if font_first_frame(&installed, ui.ctx()) {
                return;
            }
            let _ = draw(ui, Some(&view), Some(&png), &state, text);
        });
        harness.run_steps(3);
        harness.get_by_label_contains("正在注疏");
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

    enum Run {
        Settle,
        Steps(usize),
    }

    struct SnapshotCase {
        name: &'static str,
        view: Option<OverlayView>,
        attached: Option<Arc<[u8]>>,
        locale: Locale,
        run: Run,
    }

    fn snapshot_case(results: &mut egui_kittest::SnapshotResults, case: SnapshotCase) {
        let SnapshotCase {
            name,
            view,
            attached,
            locale,
            run,
        } = case;
        let state = RenderState::default();
        let text = Text::get(locale);
        let installed = Cell::new(false);
        let mut harness = Harness::builder()
            .with_theme(egui::Theme::Light)
            .build_ui(move |ui| {
                if font_first_frame(&installed, ui.ctx()) {
                    return;
                }
                let _ = draw(ui, view.as_ref(), attached.as_ref(), &state, text);
            });
        match run {
            Run::Settle => {
                harness.run();
            }
            Run::Steps(steps) => {
                harness.run_steps(steps);
            }
        }
        harness.snapshot(name);
        results.extend_harness(&mut harness);
    }

    fn case(
        name: &'static str,
        view: Option<OverlayView>,
        locale: Locale,
        run: Run,
    ) -> SnapshotCase {
        SnapshotCase {
            name,
            view,
            attached: None,
            locale,
            run,
        }
    }

    fn core_cases() -> Vec<SnapshotCase> {
        vec![
            case(
                "popup_word_card",
                Some(word_card_view_en()),
                Locale::En,
                Run::Settle,
            ),
            case(
                "popup_streaming",
                Some(streaming_view_en()),
                Locale::En,
                Run::Steps(3),
            ),
            case("popup_failed", Some(failed_view()), Locale::En, Run::Settle),
            case(
                "popup_failed_auth",
                Some(auth_failed_view()),
                Locale::En,
                Run::Settle,
            ),
            case(
                "popup_code_streaming",
                Some(code_streaming_view_en()),
                Locale::En,
                Run::Steps(3),
            ),
            case(
                "popup_image_failed",
                Some(image_too_large_view()),
                Locale::En,
                Run::Settle,
            ),
            case(
                "popup_degraded_outcome",
                Some(degraded_outcome_view_en()),
                Locale::En,
                Run::Settle,
            ),
            case("popup_selfcheck", None, Locale::En, Run::Settle),
            case(
                "popup_long_line",
                Some(long_line_view()),
                Locale::En,
                Run::Settle,
            ),
        ]
    }

    fn image_and_pending_cases() -> Vec<SnapshotCase> {
        vec![
            case(
                "popup_code_outcome",
                Some(code_outcome_view_en()),
                Locale::En,
                Run::Settle,
            ),
            case(
                "popup_image_outcome",
                Some(image_commentary_view_en()),
                Locale::En,
                Run::Settle,
            ),
            case(
                "popup_image_streaming",
                Some(image_streaming_view_en()),
                Locale::En,
                Run::Steps(3),
            ),
            case(
                "popup_pending_text_zh",
                Some(pending_text_view()),
                Locale::Zh,
                Run::Settle,
            ),
            case(
                "popup_pending_text_en",
                Some(pending_text_view_en()),
                Locale::En,
                Run::Settle,
            ),
            case(
                "popup_pending_image_zh",
                Some(pending_image_view()),
                Locale::Zh,
                Run::Settle,
            ),
            case(
                "popup_pending_image_en",
                Some(pending_image_view()),
                Locale::En,
                Run::Settle,
            ),
        ]
    }

    #[test]
    fn snapshots_match_baseline() {
        let mut cases = core_cases();
        cases.extend(image_and_pending_cases());
        for c in &mut cases {
            if [
                "popup_image_outcome",
                "popup_image_streaming",
                "popup_pending_image_zh",
                "popup_pending_image_en",
            ]
            .contains(&c.name)
            {
                c.attached = Some(fixture_png());
            }
        }
        let mut results = egui_kittest::SnapshotResults::new();
        for c in cases {
            snapshot_case(&mut results, c);
        }
        results.unwrap();
    }
}
