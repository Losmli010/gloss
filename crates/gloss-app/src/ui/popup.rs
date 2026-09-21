//! 浮层内容：按 [`TaskKind`] 分发的结果卡与流式/失败视图。
//!
//! 词卡精排（音标/词性/释义/例句），其余任务展示 markdown 正文
//! （egui_commonmark 渲染；OCR 提取文本保持纯文本，不按 markdown 解释）。
//! 划选即复制：全部文本可选中，无独立复制按钮。正文完整渲染不截断，
//! 高度自适应内容（宽度默认 380、上限 480，高度上限按屏幕），超出部分
//! 滚动兜底。流式视图按 [`STRUCTURED_FENCE`] 过滤已完整出现的结构化块
//! （在累积文本上按最后围栏标记截断）；跨 chunk 切分出的残缺围栏前缀
//! 可能短暂显示，随下一 chunk 自愈。

use std::cell::{Cell, RefCell};
use std::time::Duration;

use egui::{Color32, CornerRadius, Frame, Margin, RichText, ScrollArea, Stroke, vec2};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use gloss_core::prompt::STRUCTURED_FENCE;
use gloss_core::task::OutcomeStructured;

use crate::machine::{ErrorAction, OverlayView};

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
/// 卡片圆角
const CORNER_RADIUS: u8 = 8;
/// 内容区内边距
const PADDING: i8 = 14;
/// 框线宽度（计入期望尺寸）
const STROKE_WIDTH: f32 = 0.5;
/// 头部身份圆点：品牌珊瑚橙
const BRAND_DOT: Color32 = Color32::from_rgb(0xD8, 0x5A, 0x30);
/// 字号阶梯：正文 / 流式来源与失败提示 / 标题
const BODY_SIZE: f32 = 14.0;
const NOTICE_SIZE: f32 = 13.0;
const TITLE_SIZE: f32 = 15.0;
/// 字号阶梯：词条 / 音标 / 释义 / 词性 / 例句
const WORD_SIZE: f32 = 18.0;
const PHONETIC_SIZE: f32 = 13.0;
const MEANING_SIZE: f32 = 14.0;
const POS_SIZE: f32 = 12.0;
const EXAMPLE_SIZE: f32 = 12.0;
/// 头部字号：品牌标签 / 任务类型 / 动作按钮
const HEADER_SIZE: f32 = 12.0;
const TAG_SIZE: f32 = 11.0;
const BUTTON_SIZE: f32 = 12.0;
/// 出现动画时长（淡入，秒）：显示/重显后的第一帧从 0 渐进到 1。
const APPEAR_SECONDS: f32 = 0.18;
/// 间距阶梯：标题行后 / 正文分区前 / 词条行后 / 释义组间 / 例句间
const SECTION_SPACE: f32 = 12.0;
const BODY_SPACE: f32 = 8.0;
const WORD_ROW_SPACE: f32 = 10.0;
const SENSE_SPACE: f32 = 6.0;
const EXAMPLE_SPACE: f32 = 2.0;

/// 浮层的跨帧渲染状态（每窗口一份，由渲染管线持有）。
pub(crate) struct RenderState {
    /// markdown 渲染状态（egui_commonmark 要求跨帧持有）。
    pub cache: RefCell<CommonMarkCache>,
    /// 上一帧应用的浮层宽度（宽度收敛的滞回状态）。
    pub last_width: Cell<f32>,
}

impl Default for RenderState {
    fn default() -> Self {
        Self {
            cache: RefCell::new(CommonMarkCache::default()),
            last_width: Cell::new(WIDTH),
        }
    }
}

/// 一帧浮层绘制的产物：失败卡动作上交 + 内容期望的窗口尺寸（逻辑点）。
pub(crate) struct PopupOutput {
    pub action: Option<ErrorAction>,
    pub sizing: OverlaySizing,
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

/// 清除出现动画起点：壳在隐藏与每次显示时调用，让下一帧重新从 0 淡入
/// （隐藏期无帧运行，不清除则旧起点让重显直接落在完成态）。
pub(crate) fn reset_appear_animation(ctx: &egui::Context) {
    ctx.memory_mut(|mem| mem.data.remove_temp::<f64>(appear_t0_id()));
}

/// 画一帧浮层。根 `Ui` 覆盖整个窗口，卡片铺满它，圆角之外由透明窗口露出桌面。
///
/// `view` 为 `None` 时显示渲染自检卡（预热与自检路径）。返回本帧绘制的
/// 产物——失败卡动作按钮（重试/打开设置）由壳执行——浮层只渲染、不副作
/// 用；期望尺寸由壳经窗口管理器应用（内容自适应高度，超出屏幕滚动兜底）。
pub(crate) fn draw(
    ui: &mut egui::Ui,
    view: Option<&OverlayView>,
    state: &RenderState,
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
    let stroke = ui.visuals().window_stroke;
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
        .stroke(Stroke::new(STROKE_WIDTH, stroke.color))
        .corner_radius(CornerRadius::same(CORNER_RADIUS))
        .inner_margin(Margin::same(PADDING))
        .show(ui, |ui| {
            output.action = render_content(ui, view, state, &mut content_h);
            ui.set_min_size(ui.available_size());
        });

    // 宽度按内容体量收敛：矮内容保持默认宽，长文本加宽到上限，滞回带
    // 防两档间来回切换；高度取完整内容高（超出屏幕由壳侧钳制、滚动兜底）。
    let width = resolve_width(state.last_width.get(), content_h);
    state.last_width.set(width);
    output.sizing = OverlaySizing {
        width,
        height: (content_h + 2.0 * PADDING as f32 + STROKE_WIDTH).round(),
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
/// ScrollArea 报告的内容尺寸，不受视口裁剪影响。
fn render_content(
    ui: &mut egui::Ui,
    view: Option<&OverlayView>,
    state: &RenderState,
    content_h: &mut f32,
) -> Option<ErrorAction> {
    match view {
        None => {
            header(ui, Some("自检"), false);
            ui.add_space(SECTION_SPACE);
            selfcheck_body(ui);
            *content_h = ui.min_rect().height();
            None
        }
        Some(OverlayView::Streaming { source, body }) => {
            header(ui, None, true);
            ui.add_space(BODY_SPACE);
            ui.label(
                RichText::new(source)
                    .size(NOTICE_SIZE)
                    .color(ui.visuals().weak_text_color()),
            );
            ui.add_space(BODY_SPACE);
            let visible = match body.rfind(STRUCTURED_FENCE) {
                Some(pos) => &body[..pos],
                None => body,
            };
            // ScrollArea 内容起点 = cursor（egui 的 cursor 停在前序内容底边
            // 加一个 item_spacing 处），从这里起算正文完整高。
            let body_top = ui.cursor().min.y;
            let scrolled = ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                render_markdown(ui, state, visible);
            });
            *content_h = body_top + scrolled.content_size.y;
            None
        }
        Some(OverlayView::Outcome(outcome)) => {
            header(ui, Some(crate::ui::kind_label(outcome.kind)), false);
            ui.add_space(SECTION_SPACE);
            let body_top = ui.cursor().min.y;
            let scrolled = ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                outcome_body(ui, outcome, state);
            });
            *content_h = body_top + scrolled.content_size.y;
            None
        }
        Some(OverlayView::Failed { message, action }) => {
            header(ui, Some("失败"), false);
            ui.add_space(SECTION_SPACE);
            ui.label(
                RichText::new(message.as_str())
                    .size(NOTICE_SIZE)
                    .color(ui.visuals().warn_fg_color),
            );
            if let Some(action) = *action {
                ui.add_space(SECTION_SPACE);
                if ui
                    .button(
                        RichText::new(action_label(action))
                            .size(BUTTON_SIZE)
                            .color(ui.visuals().strong_text_color()),
                    )
                    .clicked()
                {
                    return Some(action);
                }
            }
            *content_h = ui.min_rect().height();
            None
        }
    }
}

/// 动作按钮的文案。
fn action_label(action: ErrorAction) -> &'static str {
    match action {
        ErrorAction::Retry => "重试",
        ErrorAction::OpenSettings => "打开设置",
    }
}

/// 头部：身份圆点 + 品牌标签；`busy` 时右侧画旋转指示器替代任务标签
/// （推理中的流式反馈），否则显示任务类型标签。
fn header(ui: &mut egui::Ui, tag: Option<&str>, busy: bool) {
    let weak = ui.visuals().weak_text_color();
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 4.0, BRAND_DOT);
        ui.label(RichText::new("翻译").size(HEADER_SIZE).color(weak));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if busy {
                ui.add(egui::Spinner::new().size(TAG_SIZE + 5.0));
            } else if let Some(tag) = tag {
                ui.label(RichText::new(tag).size(TAG_SIZE).color(weak));
            }
        });
    });
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
                        .size(TITLE_SIZE)
                        .strong()
                        .color(ui.visuals().strong_text_color()),
                );
                ui.add_space(BODY_SPACE);
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
                .size(WORD_SIZE)
                .strong()
                .color(ui.visuals().strong_text_color()),
        );
        if let Some(phonetic) = phonetic {
            ui.label(
                RichText::new(phonetic)
                    .size(PHONETIC_SIZE)
                    .color(ui.visuals().weak_text_color()),
            );
        }
    });
    ui.add_space(WORD_ROW_SPACE);
    let strong = ui.visuals().strong_text_color();
    let weak = ui.visuals().weak_text_color();
    for sense in senses {
        ui.horizontal_wrapped(|ui| {
            if let Some(pos) = &sense.pos {
                ui.label(RichText::new(pos.as_str()).size(POS_SIZE).color(weak));
            }
            ui.label(
                RichText::new(sense.meaning.as_str())
                    .size(MEANING_SIZE)
                    .color(strong),
            );
        });
        for example in &sense.examples {
            ui.add_space(EXAMPLE_SPACE);
            ui.indent("example", |ui| {
                ui.label(
                    RichText::new(format!("· {example}"))
                        .size(EXAMPLE_SIZE)
                        .color(weak),
                );
            });
        }
        ui.add_space(SENSE_SPACE);
    }
}

/// 可选中、自动换行的纯文本正文（OCR 提取文本不按 markdown 解释）。
fn plain_body(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .size(BODY_SIZE)
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
            .size(NOTICE_SIZE)
            .color(weak),
    );
    ui.add_space(SECTION_SPACE);
    ui.label(
        RichText::new("敏捷的棕色狐狸从懒狗身上跳过。")
            .size(TITLE_SIZE)
            .strong()
            .color(strong),
    );
    ui.add_space(SECTION_SPACE);
    ui.label(
        RichText::new("中文渲染自检：划词翻译、代码解释、图片识别。")
            .size(NOTICE_SIZE)
            .color(weak),
    );
}

#[cfg(test)]
mod tests {
    use super::{MAX_WIDTH, WIDTH, resolve_width};

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
}

#[cfg(test)]
mod kittest_tests {

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

    fn bare_failed_view() -> OverlayView {
        OverlayView::Failed {
            message: "服务返回异常：HTTP 400 Bad Request".into(),
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

    type Clicked = Rc<RefCell<Option<ErrorAction>>>;

    fn harness_for(view: OverlayView) -> (Harness<'static>, Clicked) {
        let clicked: Clicked = Rc::new(RefCell::new(None));
        let sink = Rc::clone(&clicked);
        let state = RenderState::default();
        let harness = Harness::new_ui(move |ui| {
            let output = draw(ui, Some(&view), &state);
            if let Some(action) = output.action {
                *sink.borrow_mut() = Some(action);
            }
        });
        (harness, clicked)
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
            Some(ErrorAction::Retry),
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
            Some(ErrorAction::OpenSettings),
            "clicking open-settings must surface the action"
        );
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

        let (mut harness, _clicked) = harness_for(streaming_view());
        // Spinner 每帧请求重绘，run() 会在 max_steps 处报错；固定步数让
        // 指示器角度确定，快照不抖。
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

        results.unwrap();
    }
}
