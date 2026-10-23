//! 浮层视图分发与卡骨架：按 [`OverlayView`] 变体选卡（自检、流式、降级
//! 产物、产物、失败），平铺/滚动骨架在此拼装；经位与注疏的正文排布委托
//! [`super::outcome`]，页头页脚印章等固定件委托 [`super::card`]。
//!
//! 骨架条（「经显注未至」的占位脉动条）也在这里：它是流式正文的占位
//! 形态，与页脚呼吸点同源脉动。

use std::borrow::Cow;
use std::time::Duration;

use egui::{RichText, ScrollArea, vec2};

use super::card::{footer, header};
use super::content::ZHU_FONT;
use super::note::{StreamFields, stream_fields};
use super::outcome::{
    apply_markdown_typography, attached_image, is_code, jing_section, outcome_body,
    render_markdown, selfcheck_body, shu_examples, source_block, zhu_section,
};
use super::{OverlayAction, PULSE_PERIOD_SECS, RenderCtx};
use crate::machine::{ErrorAction, FailureCause, OverlayView};
use crate::ui::i18n::Text;
use crate::ui::style::{font, space};

/// 滚动区视口的下限：窗口被压得极矮时正文至少还能滚出这么多。
const MIN_BODY_VIEWPORT: f32 = 48.0;
/// 页脚与正文之间的空隙；滚动区按这条带宽预留视口
/// （auto_shrink(false) 的滚动区会吃光剩余空间，不预留页脚就被挤出窗外）。
const FOOTER_RESERVE: f32 = super::FOOTER_HEIGHT + space::PARAGRAPH;
/// 方向键的单次滚动步长（约三行正文；按住由系统按键重复续推）。
const ARROW_SCROLL_STEP: f32 = 48.0;
/// 骨架条高（「经显注未至」的占位行）
const SHIMMER_BAR_HEIGHT: f32 = 12.0;
/// 骨架条圆角
const SHIMMER_BAR_RADIUS: u8 = 4;
/// 骨架条铺底的透明度档（0-255）：脉动在这个下限与峰值之间摆动
const SHIMMER_ALPHA_MIN: u8 = 38;
const SHIMMER_ALPHA_MAX: u8 = 96;

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

/// 浮层聚焦时的方向键滚动：↑/↓ 平移滚动区视口，内容不溢出时到界即停。
/// 在滚动区内容闭包的起始处调用（无修饰键参与，不与其它快捷键争抢）。
fn arrow_key_scroll(ui: &egui::Ui) {
    let step = ui.input(|input| {
        if !input.modifiers.any() {
            if input.key_pressed(egui::Key::ArrowDown) {
                return Some(-ARROW_SCROLL_STEP);
            }
            if input.key_pressed(egui::Key::ArrowUp) {
                return Some(ARROW_SCROLL_STEP);
            }
        }
        None
    });
    if let Some(dy) = step {
        ui.scroll_with_delta(egui::vec2(0.0, dy));
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

/// 流式正文（注位渐进 markdown + 疏位已到达的解读条目）：两字段全空落
/// 骨架（经已回显、注未至——无注印，还没有可注的内容）。
fn streamed_sections(ui: &mut egui::Ui, ctx: &RenderCtx<'_>, fields: &StreamFields) {
    if fields.note.is_empty() && fields.examples.is_empty() {
        shimmer_bars(ui);
        return;
    }
    if !fields.note.is_empty() {
        zhu_section(ui, ctx.text, |ui| {
            apply_markdown_typography(ui, ZHU_FONT);
            render_markdown(ui, ctx.state, &fields.note);
        });
    }
    if !fields.examples.is_empty() {
        ui.add_space(space::PARAGRAPH);
        shu_examples(ui, ctx, &fields.examples);
    }
}

/// 流式卡的滚动区正文：经位由调用方定形制（附件图像或原文块），注/疏
/// 两区由 [`streamed_sections`] 固定跟进。
fn streamed_body(
    ui: &mut egui::Ui,
    ctx: &RenderCtx<'_>,
    fields: &StreamFields,
    jing: impl FnOnce(&mut egui::Ui),
) {
    jing_section(ui, ctx.text, jing);
    ui.add_space(space::PARAGRAPH);
    streamed_sections(ui, ctx, fields);
}

/// 浮层内容（头部 + 各视图正文），并把完整内容高记入 `content_h`：
/// 产物与流式正文（含经位）放进 ScrollArea（完整渲染、超出滚动兜底），
/// 其高度取 ScrollArea 报告的内容尺寸，不受视口裁剪影响；页脚带恒在
/// （滚动区按 [`FOOTER_RESERVE`] 预留视口，页脚不被内容挤出窗外）。头部
/// 动作区与失败卡动作按钮的点击结果、页头拖动热区的位移一并透传给调用方。
pub(super) fn render_content(
    ui: &mut egui::Ui,
    ctx: &RenderCtx<'_>,
    view: Option<&OverlayView>,
    content_h: &mut f32,
) -> (Option<OverlayAction>, Option<egui::Vec2>) {
    match view {
        None => plain_card(ui, ctx, content_h, |ui, _| selfcheck_body(ui)),
        Some(OverlayView::Streaming {
            source,
            raw,
            classified,
            repairing,
        }) => {
            let fields = stream_fields(raw);
            let code = is_code(*classified);
            let mode = FooterMode::Streaming {
                repairing: *repairing,
            };
            match ctx.attached {
                // 图像卡：经位图像与注/疏一起进滚动区——高图由滚动区吞
                // 高，窗口按完整内容高申请、壳侧钳到屏，超出滚动兜底。
                Some(png) => scrolled_card(ui, ctx, content_h, mode, |ui| {
                    streamed_body(ui, ctx, &fields, |ui| attached_image(ui, ctx, png));
                }),
                // 经位与注/疏同一滚动区：长选区只抬高滚动内容，页脚仍钉在
                // 视口下方（经位若留在滚动区外，长选区会把页脚顶出窗外，
                // 生成指示随之不可见）。
                None => scrolled_card(ui, ctx, content_h, mode, |ui| {
                    // 流式期没有语言判定（唯一来源是产物的 structured
                    // 回传），经位代码面板按通用启发集着色、无角标。
                    streamed_body(ui, ctx, &fields, |ui| source_block(ui, source, code, None));
                }),
            }
        }
        Some(OverlayView::Outcome { source: _, outcome }) if outcome.degraded => {
            degraded_card(ui, ctx, content_h)
        }
        Some(OverlayView::Outcome { source, outcome }) => {
            scrolled_card(ui, ctx, content_h, FooterMode::Static, |ui| {
                outcome_body(ui, ctx, source, outcome)
            })
        }
        Some(OverlayView::Failed {
            cause,
            action: error_action,
        }) => failed_card(ui, ctx, content_h, cause, error_action),
    }
}

/// 不滚动的卡骨架：头部 + 正文 + 页脚，正文高度即内容高（自检、降级与
/// 失败卡共用）。正文可写回卡片动作（重试/打开设置）。
fn plain_card(
    ui: &mut egui::Ui,
    ctx: &RenderCtx<'_>,
    content_h: &mut f32,
    body: impl FnOnce(&mut egui::Ui, &mut Option<OverlayAction>),
) -> (Option<OverlayAction>, Option<egui::Vec2>) {
    let (mut action, drag) = header(ui, ctx.state, ctx.text);
    ui.add_space(space::SECTION);
    body(ui, &mut action);
    ui.add_space(space::PARAGRAPH);
    footer(ui, false, false, ctx.text);
    *content_h = ui.min_rect().height();
    (action, drag)
}

/// 页脚形态：流式期显示注解状态（`repairing` 区分首轮与重新注解），
/// 其余卡片是静态页脚。
#[derive(Clone, Copy)]
enum FooterMode {
    Static,
    Streaming { repairing: bool },
}

/// 滚动卡骨架：头部 + 纵向滚动正文 + 页脚。正文（经位与注/疏）整体进
/// 滚动区，完整高按滚动区报告的内容尺寸回记（不受视口裁剪影响）；页脚
/// 在滚动区之外、随其下缘排布——正文再长也只在滚动区内部滚动，不会把
/// 页脚顶出窗外。
fn scrolled_card(
    ui: &mut egui::Ui,
    ctx: &RenderCtx<'_>,
    content_h: &mut f32,
    mode: FooterMode,
    body: impl FnOnce(&mut egui::Ui),
) -> (Option<OverlayAction>, Option<egui::Vec2>) {
    let (action, drag) = header(ui, ctx.state, ctx.text);
    ui.add_space(space::SECTION);
    // 只纵向滚动：代码正文按可用宽折行。
    let scrolled = ScrollArea::new([false, true])
        .auto_shrink([false, true])
        .max_height(viewport_max(ui))
        .show(ui, |ui| {
            arrow_key_scroll(ui);
            body(ui);
        });
    ui.add_space(space::PARAGRAPH);
    match mode {
        FooterMode::Static => footer(ui, false, false, ctx.text),
        FooterMode::Streaming { repairing } => footer(ui, true, repairing, ctx.text),
    }
    record_scrolled_height(ui, content_h, &scrolled);
    (action, drag)
}

/// 降级产物卡（契约校验失败）：渲染「输出异常」+ 复用重试按钮，
/// 不渲染注疏内容——原文垃圾不展示不留存（缓存侧同样不收，
/// 见 `crate::runtime::pipeline`）。
fn degraded_card(
    ui: &mut egui::Ui,
    ctx: &RenderCtx<'_>,
    content_h: &mut f32,
) -> (Option<OverlayAction>, Option<egui::Vec2>) {
    plain_card(ui, ctx, content_h, |ui, action| {
        ui.label(
            RichText::new(&ctx.text.gloss_popup_output_abnormal)
                .size(font::NOTICE)
                .color(ui.visuals().warn_fg_color),
        );
        ui.add_space(space::SECTION);
        if ui
            .button(
                RichText::new(ctx.text.gloss_popup_retry.as_str())
                    .size(font::CAPTION)
                    .color(ui.visuals().strong_text_color()),
            )
            .clicked()
        {
            *action = Some(OverlayAction::Retry);
        }
    })
}

/// 失败卡：按失败来源的文案（含占位符变体）+ 可选动作按钮（重试/打开
/// 设置）。
fn failed_card(
    ui: &mut egui::Ui,
    ctx: &RenderCtx<'_>,
    content_h: &mut f32,
    cause: &FailureCause,
    error_action: &Option<ErrorAction>,
) -> (Option<OverlayAction>, Option<egui::Vec2>) {
    plain_card(ui, ctx, content_h, |ui, action| {
        ui.label(
            RichText::new(failure_message(cause, ctx.text))
                .size(font::NOTICE)
                .color(ui.visuals().warn_fg_color),
        );
        if let Some(error_action) = *error_action {
            ui.add_space(space::SECTION);
            if ui
                .button(
                    RichText::new(action_label(error_action, ctx.text))
                        .size(font::CAPTION)
                        .color(ui.visuals().strong_text_color()),
                )
                .clicked()
            {
                *action = Some(error_action.into());
            }
        }
    })
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
fn action_label(action: ErrorAction, text: &'static Text) -> &'static str {
    match action {
        ErrorAction::Retry => text.gloss_popup_retry.as_str(),
        ErrorAction::OpenSettings => text.gloss_popup_open_settings.as_str(),
    }
}
