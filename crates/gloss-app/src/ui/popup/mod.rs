//! 浮层内容：按视图分发的骨架、经注疏结果卡与流式/失败视图。
//!
//! 内容按「经 · 注 · 疏」三层组织，视觉以设计定稿的浮层稿为准：
//! - **经**＝选区原文/词条/提取文本（宋体；代码任务为单层代码面板
//!   ——专属底色、`gloss-mono` 等宽体、按可用宽折行、左上角语言标签行）；
//! - **注**＝译文/释义/概要（楷体，系统缺楷体时随 demo 回退链落宋体；
//!   朱丝栏左线 + 朱印）；
//! - **疏**＝译注/例句/小记（楷体 markdown 正文色、上缘虚线 + 疏印）。
//!
//! 三分区的正文列都显式声明垂直布局：egui 的 Frame/子 Ui 会继承父级的
//! 水平布局（`new_child` 不带 layout 参数），横排父级里多条目正文会从左往
//! 右流（释义并排、溢出右缘）。分区的间距是弹窗私有量（随
//! [`outcome`]），字号规格与宋楷字体助手在 [`content`]（都是 demo 基准
//! 的应用定稿值），不进 style 阶梯。
//!
//! 决策纯函数不进绘制模板，按职责拆在子模块：[`note`]（流式 JSON 渐进
//! 提取）、[`icon`]（图标矢量栅格化）、[`content`]（文案整形）；绘制模板
//! 按职责拆在 [`card`]（页头/页脚/印章等卡片固定件）、[`views`]（视图
//! 分发与卡骨架）、[`outcome`]（经注疏正文排布），依赖方向恒为视图 →
//! 排布 → 固定件 → 决策；本模块留面板编排、跨帧状态与各文件共享的
//! 常量。
//!
//! 词卡精排（词条/音标落经位，释义逐行落注位，例句落疏位），其余任务正文
//! 走 markdown（egui_commonmark 渲染；注区统一改写文本样式为楷体字号）。
//! 划选即复制：全部文本可选中，无独立复制按钮。正文完整渲染不截断，宽度
//! 固定 [`WIDTH`]（不随内容变化），高度自适应内容（上限按屏幕），超出部分
//! 滚动兜底。
//! 流式视图对累积的原始流做**转义感知**的 `note`（注）渐进提取（现行输出
//! 契约是纯 JSON 对象，见 `gloss_core::schema`）：`note` 键未到齐时正文区
//! 落骨架、页脚保留「正在注解」；`structured.examples` 随流下钻渐进出。
//! 降级产物（`outcome.degraded`，契约校验失败）不出注疏——降级卡渲染
//! 「输出异常」与重试按钮，不渲染 note 内容（原文垃圾不展示不留存）。
//! 注文一律换行排版：代码面板等宽折行、markdown 由 egui_commonmark 按
//! 可用宽折行，横滚不进弹窗。
//!
//! 页头回归品牌：只有应用图标与动作区（⚙/×），任务与状态由内容层自明，
//! 页头不带任何标签药丸；行下发丝线与页脚上缘线呼应成卡片的上下界。
//! 页头行同时是拖动热区：按住拖动即移动浮层（壳侧逐帧平移窗口），热区
//! 与动作钮零重叠，按钮点击不受影响。
//! 页脚常驻
//! 一条窄带（带高取 [`FOOTER_HEIGHT`]）：推理中左端是「正在注解」+ 呼吸点（生成指示唯一落点），右端
//! 恒为 Gloss 水印（品牌名不翻译，与窗口标题同一原则）；滚动区按页脚带宽
//! 预留视口，页脚不被内容挤出窗外。「经显注未至」时正文区是纯骨架
//! （脉动条），无占位文字。动作点击经 draw 返回 [`OverlayAction`] 上交壳执行；
//! 拖动热区的指针位移经 [`PopupOutput::drag`] 上交壳换算。
//!
//! 敏感信息防护不在这里：两条闸门都不出浮层（见 `gloss_app::machine`），
//! 因此也没有「疑似敏感」这张卡。

mod card;
mod content;
mod icon;
mod image;
mod note;
mod outcome;
mod views;

pub(crate) use card::clear_cached_text_galleys;

use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::time::Duration;

use egui::{CornerRadius, Frame, Margin, Stroke};
use egui_commonmark::CommonMarkCache;
use icon::app_icon_image;
use image::decode_attached;

use super::style::{radius, space, stroke};
use crate::machine::{ErrorAction, OverlayView};
use crate::ui::i18n::Text;

/// 浮层宽度（逻辑点）：窗口创建与每帧期望尺寸共用此值，不随内容变化。
pub const WIDTH: f32 = 480.0;
/// 页头应用图标的边长（栅格化与页头行布局共用）。
pub(super) const HEADER_ICON: f32 = 20.0;
/// 页脚带高：水印字形（`font::TAG` 11px）加合理上下余量的下限档。
pub(super) const FOOTER_HEIGHT: f32 = 20.0;
/// 骨架条与页脚呼吸点的脉动周期（秒）
pub(super) const PULSE_PERIOD_SECS: f64 = 1.2;
/// 出现动画时长（淡入，秒）：显示/重显后的第一帧从 0 渐进到 1。
const APPEAR_SECONDS: f32 = 0.18;

/// 一帧内容分发的共享绘制输入：状态、文案表与本帧经位附件三件套随
/// 视图函数整组同行，捆一个结构免得逐函数穿参。
struct RenderCtx<'a> {
    state: &'a RenderState,
    text: &'static Text,
    attached: Option<&'a Arc<[u8]>>,
}

/// 浮层的跨帧渲染状态（每窗口一份，由渲染管线持有）。
pub struct RenderState {
    /// markdown 渲染状态（egui_commonmark 要求跨帧持有）。
    pub cache: RefCell<CommonMarkCache>,
    /// 页头拖动的按压点（**物理像素**，按压帧的窗口内逻辑坐标乘当下
    /// `pixels_per_point`）：拖动开始帧记录，结束即清。存物理口径是为
    /// 跨 DPI 显示器的拖动——逻辑坐标随新显示器的缩放比例重标定，逻辑
    /// 按压点会混尺；物理位移按当下比例还原成当下尺度的逻辑位移，与壳
    /// 侧落点换算（`apply_overlay_drag`）同口径。清零后 egui 侧若仍有
    /// 残留拖动所有权（见 [`card::drag_strip`]），热区保持惰性。
    drag_press_phys: Cell<Option<egui::Pos2>>,
    /// 页头应用图标的纹理缓存：键是栅格化边长（物理像素）。None＝尚未
    /// 栅格化；内层 `None`＝该边长下渲染失败（同样只试一次，占位降级）。
    /// 换显示器（`pixels_per_point` 变化）即按新密度重栅。
    icon: RefCell<Option<(u32, Option<egui::TextureHandle>)>>,
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

    /// 页头应用图标纹理：矢量源按当前 `pixels_per_point` 栅格化到物理
    /// 像素级（纹理纹元 1:1 对应设备像素，图标锐利），装完跨帧复用；
    /// 密度变化（跨显示器拖动）按新边长重栅，失败按边长缓存不重试。
    fn icon_texture(&self, ctx: &egui::Context) -> Option<egui::TextureHandle> {
        let side_px = (HEADER_ICON * ctx.pixels_per_point()).round().max(1.0) as u32;
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
        // TextureHandle 内部是 Arc：克隆只递引用计数（零堆分配），缓存槽
        // 与当帧绘制各持一枚，勿改成逐帧重装纹理。
        slot.as_ref().and_then(|(_, texture)| texture.clone())
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

/// 本帧期望的浮层高（逻辑点，含内边距与框线）：宽度恒为 [`WIDTH`]、
/// 不随内容变化（窗口创建与每帧期望宽同源此常量，不经此结构传递），
/// 只有高度随内容。
#[derive(Clone, Copy, Debug)]
pub struct OverlaySizing {
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

/// 出现淡入：起点在每次显示（壳侧清除后）的首帧重新写入；进度未满时
/// 请求短重绘推进动画。kittest 的 animation_time=0 与固定步进让快照
/// 恒为完成态，不受动画影响。
fn appear_fade_in(ui: &mut egui::Ui) {
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
    text: &'static Text,
) -> PopupOutput {
    // 划选即复制路径：全部文本可选中（含跨 widget 连选）
    ui.style_mut().interaction.selectable_labels = true;
    appear_fade_in(ui);

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
        sizing: OverlaySizing { height: 0.0 },
        drag: None,
    };
    let mut content_h = 0.0;
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(stroke::CARD, window_stroke.color))
        .corner_radius(CornerRadius::same(radius::CARD))
        .inner_margin(Margin::same(space::CARD_PADDING))
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

    // 宽度固定不随内容变化；高度取完整内容高（超出屏幕由壳侧钳制、滚动
    // 兜底）。
    output.sizing = OverlaySizing {
        height: (content_h + 2.0 * space::CARD_PADDING as f32 + stroke::CARD).round(),
    };
    output
}

#[cfg(test)]
mod kittest_tests {

    use std::cell::RefCell;
    use std::rc::Rc;

    use egui::vec2;
    use egui_kittest::{Harness, kittest::NodeT, kittest::Queryable};

    use super::*;

    use crate::machine::{FailureCause, OverlayView};
    use gloss_core::model::{GlossError, Locale};
    use gloss_core::task::{OutcomeStructured, TaskKind, TaskOutcome};

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

    type Clicked = Rc<RefCell<Option<OverlayAction>>>;

    fn font_first_frame(installed: &Cell<bool>, ctx: &egui::Context) -> bool {
        if installed.replace(true) {
            return false;
        }
        crate::ui::context::install_kittest_fonts(ctx);
        crate::ui::context::apply_selection_colors(ctx);
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
        // 疏条目经 markdown 渲染：段落文本是 TextRun 节点，精确标签查询会
        // 被标签提供者过滤规则吞掉——沿用注文的 contains 惯例。
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
    fn a_repairing_streaming_view_switches_the_footer_wording() {
        let (mut harness, _clicked) = harness_for(repairing_streaming_view());
        harness.run_steps(3);
        harness.get_by_label_contains("重新注解");
        assert!(
            harness
                .query_all_by_label_contains("正在注解")
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
            for label in ["正在注解", "Gloss"] {
                let node = harness.get_by_label_contains(label).rect();
                assert!(
                    node.bottom() <= capped_height + 0.5,
                    "长选区不得把页脚挤出窗外（壳侧钳高后生成指示仍须可见）: \
                     {label} bottom={} window_height={capped_height}",
                    node.bottom()
                );
            }

            let seal_before = harness.get_by_label("经").rect();
            let footer_before = harness.get_by_label_contains("正在注解").rect();
            harness.key_press(egui::Key::ArrowDown);
            harness.run_steps(8);
            let seal_after = harness.get_by_label("经").rect();
            let footer_after = harness.get_by_label_contains("正在注解").rect();
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
            repairing: false,
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

    enum Run {
        Settle,
        Steps(usize),
    }

    fn snapshot_case(
        results: &mut egui_kittest::SnapshotResults,
        name: &str,
        view: Option<OverlayView>,
        attached: Option<Arc<[u8]>>,
        run: Run,
    ) {
        let state = RenderState::default();
        let text = Text::get(Locale::En);
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

    #[test]
    fn snapshots_match_baseline() {
        let mut results = egui_kittest::SnapshotResults::new();
        for (name, view, run) in [
            ("popup_word_card", Some(word_card_view_en()), Run::Settle),
            ("popup_streaming", Some(streaming_view_en()), Run::Steps(3)),
            ("popup_failed", Some(failed_view()), Run::Settle),
            ("popup_failed_auth", Some(auth_failed_view()), Run::Settle),
            (
                "popup_code_streaming",
                Some(code_streaming_view_en()),
                Run::Steps(3),
            ),
            (
                "popup_code_outcome",
                Some(code_outcome_view_en()),
                Run::Settle,
            ),
            (
                "popup_image_failed",
                Some(image_too_large_view()),
                Run::Settle,
            ),
            (
                "popup_degraded_outcome",
                Some(degraded_outcome_view_en()),
                Run::Settle,
            ),
            ("popup_selfcheck", None, Run::Settle),
            ("popup_long_line", Some(long_line_view()), Run::Settle),
        ] {
            snapshot_case(&mut results, name, view, None, run);
        }
        snapshot_case(
            &mut results,
            "popup_image_outcome",
            Some(image_commentary_view_en()),
            Some(fixture_png()),
            Run::Settle,
        );
        snapshot_case(
            &mut results,
            "popup_image_streaming",
            Some(image_streaming_view_en()),
            Some(fixture_png()),
            Run::Steps(3),
        );
        results.unwrap();
    }
}
