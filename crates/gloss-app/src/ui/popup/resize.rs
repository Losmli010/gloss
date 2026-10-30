//! 八向缩放：卡片边界的热区注册与缩放意图上交，期望浮层高随帧产出。
//!
//! 卡片四角与上下左右边界是缩放热区（不画任何握把示意）：按住哪条边，
//! 对侧边即被壳侧锚定——拖动帧上交边界组合与指针位置，期望宽高与
//! 窗口位移一并由壳侧钳制落窗（西、北向拖动窗口随指针平移）；拖过
//! 一次即本段会话接管浮层宽高，内容高度自适应退位，显隐重置后回到
//! 默认尺寸。
//!
//! 界面侧只认窗口内坐标：按下帧上交边界组合、按下时刻窗口尺寸与指针
//! 位置，拖动帧上交边界组合与窗口内指针位置，松开后粘性上交沉降帧；
//! 全局坐标、锚定与钳制全在壳侧。边界组合是数据（四布尔）不是枚举：
//! 八段热区两两组合直接落位，壳侧锚定求解按位展开，新增组合不再加
//! 变体。

use super::RenderState;
use crate::ui::i18n::Text;
use crate::ui::style::geometry;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OverlayResize {
    Press {
        edges: ResizeEdges,
        window: egui::Vec2,
        pointer: egui::Pos2,
    },
    Drag {
        edges: ResizeEdges,
        pointer: egui::Pos2,
    },
    Settle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResizeEdges {
    pub north: bool,
    pub south: bool,
    pub east: bool,
    pub west: bool,
}

impl ResizeEdges {
    const NONE: Self = Self {
        north: false,
        south: false,
        east: false,
        west: false,
    };

    const fn with_north(mut self) -> Self {
        self.north = true;
        self
    }

    const fn with_south(mut self) -> Self {
        self.south = true;
        self
    }

    const fn with_east(mut self) -> Self {
        self.east = true;
        self
    }

    const fn with_west(mut self) -> Self {
        self.west = true;
        self
    }
}

#[derive(Clone, Copy, Debug)]
pub struct OverlaySizing {
    pub height: f32,
}

type ResizeZone = (&'static str, egui::Rect, egui::CursorIcon, ResizeEdges);

#[allow(
    clippy::too_many_lines,
    reason = "八段热区是一张声明式数据表（名字、矩形、光标、边界组合各一列），拆分只会把表撕成两半"
)]
fn resize_zones(card: egui::Rect) -> [ResizeZone; 8] {
    let hot = geometry::resize::HOTSPOT;
    let corner = geometry::resize::CORNER;
    let (l, t, r, b) = (card.left(), card.top(), card.right(), card.bottom());
    let edges = ResizeEdges::NONE;
    [
        (
            "overlay_resize_north",
            egui::Rect::from_min_max(egui::pos2(l + corner, t), egui::pos2(r - corner, t + hot)),
            egui::CursorIcon::ResizeNorth,
            edges.with_north(),
        ),
        (
            "overlay_resize_south",
            egui::Rect::from_min_max(egui::pos2(l + corner, b - hot), egui::pos2(r - corner, b)),
            egui::CursorIcon::ResizeSouth,
            edges.with_south(),
        ),
        (
            "overlay_resize_east",
            egui::Rect::from_min_max(egui::pos2(r - hot, t + corner), egui::pos2(r, b - corner)),
            egui::CursorIcon::ResizeEast,
            edges.with_east(),
        ),
        (
            "overlay_resize_west",
            egui::Rect::from_min_max(egui::pos2(l, t + corner), egui::pos2(l + hot, b - corner)),
            egui::CursorIcon::ResizeWest,
            edges.with_west(),
        ),
        (
            "overlay_resize_north_west",
            egui::Rect::from_min_max(egui::pos2(l, t), egui::pos2(l + corner, t + corner)),
            egui::CursorIcon::ResizeNorthWest,
            edges.with_north().with_west(),
        ),
        (
            "overlay_resize_north_east",
            egui::Rect::from_min_max(egui::pos2(r - corner, t), egui::pos2(r, t + corner)),
            egui::CursorIcon::ResizeNorthEast,
            edges.with_north().with_east(),
        ),
        (
            "overlay_resize_south_west",
            egui::Rect::from_min_max(egui::pos2(l, b - corner), egui::pos2(l + corner, b)),
            egui::CursorIcon::ResizeSouthWest,
            edges.with_south().with_west(),
        ),
        (
            "overlay_resize_south_east",
            egui::Rect::from_min_max(egui::pos2(r - corner, b - corner), egui::pos2(r, b)),
            egui::CursorIcon::ResizeSouthEast,
            edges.with_south().with_east(),
        ),
    ]
}

pub(super) fn resize_handle(
    ui: &mut egui::Ui,
    state: &RenderState,
    text: &'static Text,
) -> Option<OverlayResize> {
    let card = ui.min_rect();
    let (pointer, window) = ui.input(|i| (i.pointer.interact_pos(), i.viewport_rect().size()));
    let mut frame: Option<OverlayResize> = None;
    for (id, rect, icon, edges) in resize_zones(card) {
        let response = ui.interact(rect, egui::Id::new(id), egui::Sense::drag());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Other,
                true,
                text.gloss_popup_resize_label.as_str(),
            )
        });
        let response = response.on_hover_cursor(icon);
        if response.drag_started()
            && let Some(pointer) = pointer
        {
            state.resizing.set(true);
            frame = Some(OverlayResize::Press {
                edges,
                window,
                pointer,
            });
        } else if response.dragged()
            && let Some(pointer) = pointer
        {
            frame = Some(OverlayResize::Drag { edges, pointer });
        }
    }
    if frame.is_none() && state.resizing.get() {
        frame = Some(OverlayResize::Settle);
    }
    frame
}

#[cfg(test)]
mod kittest_tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use egui::vec2;
    use egui_kittest::{Harness, kittest::Queryable};

    use super::super::{RenderState, draw};
    use super::*;
    use crate::ui::style::geometry::window::OVERLAY_WIDTH;

    use crate::machine::OverlayView;
    use crate::ui::i18n::Text;
    use gloss_core::model::Locale;
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
                lang_violations: Vec::new(),
            },
        }
    }

    fn font_first_frame(installed: &Cell<bool>, ctx: &egui::Context) -> bool {
        if installed.replace(true) {
            return false;
        }
        crate::ui::context::install_kittest_fonts(ctx);
        crate::ui::context::apply_visuals(ctx);
        ctx.request_repaint();
        true
    }

    const SOUTH_EAST: ResizeEdges = ResizeEdges {
        north: false,
        south: true,
        east: true,
        west: false,
    };

    type ResizeLog = Rc<RefCell<Vec<OverlayResize>>>;

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
            let output = draw(ui, Some(&view), None, &state, text, Locale::Zh);
            if let Some(resize) = output.resize {
                sink.borrow_mut().push(resize);
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
        assert_eq!(zones.len(), 8, "四角与上下左右边界共八段热区");
    }

    #[test]
    fn corner_drag_anchors_the_press_and_reports_pointer_offsets() {
        let (mut harness, resizes) = resize_harness(word_card_view());
        harness.set_size(vec2(OVERLAY_WIDTH as f32, 400.0));
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
            resizes.borrow().first(),
            Some(&OverlayResize::Press {
                edges: ResizeEdges {
                    north: false,
                    south: true,
                    east: true,
                    west: false,
                },
                window: vec2(OVERLAY_WIDTH as f32, 400.0),
                pointer: corner.center(),
            }),
            "按下帧上交边界组合、按下时刻窗口尺寸与指针位置"
        );
        harness.hover_at(corner.center() + vec2(60.0, 40.0));
        harness.run();
        harness.hover_at(corner.center() + vec2(100.0, 80.0));
        harness.run();
        assert_eq!(
            resizes.borrow().last(),
            Some(&OverlayResize::Drag {
                edges: SOUTH_EAST,
                pointer: corner.center() + vec2(100.0, 80.0),
            }),
            "拖动帧上交边界组合与窗口内指针位置"
        );

        harness.drop_at(corner.center() + vec2(100.0, 80.0));
        harness.run();
        harness.hover_at(corner.center() + vec2(140.0, 100.0));
        harness.run();
        assert_eq!(
            resizes.borrow().last(),
            Some(&OverlayResize::Settle),
            "松开后粘性上交 Settle，会话内壳侧维持用户尺寸"
        );
    }

    #[test]
    fn west_edge_drag_reports_the_west_edge_only() {
        let (mut harness, resizes) = resize_harness(word_card_view());
        harness.set_size(vec2(OVERLAY_WIDTH as f32, 400.0));
        harness.run();
        let west = harness
            .query_all_by_label_contains("调整浮层大小")
            .map(|node| node.rect())
            .min_by(|a, b| a.left().total_cmp(&b.left()))
            .expect("west resize zone");

        harness.drag_at(west.center());
        harness.run();
        harness.hover_at(west.center() - vec2(30.0, 0.0));
        harness.run();
        assert_eq!(
            resizes.borrow().last(),
            Some(&OverlayResize::Drag {
                edges: ResizeEdges {
                    north: false,
                    south: false,
                    east: false,
                    west: true,
                },
                pointer: west.center() - vec2(30.0, 0.0),
            }),
            "西缘热区只携带西边界，指针随窗口内坐标上交"
        );
    }
}
