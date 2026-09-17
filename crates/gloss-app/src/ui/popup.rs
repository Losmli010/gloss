//! 浮层内容：按 [`TaskKind`] 分发的结果卡与流式/失败视图（M3-T9）。
//!
//! 词卡精排（音标/词性/释义/例句），其余任务展示 markdown 正文（M3 以
//! 可选中富文本呈现，语法级 markdown 渲染在需要时引入 egui_commonmark）；
//! OCR 另提供纯文本一键复制。流式视图按 [`STRUCTURED_FENCE`] 过滤已
//! 完整出现的结构化块（在累积文本上按最后围栏标记截断）；跨 chunk 切
//! 分出的残缺围栏前缀可能短暂显示，随下一 chunk 自愈。

use egui::{Color32, CornerRadius, Frame, Margin, RichText, ScrollArea, Stroke, vec2};
use gloss_core::prompt::STRUCTURED_FENCE;
use gloss_core::task::OutcomeStructured;

use crate::machine::{ErrorAction, OverlayView};

/// 浮层宽度（UI 规范 §2）
pub const WIDTH: f32 = 380.0;
/// 卡片圆角（UI 规范 §2）
const CORNER_RADIUS: u8 = 8;
/// 内容区内边距（UI 规范 §2）
const PADDING: i8 = 14;
/// 头部身份圆点：品牌珊瑚橙（UI 规范 §〇）
const BRAND_DOT: Color32 = Color32::from_rgb(0xD8, 0x5A, 0x30);
/// 流式正文/产物的展示字符上限（超出截断，浮层窗口固定）。
const MAX_BODY_CHARS: usize = 4000;

/// 画一帧浮层。根 `Ui` 覆盖整个窗口，卡片铺满它，圆角之外由透明窗口露出桌面。
///
/// `view` 为 `None` 时显示渲染自检卡（预热与自检路径）。返回本帧被点击
/// 的失败卡动作按钮（重试/打开设置），由壳执行——浮层只渲染、不副作用。
pub(crate) fn draw(ui: &mut egui::Ui, view: Option<&OverlayView>) -> Option<ErrorAction> {
    let fill = ui.visuals().window_fill;
    let stroke = ui.visuals().window_stroke;

    let mut clicked = None;
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(0.5, stroke.color))
        .corner_radius(CornerRadius::same(CORNER_RADIUS))
        .inner_margin(Margin::same(PADDING))
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            match view {
                None => {
                    header(ui, "自检", None);
                    ui.add_space(12.0);
                    selfcheck_body(ui);
                }
                Some(OverlayView::Streaming { source, body }) => {
                    header(ui, "推理中", None);
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(source)
                            .size(13.0)
                            .color(ui.visuals().weak_text_color()),
                    );
                    ui.add_space(8.0);
                    streamed_body(ui, body);
                }
                Some(OverlayView::Outcome(outcome)) => {
                    header(
                        ui,
                        crate::ui::kind_label(outcome.kind),
                        Some(copy_text(outcome)),
                    );
                    ui.add_space(12.0);
                    ScrollArea::vertical()
                        .auto_shrink(false)
                        .show(ui, |ui| outcome_body(ui, outcome));
                }
                Some(OverlayView::Failed { message, action }) => {
                    header(ui, "失败", None);
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(message.as_str())
                            .size(13.0)
                            .color(ui.visuals().warn_fg_color),
                    );
                    if let Some(action) = *action {
                        ui.add_space(12.0);
                        if ui
                            .button(
                                RichText::new(action_label(action))
                                    .size(12.0)
                                    .color(ui.visuals().strong_text_color()),
                            )
                            .clicked()
                        {
                            clicked = Some(action);
                        }
                    }
                }
            }
        });
    clicked
}

/// 动作按钮的文案。
fn action_label(action: ErrorAction) -> &'static str {
    match action {
        ErrorAction::Retry => "重试",
        ErrorAction::OpenSettings => "打开设置",
    }
}

/// 复制按钮写入剪贴板的文本：OCR 取纯文本，其余取 markdown 正文。
fn copy_text(outcome: &gloss_core::task::TaskOutcome) -> String {
    match &outcome.structured {
        OutcomeStructured::Extracted { text } => text.clone(),
        _ => outcome.body.clone(),
    }
}

/// 头部：身份圆点 + 标题；右侧状态位与可选的一键复制按钮。
fn header(ui: &mut egui::Ui, tag: &str, copy: Option<String>) {
    let weak = ui.visuals().weak_text_color();
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 4.0, BRAND_DOT);
        ui.label(RichText::new("翻译").size(12.0).color(weak));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(text) = copy
                && ui
                    .button(RichText::new("复制").size(11.0).color(weak))
                    .clicked()
            {
                ui.ctx().copy_text(text);
            }
            ui.label(RichText::new(tag).size(11.0).color(weak));
        });
    });
}

/// 流式正文：过滤掉已开始出现的结构化块（可能跨 chunk 切分，因此在
/// 累积文本上按最后一次围栏标记截断），可滚动查看。
fn streamed_body(ui: &mut egui::Ui, raw: &str) {
    let visible = match raw.rfind(STRUCTURED_FENCE) {
        Some(pos) => &raw[..pos],
        None => raw,
    };
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        ui.label(
            RichText::new(truncate(visible))
                .size(14.0)
                .strong()
                .color(ui.visuals().strong_text_color()),
        );
    });
}

/// 产物正文：词卡精排，其余任务展示 markdown 正文/提取文本。
fn outcome_body(ui: &mut egui::Ui, outcome: &gloss_core::task::TaskOutcome) {
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
                        .size(15.0)
                        .strong()
                        .color(ui.visuals().strong_text_color()),
                );
                ui.add_space(8.0);
            }
            selectable_body(ui, &outcome.body);
        }
        OutcomeStructured::Extracted { text } => selectable_body(ui, text),
    }
}

/// 词卡精排：词条 + 音标，按词性分组的释义与例句（UI 规范 §3.3 的最小
/// 落地；精排细节随 M4 真实数据调优）。
fn word_card(
    ui: &mut egui::Ui,
    word: &str,
    phonetic: Option<&str>,
    senses: &[gloss_core::task::Sense],
) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(word)
                .size(18.0)
                .strong()
                .color(ui.visuals().strong_text_color()),
        );
        if let Some(phonetic) = phonetic {
            ui.label(
                RichText::new(phonetic)
                    .size(13.0)
                    .color(ui.visuals().weak_text_color()),
            );
        }
    });
    ui.add_space(10.0);
    let strong = ui.visuals().strong_text_color();
    let weak = ui.visuals().weak_text_color();
    for sense in senses {
        ui.horizontal_wrapped(|ui| {
            if let Some(pos) = &sense.pos {
                ui.label(RichText::new(pos.as_str()).size(13.0).color(weak));
            }
            ui.label(
                RichText::new(sense.meaning.as_str())
                    .size(14.0)
                    .color(strong),
            );
        });
        for example in &sense.examples {
            ui.indent("example", |ui| {
                ui.label(RichText::new(format!("· {example}")).size(12.0).color(weak));
            });
        }
        ui.add_space(6.0);
    }
}

/// 可选中、自动换行的正文标签（超长截断；markdown 富渲染按需后续引入）。
fn selectable_body(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            RichText::new(truncate(text))
                .size(14.0)
                .color(ui.visuals().strong_text_color()),
        )
        .wrap()
        .selectable(true),
    );
}

/// 展示截断：超出上限的尾部以省略号收束（截断只发生在展示层）。
fn truncate(text: &str) -> String {
    if text.chars().count() <= MAX_BODY_CHARS {
        return text.to_owned();
    }
    let mut truncated: String = text.chars().take(MAX_BODY_CHARS).collect();
    truncated.push('…');
    truncated
}

/// 自检卡正文（预热与显隐自检路径）：中英混排一眼可辨字体链路健康。
fn selfcheck_body(ui: &mut egui::Ui) {
    let strong = ui.visuals().strong_text_color();
    let weak = ui.visuals().weak_text_color();
    ui.label(
        RichText::new("The quick brown fox jumps over the lazy dog.")
            .size(13.0)
            .color(weak),
    );
    ui.add_space(12.0);
    ui.label(
        RichText::new("敏捷的棕色狐狸从懒狗身上跳过。")
            .size(15.0)
            .strong()
            .color(strong),
    );
    ui.add_space(12.0);
    // 中英混排 + 中文标点：字体 fallback 链接没接上，一眼能看出来
    ui.label(
        RichText::new("中文渲染自检：划词翻译、代码解释、图片识别。")
            .size(13.0)
            .color(weak),
    );
}

#[cfg(test)]
mod kittest_tests {
    //! L2 浮层 harness 测试：真实 `draw` 喂 `OverlayView` 序列，经
    //! AccessKit 树断言内容、模拟点击复制按钮；快照对比仅 macos 门控
    //! （跨平台渲染差异，见仓库根 kittest.toml）。

    use std::cell::RefCell;
    use std::rc::Rc;

    use egui_kittest::{Harness, kittest::Queryable};

    use super::*;
    use crate::machine::OverlayView;
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
        }
    }

    fn failed_view() -> OverlayView {
        OverlayView::Failed {
            message: "网络错误，请检查网络后重试".into(),
            action: Some(ErrorAction::Retry),
        }
    }

    fn auth_failed_view() -> OverlayView {
        OverlayView::Failed {
            message: "API Key 无效或未配置，请到设置中检查".into(),
            action: Some(ErrorAction::OpenSettings),
        }
    }

    /// 无动作出口的失败（协议异常、权限缺失）：只展示文案，没有按钮。
    fn bare_failed_view() -> OverlayView {
        OverlayView::Failed {
            message: "服务返回异常：HTTP 400 Bad Request".into(),
            action: None,
        }
    }

    /// 动作收集器：`draw` 的返回值经它带出闭包——点击测试断言的就是
    /// 「点击 → 返回动作」这条交付物契约本身，而不是按钮可点。
    type Clicked = Rc<RefCell<Option<ErrorAction>>>;

    fn harness_for(view: OverlayView) -> (Harness<'static>, Clicked) {
        let clicked: Clicked = Rc::new(RefCell::new(None));
        let sink = Rc::clone(&clicked);
        let harness = Harness::new_ui(move |ui| {
            // 只累积不覆盖：一次 run 可能驱动多帧，点击帧之后的帧返回
            // None，不能把已上交的动作冲掉。
            if let Some(action) = draw(ui, Some(&view)) {
                *sink.borrow_mut() = Some(action);
            }
        });
        (harness, clicked)
    }

    /// 词卡精排内容全部可达：词条、释义、例句与复制按钮都能在
    /// AccessKit 树中按文本定位。
    #[test]
    fn word_card_exposes_entries_to_accesskit() {
        let (mut harness, _clicked) = harness_for(word_card_view());
        harness.run();
        harness.get_by_label("gloss");
        harness.get_by_label("/ɡlɒs/");
        harness.get_by_label("光泽；注释");
        harness.get_by_label("· a gloss of silk");
        harness.get_by_label("复制");
    }

    /// 一键复制按钮可定位可点击（剪贴板内容的端到端回读属真机 L4；
    /// egui 的 copy_text 通路由 egui 自测覆盖）。
    #[test]
    fn copy_button_is_clickable() {
        let (mut harness, clicked) = harness_for(word_card_view());
        harness.run();
        harness.get_by_label("复制").click();
        harness.run();
        assert_eq!(*clicked.borrow(), None, "result cards expose no action");
    }

    /// 流式视图过滤结构化块：正文可见，\`\`\`gloss\` 围栏不出现在
    /// AccessKit 树里（围栏可能跨 chunk 切分，过滤在累积文本上进行）。
    #[test]
    fn streaming_view_hides_structured_block() {
        let (mut harness, _clicked) = harness_for(streaming_view());
        harness.run();
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

    /// 失败卡展示失败信息；可重试类带重试按钮，点击经返回值交给壳
    /// （浮层只渲染，重发任务在 app 层）。
    #[test]
    fn failed_view_shows_retry_hint() {
        let (mut harness, clicked) = harness_for(failed_view());
        harness.run();
        harness.get_by_label_contains("网络错误");
        harness.get_by_label("重试").click();
        harness.run();
        assert_eq!(
            *clicked.borrow(),
            Some(ErrorAction::Retry),
            "clicking retry must surface the action via draw's return value"
        );
    }

    /// 鉴权类失败引导去设置页：按钮可定位可点击。
    #[test]
    fn auth_failed_view_offers_open_settings() {
        let (mut harness, clicked) = harness_for(auth_failed_view());
        harness.run();
        harness.get_by_label_contains("API Key");
        harness.get_by_label("打开设置").click();
        harness.run();
        assert_eq!(
            *clicked.borrow(),
            Some(ErrorAction::OpenSettings),
            "clicking open-settings must surface the action"
        );
    }

    /// 无动作出口的失败卡没有按钮可点。
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

    /// 快照对比（wgpu 渲染 + 基线图 diff，阈值见 kittest.toml）。
    /// 多个 harness 的快照结果须合并为单个 SnapshotResults 处理。
    #[test]
    fn snapshots_match_baseline() {
        let mut results = egui_kittest::SnapshotResults::new();

        let (mut harness, _clicked) = harness_for(word_card_view());
        harness.run();
        harness.snapshot("popup_word_card");
        results.extend_harness(&mut harness);

        let (mut harness, _clicked) = harness_for(streaming_view());
        harness.run();
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

        results.unwrap();
    }
}
