//! 经注疏正文排布：三区行（印章列 + 正文列）、经位的代码面板/附件图像/
//! 原文、markdown 渲染管线与楷体排版改写、产物变体的布局分派。
//!
//! 依赖方向：[`super::views`] 的视图分发到这里取分区行与产物正文；本
//! 模块只向下用 [`super::card`] 的印章与朱丝栏线色。

use std::sync::Arc;

use egui::{
    Align, CornerRadius, FontFamily, FontId, Frame, Margin, RichText, Shape, Stroke, TextStyle,
};
use egui_commonmark::CommonMarkViewer;
use gloss_core::task::{OutcomeStructured, TaskKind};

use super::card::{SealTint, seal, zhu_line_color};
use super::content::{
    JING_FONT, PHON_FONT, SHU_FONT, SMALL_FONT, WORD_FONT, ZHU_FONT, kaiti_font, serif_font,
};
use super::{RenderCtx, RenderState};
use crate::ui::code_hl;
use crate::ui::fonts;
use crate::ui::i18n::Text;
use crate::ui::style::{color, font, space};

/// 印章列与正文列之间的空隙（demo .sec gap 10px）。
const SEAL_GAP: f32 = 10.0;
/// 朱丝栏（注区左线）的线宽（demo：2px）。
const ZHU_LINE_WIDTH: f32 = 2.0;
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

/// 经区行：墨印 + 正文列（`body` 在列内绘制，显式垂直布局）。
pub(super) fn jing_section(
    ui: &mut egui::Ui,
    text: &'static Text,
    body: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal_top(|ui| {
        seal(ui, text.gloss_popup_seal_jing.as_str(), SealTint::Ink);
        ui.add_space(SEAL_GAP);
        ui.with_layout(egui::Layout::top_down(Align::LEFT), body);
    });
}

/// 注区行：朱印 + 朱丝栏左线的直接解释（译文/释义/概要）。朱丝栏画在
/// 正文列左缘，线高随正文（跨行延续，demo 的疏密语义）。
pub(super) fn zhu_section(
    ui: &mut egui::Ui,
    text: &'static Text,
    body: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal_top(|ui| {
        seal(ui, text.gloss_popup_seal_zhu.as_str(), SealTint::Zhu);
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
pub(super) fn shu_section(
    ui: &mut egui::Ui,
    text: &'static Text,
    body: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal_top(|ui| {
        seal(ui, text.gloss_popup_seal_shu.as_str(), SealTint::Wei);
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

/// 经位附件图像：纹理经 [`RenderState`] 跨帧复用（同字节恒同纹理，绝不
/// 逐帧解码），宽度随卡宽等比收缩；解码失败落占位降级（字节进不了纹理
/// 不是 panic 面）。
pub(super) fn attached_image(ui: &mut egui::Ui, ctx: &RenderCtx<'_>, png: &Arc<[u8]>) {
    match ctx.state.attached_texture(ui.ctx(), png) {
        Some(texture) => {
            ui.add(egui::Image::new(&texture).max_width(ui.available_width()));
        }
        None => image_unavailable(ui, ctx.text),
    }
}

/// 经位图像的占位降级：图像缺席（无可渲染的字节）或解码失败时，经位落
/// 一行弱色占位，注/疏照常。
fn image_unavailable(ui: &mut egui::Ui, text: &'static Text) {
    ui.label(
        RichText::new(&text.gloss_popup_image_unavailable)
            .font(serif_font(SMALL_FONT))
            .weak(),
    );
}

/// 经区块的正文：原文/词条随任务形制。代码任务为单层代码面板——代码
/// 底色直接铺满经位区块（用户反馈定稿：无外层底色的界栏框），配等宽
/// 体、按可用宽折行、左上角语言标签行与 syntect（Sublime 语法集）的六类
/// 语法着色；其余为宋体原文。
pub(super) fn source_block(ui: &mut egui::Ui, source: &str, code: bool, code_lang: Option<&str>) {
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
                // 等宽折行：超宽按可用宽换行（滚动区只纵向滚，横滚不进
                // 弹窗）。
                let job = code_job(ui, source, code_lang, ui.available_width());
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

/// 代码正文的着色排版：syntect 分词 + egui_extras 出 [`egui::text::LayoutJob`]
/// （六类色由 `code_hl` 的自定义主题注入），再补上本面板的折行与行高。
/// 语言未定或表外语言落 [`code_hl::fallback_job`] 的通用启发集——不猜具体
/// 语言，只认跨语言稳定的字符串/注释/数字/类型/函数形。
fn code_job(
    ui: &egui::Ui,
    source: &str,
    code_lang: Option<&str>,
    wrap_width: f32,
) -> egui::text::LayoutJob {
    let dark = ui.visuals().dark_mode;
    let font_id = FontId::new(CODE_FONT, fonts::mono_family());
    let Some(mut job) =
        code_hl::highlight(ui.ctx(), ui.style(), font_id.clone(), source, code_lang)
    else {
        return code_hl::fallback_job(source, &font_id, dark, wrap_width, CODE_LINE_HEIGHT);
    };
    job.wrap = egui::text::TextWrapping {
        max_width: wrap_width,
        break_anywhere: true,
        ..egui::text::TextWrapping::no_max_width()
    };
    job.break_on_newline = true;
    for section in &mut job.sections {
        // 逐段写回字体：egui_extras 在分词失败时退回的纯文本用 style 派生的
        // 字体，不写回会丢掉 gloss-mono。
        section.format.font_id = font_id.clone();
        section.format.line_height = Some(CODE_LINE_HEIGHT);
    }
    job
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

/// 注/疏区共用的文本样式改写：markdown 正文与列表项落到楷体（egui_commonmark
/// 经 `ui.style().text_styles` 取字体，改写即生效；代码块仍走等宽族）。
/// 注正文用注字号（[`ZHU_FONT`]），疏条目用疏字号（[`SHU_FONT`]）。
pub(super) fn apply_markdown_typography(ui: &mut egui::Ui, body_size: f32) {
    for (style, size) in [
        (TextStyle::Body, body_size),
        (TextStyle::Button, body_size),
        (TextStyle::Small, SMALL_FONT),
        (TextStyle::Heading, JING_FONT),
    ] {
        ui.style_mut().text_styles.insert(style, kaiti_font(size));
    }
}

/// markdown 正文：完整渲染（egui_commonmark 解析绘制），缓存跨帧持有。
pub(super) fn render_markdown(ui: &mut egui::Ui, state: &RenderState, text: &str) {
    let mut cache = state.cache.borrow_mut();
    CommonMarkViewer::new().show(ui, &mut cache, text);
}

/// 该任务是否按代码排版（经位走代码面板）：流式视图看自动分类的
/// 判定，完成态看产物任务的类型。
pub(super) fn is_code(kind: Option<TaskKind>) -> bool {
    kind == Some(TaskKind::ExplainCode)
}

/// 产物正文的代码语言（渲染时现算）：唯一来源是产物的
/// `structured.code_language`（LLM 判定），经 `code_hl::adopt` 过采纳
/// 门槛与归一化；非代码变体恒无语言。
fn outcome_code_lang(outcome: &gloss_core::task::TaskOutcome) -> Option<String> {
    match &outcome.structured {
        OutcomeStructured::Code { code_language, .. } => code_hl::adopt(code_language.as_deref()),
        _ => None,
    }
}

/// 产物正文（经注疏排布）：经（原文；图像任务为附件图像本身，缺席或
/// 解码失败落占位）、注（markdown 注文，词卡的义/句译的译文/讲解的正文）、
/// 疏（examples 疏证逐条）。代码语言按产物判定现算（见
/// [`outcome_code_lang`]）。
pub(super) fn outcome_body(
    ui: &mut egui::Ui,
    ctx: &RenderCtx<'_>,
    source: &str,
    outcome: &gloss_core::task::TaskOutcome,
) {
    let code_lang = outcome_code_lang(outcome);
    match &outcome.structured {
        OutcomeStructured::WordCard { phonetic, examples } => {
            if !source.trim().is_empty() {
                jing_section(ui, ctx.text, |ui| {
                    word_head(ui, source, phonetic.as_deref())
                });
                ui.add_space(space::PARAGRAPH);
            }
            zhu_and_shu(ui, ctx, &outcome.note, examples);
        }
        OutcomeStructured::Code { examples, .. } | OutcomeStructured::Plain { examples, .. }
            if outcome.kind == TaskKind::ImageExplain =>
        {
            // 图像任务（恒落 Plain 变体）：经位是图像本身（布局开关＝
            // 附件在场，不依赖 classified 到达时序）；缺图回落原文经位
            // （缓存命中等会话边缘），再缺落占位。
            match ctx.attached {
                Some(png) => jing_section(ui, ctx.text, |ui| attached_image(ui, ctx, png)),
                None if !source.trim().is_empty() => {
                    jing_section(ui, ctx.text, |ui| source_block(ui, source, false, None));
                }
                None => jing_section(ui, ctx.text, |ui| image_unavailable(ui, ctx.text)),
            }
            ui.add_space(space::PARAGRAPH);
            zhu_and_shu(ui, ctx, &outcome.note, examples);
        }
        OutcomeStructured::Plain { examples } | OutcomeStructured::Code { examples, .. } => {
            if !source.trim().is_empty() {
                jing_section(ui, ctx.text, |ui| {
                    source_block(
                        ui,
                        source,
                        is_code(Some(outcome.kind)),
                        code_lang.as_deref(),
                    )
                });
                ui.add_space(space::PARAGRAPH);
            }
            zhu_and_shu(ui, ctx, &outcome.note, examples);
        }
    }
}

/// 注 + 疏：三个产物变体共用（注文 markdown 与例句列表的位置固定）。
fn zhu_and_shu(ui: &mut egui::Ui, ctx: &RenderCtx<'_>, note: &str, examples: &[String]) {
    zhu_section(ui, ctx.text, |ui| {
        apply_markdown_typography(ui, ZHU_FONT);
        render_markdown(ui, ctx.state, note);
    });
    if !examples.is_empty() {
        ui.add_space(space::PARAGRAPH);
        shu_examples(ui, ctx, examples);
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

/// 疏位疏证（examples 逐条）：条目在 code 模板下是 markdown、其余模板下
/// 以纯文本为主（general 的链接语法除外），统一走注文同一条 markdown
/// 渲染管线（楷体疏字号、正文色，纯文本不变形）——条目的出处/原文/
/// 按断结构由条目自身的内容承载，不做拆行整形与列表符装饰。
pub(super) fn shu_examples(ui: &mut egui::Ui, ctx: &RenderCtx<'_>, examples: &[String]) {
    shu_section(ui, ctx.text, |ui| {
        apply_markdown_typography(ui, SHU_FONT);
        for (index, example) in examples.iter().enumerate() {
            if index > 0 {
                ui.add_space(space::INLINE);
            }
            render_markdown(ui, ctx.state, example);
        }
    });
}

/// 自检卡正文（预热与显隐自检路径）：中英混排一眼可辨字体链路健康。
pub(super) fn selfcheck_body(ui: &mut egui::Ui) {
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
