//! 经注疏正文排布：三区行（印章列与正文列）、经位的代码面板、附件图像、
//! 原文、标记文档（Markdown）渲染管线与楷体排版改写、产物变体的布局分派。
//!
//! 依赖方向：视图分发到这里取分区行与产物正文；本
//! 模块只向下用卡片固定件的印章与朱丝栏线色。

use std::sync::Arc;

use egui::{Align, Frame, Margin, RichText, Shape, Stroke};
use egui_commonmark::CommonMarkViewer;
use gloss_core::task::{OutcomeStructured, TaskKind};

use super::card::{SealTint, seal, zhu_line_color};
use super::{RenderCtx, RenderState};
use crate::ui::code_hl;
use crate::ui::i18n::Text;
use crate::ui::style::{
    apply_kaiti, geometry, line_height, mono, palette, radius, sans, serif, size, space, stroke,
};

const CODE_LINE_HEIGHT: f32 = size::CODE * line_height::CODE;

pub(crate) fn annotation_text(outcome: &gloss_core::task::TaskOutcome) -> String {
    let examples = match &outcome.structured {
        OutcomeStructured::WordCard { examples, .. }
        | OutcomeStructured::Code { examples, .. }
        | OutcomeStructured::Plain { examples } => examples,
    };
    let mut text = outcome.note.clone();
    for example in examples {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(example);
    }
    text
}

pub(super) fn jing_section(
    ui: &mut egui::Ui,
    text: &'static Text,
    body: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal_top(|ui| {
        seal(ui, text, text.gloss_popup_seal_jing.as_str(), SealTint::Ink);
        ui.add_space(geometry::seal::GAP);
        ui.with_layout(egui::Layout::top_down(Align::LEFT), body);
    });
}

pub(super) fn zhu_section(
    ui: &mut egui::Ui,
    text: &'static Text,
    body: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal_top(|ui| {
        seal(ui, text, text.gloss_popup_seal_zhu.as_str(), SealTint::Zhu);
        ui.add_space(geometry::seal::GAP);
        let line_top = ui.cursor().top();
        let column = ui
            .with_layout(egui::Layout::top_down(Align::LEFT), |ui| {
                Frame::new()
                    .inner_margin(Margin {
                        left: geometry::seal::ZHU_TEXT_GAP.round() as i8,
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
            Stroke::new(stroke::THICK, zhu_line_color(ui)),
        );
    });
}

pub(super) fn shu_section(
    ui: &mut egui::Ui,
    text: &'static Text,
    body: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal_top(|ui| {
        seal(ui, text, text.gloss_popup_seal_shu.as_str(), SealTint::Wei);
        ui.add_space(geometry::seal::GAP);
        let column = ui
            .with_layout(egui::Layout::top_down(Align::LEFT), |ui| {
                Frame::new()
                    .inner_margin(Margin {
                        top: geometry::seal::SHU_TEXT_GAP.round() as i8,
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
            Stroke::new(stroke::LINE, ui.visuals().weak_text_color()),
            geometry::seal::SHU_DASH_LENGTH,
            geometry::seal::SHU_GAP_LENGTH,
        );
        ui.painter().extend(dashes);
    });
}

pub(super) fn attached_image(ui: &mut egui::Ui, ctx: &RenderCtx<'_>, png: &Arc<[u8]>) {
    match ctx.state.attached_texture(ui.ctx(), png) {
        Some(texture) => {
            ui.add(egui::Image::new(&texture).max_width(ui.available_width()));
        }
        None => image_unavailable(ui, ctx.text),
    }
}

pub(super) fn image_unavailable(ui: &mut egui::Ui, text: &'static Text) {
    ui.label(
        RichText::new(&text.gloss_popup_image_unavailable)
            .font(serif(size::NOTICE))
            .weak(),
    );
}

pub(super) fn source_block(ui: &mut egui::Ui, source: &str, code: bool, code_lang: Option<&str>) {
    let strong = ui.visuals().strong_text_color();
    if code {
        let colors = palette(ui);
        Frame::new()
            .fill(colors.code_bg)
            .stroke(Stroke::new(stroke::LINE, colors.code_border))
            .corner_radius(radius::CODE_PANEL)
            .inner_margin(egui::vec2(
                geometry::code_panel::PADDING_H,
                geometry::code_panel::PADDING_V,
            ))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                code_badge(ui, code_lang);
                let job = code_job(ui, source, code_lang, ui.available_width());
                let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                ui.add(egui::Label::new(egui::WidgetText::Galley(galley)).selectable(true));
            });
        return;
    }
    ui.add(
        egui::Label::new(RichText::new(source).font(serif(size::JING)).color(strong))
            .wrap()
            .selectable(true),
    );
}

fn code_job(
    ui: &egui::Ui,
    source: &str,
    code_lang: Option<&str>,
    wrap_width: f32,
) -> egui::text::LayoutJob {
    let font_id = mono(size::CODE);
    let Some(mut job) = code_hl::highlight(ui, font_id.clone(), source, code_lang) else {
        return code_hl::fallback_job(
            source,
            &font_id,
            &palette(ui).syntax,
            wrap_width,
            CODE_LINE_HEIGHT,
        );
    };
    job.wrap = egui::text::TextWrapping {
        max_width: wrap_width,
        break_anywhere: true,
        ..egui::text::TextWrapping::no_max_width()
    };
    job.break_on_newline = true;
    for section in &mut job.sections {
        section.format.font_id = font_id.clone();
        section.format.line_height = Some(CODE_LINE_HEIGHT);
    }
    job
}

fn code_badge(ui: &mut egui::Ui, lang: Option<&str>) {
    let Some(lang) = lang else {
        return;
    };
    ui.label(
        RichText::new(lang)
            .font(sans(size::CODE_BADGE))
            .color(ui.visuals().weak_text_color()),
    )
    .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, lang.to_owned()));
    ui.add_space(CODE_LINE_HEIGHT);
}

pub(super) fn render_markdown(ui: &mut egui::Ui, state: &RenderState, text: &str) {
    let mut cache = state.cache.borrow_mut();
    CommonMarkViewer::new().show(ui, &mut cache, text);
}

pub(super) fn is_code(kind: Option<TaskKind>) -> bool {
    kind == Some(TaskKind::ExplainCode)
}

fn outcome_code_lang(outcome: &gloss_core::task::TaskOutcome) -> Option<String> {
    match &outcome.structured {
        OutcomeStructured::Code { code_language, .. } => code_hl::adopt(code_language.as_deref()),
        _ => None,
    }
}

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

fn zhu_and_shu(ui: &mut egui::Ui, ctx: &RenderCtx<'_>, note: &str, examples: &[String]) {
    zhu_section(ui, ctx.text, |ui| {
        apply_kaiti(ui, size::ZHU);
        render_markdown(ui, ctx.state, note);
    });
    if !examples.is_empty() {
        ui.add_space(space::PARAGRAPH);
        shu_examples(ui, ctx, examples);
    }
}

fn word_head(ui: &mut egui::Ui, word: &str, phonetic: Option<&str>) {
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(word)
                .font(serif(size::WORD))
                .color(ui.visuals().strong_text_color()),
        );
        if let Some(phonetic) = phonetic {
            ui.label(
                RichText::new(phonetic)
                    .font(mono(size::PHON))
                    .color(ui.visuals().weak_text_color()),
            );
        }
    });
}

pub(super) fn shu_examples(ui: &mut egui::Ui, ctx: &RenderCtx<'_>, examples: &[String]) {
    shu_section(ui, ctx.text, |ui| {
        apply_kaiti(ui, size::SHU);
        for (index, example) in examples.iter().enumerate() {
            if index > 0 {
                ui.add_space(space::INLINE);
            }
            render_markdown(ui, ctx.state, example);
        }
    });
}

pub(super) fn selfcheck_body(ui: &mut egui::Ui) {
    let strong = ui.visuals().strong_text_color();
    let weak = ui.visuals().weak_text_color();
    ui.label(
        RichText::new("The quick brown fox jumps over the lazy dog.") // i18n:allow 自检样张，字体链路夹具
            .size(size::NOTICE)
            .color(weak),
    );
    ui.add_space(space::SECTION);
    ui.label(
        RichText::new("敏捷的棕色狐狸从懒狗身上跳过。") // i18n:allow 自检字样，字体链路夹具
            .size(size::TITLE)
            .strong()
            .color(strong),
    );
    ui.add_space(space::SECTION);
    ui.label(
        RichText::new("中文渲染自检：划词翻译、代码解释、图片识别。") // i18n:allow 自检字样，字体链路夹具
            .size(size::NOTICE)
            .color(weak),
    );
}

#[cfg(test)]
mod tests {
    use gloss_core::task::{OutcomeStructured, TaskKind, TaskOutcome};

    use super::annotation_text;

    fn outcome(structured: OutcomeStructured) -> TaskOutcome {
        TaskOutcome {
            kind: TaskKind::TranslateWord,
            note: "注疏正文".into(),
            structured,
            degraded: false,
            lang_violations: Vec::new(),
        }
    }

    #[test]
    fn annotation_text_joins_the_note_and_the_examples() {
        let full = outcome(OutcomeStructured::Plain {
            examples: vec!["例句一".into(), "例句二".into()],
        });
        assert_eq!(
            annotation_text(&full),
            "注疏正文\n例句一\n例句二",
            "疏以换行拼在注后"
        );
    }

    #[test]
    fn annotation_text_without_examples_carries_no_trailing_newline() {
        let bare = outcome(OutcomeStructured::Plain {
            examples: Vec::new(),
        });
        assert_eq!(annotation_text(&bare), "注疏正文");
    }

    #[test]
    fn annotation_text_keeps_the_source_out_of_the_copy() {
        let card = outcome(OutcomeStructured::WordCard {
            phonetic: Some("/ɡlɒs/".into()),
            examples: vec!["例句".into()],
        });
        let text = annotation_text(&card);
        assert!(!text.contains("/ɡlɒs/"), "音标不进复制内容");
        assert_eq!(text, "注疏正文\n例句");
    }
}
