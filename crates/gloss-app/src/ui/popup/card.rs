//! 卡片固定件：页头（应用图标 + 动作区 + 拖动热区）、页脚带（状态文案 +
//! 呼吸点 + 水印）、经注疏印章，以及印章/水印 galley 的跨帧缓存。
//!
//! 几何与配色常量是 demo 定稿值，随实现就近存放；字号规格与宋楷字体
//! 助手在 [`super::content`]。视图骨架（[`super::views`]）与经注疏排布
//! （[`super::outcome`]）从这里取页头、页脚与印章。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use egui::{Align, RichText, Stroke, vec2};

use super::content::{SEAL_FONT, serif_font, watermark};
use super::{FOOTER_HEIGHT, HEADER_ICON, OverlayAction, PULSE_PERIOD_SECS, RenderState};
use crate::ui::i18n::Text;
use crate::ui::style::{color, font, space, stroke};

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
/// 印章方块的边长与圆角（demo 定稿：21px、5px 圆角）；en 缩写按文本宽度
/// 撑宽，方块边长是下限。
const SEAL_SIZE: f32 = 21.0;
const SEAL_RADIUS: u8 = 5;
/// 印章字在方块内的水平余量（单侧）：单字居中即方块本身，缩写按文本撑宽。
const SEAL_TEXT_PADDING: f32 = 5.0;
/// 朱丝栏线色的不透明度档（demo：朱砂 55% 混透明）。
const ZHU_LINE_ALPHA: u8 = 140;
/// 页脚呼吸点的半径与个数
const FOOTER_DOT_RADIUS: f32 = 2.0;
const FOOTER_DOT_COUNT: usize = 3;
/// 页脚呼吸点的透明度档（0-255）：点径只有 2px，沿用骨架条的档位在浅底
/// 上几乎不可辨；整体上调让加载态一眼可辨，脉动在这个区间内保留呼吸感。
const FOOTER_DOT_ALPHA_MIN: u8 = 90;
const FOOTER_DOT_ALPHA_MAX: u8 = 180;
/// 页脚水印字串与槽缘的余量（单侧）：槽宽按实测字宽加此余量。
const WATERMARK_PADDING: f32 = 6.0;

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
pub(super) fn header(
    ui: &mut egui::Ui,
    state: &RenderState,
    text: &'static Text,
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
    text: &'static Text,
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
fn close_button(ui: &mut egui::Ui, text: &'static Text) -> egui::Response {
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
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum SealTint {
    Ink,
    Zhu,
    Wei,
}

/// 经注疏印章：小圆角方块 + 单字（en 缩写按文本撑宽，方块边长是下限）。
/// 几何与配色按 demo 定稿（21px 方块、5px 圆角、宋体字）。
pub(super) fn seal(ui: &mut egui::Ui, label: &'static str, tint: SealTint) {
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
    let galley = cached_galley(
        ui,
        CachedGalley::Seal(
            label,
            tint,
            ui.visuals().dark_mode,
            ui.ctx().pixels_per_point().to_bits(),
        ),
        || {
            ui.painter()
                .layout_no_wrap(label.to_owned(), serif_font(SEAL_FONT), fg)
        },
    );
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

/// 印章/水印 galley 的缓存槽位：键穷举**除字体表外**的全部布局输入
/// （静态文本、墨色档、明暗、像素密度），取值空间有界（三印一水印 ×
/// 文案表 × 明暗两态 × 像素密度档），不做逐出。字体表启动期会整体更换一次（内置
/// 快路径 → 完整表），布局结果随表变化——槽位键不含它，由统一装入点
/// （`ui::context`）在换表时调 [`clear_cached_text_galleys`] 整体失效。
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum CachedGalley {
    /// 印章：标签（文案表字段，`'static`）+ 墨色档 + 明暗 + 像素密度位型。
    Seal(&'static str, SealTint, bool, u32),
    /// 页脚水印（文案恒为品牌名）+ 明暗 + 像素密度位型。
    Watermark(bool, u32),
    /// 页脚生成指示文案（正在/重新注解，文案表字段）+ 明暗 + 像素密度位型。
    FooterLabel(&'static str, bool, u32),
}

/// galley 按槽位缓存：epaint 对布局本身记忆化，但每次查询都要先构造
/// `LayoutJob`（文本拷贝 + sections Vec）再算哈希——印章与水印每帧各一
/// 次的这份固定开销在这里消失，稳态帧零分配。 galley 不可变，跨帧复用
/// 渲染一致；明暗或像素密度变化换键重排，字体表变化整体失效。
#[derive(Default)]
struct CachedGalleys(HashMap<CachedGalley, Arc<egui::Galley>>);

impl egui::cache::CacheTrait for CachedGalleys {
    fn update(&mut self) {}

    fn len(&self) -> usize {
        self.0.len()
    }
}

/// 字体表更换后清空文本 galley 缓存：槽位键不含字体代际，换表前以旧表
/// （内置快路径字形）排出的 galley 必须整体失效重排，否则字形与量宽
/// 滞留到下次明暗/DPI 变化。由 `ui::context` 的统一装入点在 `set_fonts`
/// 后调用（含启动首装——空表清空是空操作）。
pub(crate) fn clear_cached_text_galleys(ctx: &egui::Context) {
    ctx.memory_mut(|mem| mem.caches.cache::<CachedGalleys>().0.clear());
}

/// 取槽位对应的 galley，未缓存时按 `layout` 现排一次。查与写分两步拿
/// Context 写锁：`layout` 里的排版要走 `fonts_mut`（同一把锁），不能在
/// `memory_mut` 闭包内重入。UI 单线程，先查后插无竞态。
fn cached_galley(
    ui: &egui::Ui,
    slot: CachedGalley,
    layout: impl FnOnce() -> Arc<egui::Galley>,
) -> Arc<egui::Galley> {
    let ctx = ui.ctx();
    let cached = ctx.memory_mut(|mem| mem.caches.cache::<CachedGalleys>().0.get(&slot).cloned());
    if let Some(galley) = cached {
        return galley;
    }
    let galley = layout();
    ctx.memory_mut(|mem| {
        mem.caches
            .cache::<CachedGalleys>()
            .0
            .insert(slot, Arc::clone(&galley));
    });
    galley
}

/// 朱丝栏的线色：朱砂按 demo 的 55% 透明档。
pub(super) fn zhu_line_color(ui: &egui::Ui) -> egui::Color32 {
    let zhu = zhu_color(ui);
    egui::Color32::from_rgba_unmultiplied(zhu.r(), zhu.g(), zhu.b(), ZHU_LINE_ALPHA)
}

/// 页脚带（常驻）：上缘细线，推理中左端是「正在注解」/「重新注解」
/// 文案 + 呼吸点（修复轮切换文案，生成指示唯一落点，无障碍标签也在
/// 这里），右端恒为 Gloss 水印。动画与骨架同源（时间驱动 alpha + 短重绘），
/// 仅在推理中请求。
pub(super) fn footer(ui: &mut egui::Ui, streaming: bool, repairing: bool, text: &'static Text) {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), FOOTER_HEIGHT),
        egui::Sense::hover(),
    );
    ui.painter().line_segment(
        [
            egui::pos2(rect.min.x, rect.min.y),
            egui::pos2(rect.max.x, rect.min.y),
        ],
        hairline(ui),
    );
    if streaming {
        let label_width = footer_status(ui, &rect, &response, repairing, text);
        footer_dots(ui, &rect, label_width);
    }
    footer_watermark(ui, &rect);
}

/// 页脚状态文案（推理中）：「正在注解」/「重新注解」的缓存 galley 落在
/// 带内左缘垂直居中，返回文案宽供呼吸点接排。
fn footer_status(
    ui: &egui::Ui,
    rect: &egui::Rect,
    response: &egui::Response,
    repairing: bool,
    text: &'static Text,
) -> f32 {
    let label = if repairing {
        &text.gloss_popup_reannotating
    } else {
        &text.gloss_popup_annotating
    };
    let dim = ui.visuals().weak_text_color();
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::ProgressIndicator, true, label.as_str())
    });
    let galley = cached_galley(
        ui,
        CachedGalley::FooterLabel(
            label.as_str(),
            ui.visuals().dark_mode,
            ui.ctx().pixels_per_point().to_bits(),
        ),
        || {
            egui::WidgetText::from(RichText::new(label.as_str()).size(font::TAG).color(dim))
                .into_galley(
                    ui,
                    Some(ui.wrap_mode()),
                    ui.available_width(),
                    egui::FontSelection::Default,
                )
        },
    );
    ui.painter().galley(
        egui::pos2(
            rect.min.x,
            rect.center().y + stroke::CARD - galley.size().y / 2.0,
        ),
        galley.clone(),
        dim,
    );
    galley.size().x
}

/// 页脚呼吸点（推理中）：朱砂圆点的相位波脉动，紧跟状态文案排布；动画
/// 期间请求短重绘推进。
fn footer_dots(ui: &egui::Ui, rect: &egui::Rect, label_width: f32) {
    let now = ui.input(|i| i.time);
    let zhu = zhu_color(ui);
    let mut dot_center = egui::pos2(
        rect.min.x + label_width + space::TIGHT + FOOTER_DOT_RADIUS,
        rect.center().y + stroke::CARD,
    );
    for index in 0..FOOTER_DOT_COUNT {
        let wave = ((now / PULSE_PERIOD_SECS + index as f64 * 0.9).sin() + 1.0) / 2.0;
        let alpha = (FOOTER_DOT_ALPHA_MIN as f64
            + wave * f64::from(FOOTER_DOT_ALPHA_MAX - FOOTER_DOT_ALPHA_MIN))
            as f32;
        let alpha = alpha.round().clamp(0.0, 255.0) as u8;
        ui.painter().circle_filled(
            dot_center,
            FOOTER_DOT_RADIUS,
            egui::Color32::from_rgba_unmultiplied(zhu.r(), zhu.g(), zhu.b(), alpha),
        );
        dot_center.x += FOOTER_DOT_RADIUS * 3.0;
    }
    ui.ctx()
        .request_repaint_after(Duration::from_secs_f32(0.016));
}

/// 页脚水印（常驻右端）。
fn footer_watermark(ui: &mut egui::Ui, rect: &egui::Rect) {
    let dim = ui.visuals().weak_text_color();
    // 水印走 Label 而不是 painter 文字：进无障碍树，可检索、可测。槽宽按
    // 水印字串实测宽 + 余量（与印章同一模式），右对齐贴页脚右缘。标签
    // 直接吃缓存槽里的同一份 galley，不再逐帧重排版、不逐帧复制字串。
    // 缓存未命中时的排版走与 `Label` 同源的 `WidgetText::into_galley`
    // （valign/字体/style 解析一致）；job 的 halign/justify 维持默认
    // （LEFT/不两端对齐），落点由 `ui.put` 的居中与槽宽（=galley 宽 +
    // 对称余量）决定——渲染一致性由 L2 快照钉住，不靠此处机制断言。
    let galley = cached_galley(
        ui,
        CachedGalley::Watermark(
            ui.visuals().dark_mode,
            ui.ctx().pixels_per_point().to_bits(),
        ),
        || {
            egui::WidgetText::from(RichText::new(watermark()).size(font::TAG).color(dim))
                .into_galley(
                    ui,
                    Some(ui.wrap_mode()),
                    ui.available_width(),
                    egui::FontSelection::Default,
                )
        },
    );
    let slot = galley.size().x + 2.0 * WATERMARK_PADDING;
    ui.put(
        egui::Rect::from_min_max(
            egui::pos2(rect.max.x - slot, rect.min.y),
            egui::pos2(rect.max.x, rect.max.y),
        ),
        egui::Label::new(egui::WidgetText::Galley(galley))
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
