//! 控件几何与窗口视觉尺寸：两窗口共用的控件定稿（空间单位）与窗口
//! 骨架的像素例外。
//!
//! 每个常量是一档视觉定稿——改值即改对应控件观感，快照基线随之重录；
//! 控件私有量政策废止，视图模块不再声明设计常数，只引用这里的档位或
//! 对其做算术。窗口尺寸是像素例外（窗口库的逻辑像素，与字号缩放正交），
//! 帧节拍与动效时长住动效模块。

use super::rem;

pub mod seal {
    use super::rem;

    pub const SIZE: f32 = rem(1.75);
    pub const TEXT_PADDING: f32 = rem(0.375);
    pub const GAP: f32 = rem(0.875);
    pub const ZHU_TEXT_GAP: f32 = rem(0.875);
    pub const SHU_TEXT_GAP: f32 = rem(0.75);
    pub const SHU_DASH_LENGTH: f32 = rem(0.375);
    pub const SHU_GAP_LENGTH: f32 = rem(0.25);
}

pub mod action {
    use super::rem;

    pub const ICON_SIZE: f32 = rem(1.375);
    pub const BUTTON: f32 = rem(1.625);
    pub const BUTTON_RADIUS: f32 = rem(0.5);
    pub const ICON_ARM: f32 = rem(0.375);
    pub const DRAG_STRIP_INSET: f32 = rem(0.5);
    pub const ROW_HEIGHT: f32 = rem(2.5);
}

pub mod toggle {
    use super::rem;

    pub const TRACK: egui::Vec2 = egui::vec2(rem(3.0), rem(1.625));
    pub const KNOB_RADIUS: f32 = rem(0.625);
    pub const KNOB_INSET: f32 = rem(0.25);
}

pub mod code_panel {
    use super::rem;

    pub const PADDING_V: f32 = rem(0.875);
    pub const PADDING_H: f32 = rem(1.125);
}

pub mod overlay {
    use super::rem;

    pub const HEADER_ICON: f32 = rem(1.625);
    pub const FOOTER_HEIGHT: f32 = rem(1.625);
    pub const FOOTER_DOT_RADIUS: f32 = rem(0.125);
    pub const FOOTER_DOT_COUNT: usize = 3;
    pub const WATERMARK_PADDING: f32 = rem(0.5);
    pub const MIN_BODY_VIEWPORT: f32 = rem(4.0);
    pub const ARROW_SCROLL_STEP: f32 = rem(4.0);
    pub const SHIMMER_BAR_HEIGHT: f32 = rem(1.0);
    pub const SHIMMER_BAR_RATIOS: [f32; 3] = [0.95, 0.78, 0.6];
}

pub mod settings {
    use super::rem;

    pub const WINDOW_PADDING: f32 = rem(1.375);
    pub const CLEAR_BUTTON_RESERVE: f32 = rem(7.375);
}

pub mod resize {
    pub const HOTSPOT: f32 = 4.0;
    pub const CORNER: f32 = 14.0;
}

pub mod window {
    pub const OVERLAY_WIDTH: f64 = 480.0;
    pub const OVERLAY_HEIGHT: f64 = 296.0;
    pub const OVERLAY_MIN_WIDTH: f64 = 320.0;
    pub const OVERLAY_MAX_WIDTH: f64 = 960.0;
    pub const OVERLAY_MIN_HEIGHT: f64 = 160.0;
    pub const SETTINGS_WIDTH: f64 = 396.0;
    pub const SETTINGS_HEIGHT: f64 = 640.0;
    pub const WORK_AREA_MARGIN: f64 = 96.0;
    pub const STREAM_HEIGHT_STEP: f64 = 48.0;
}
