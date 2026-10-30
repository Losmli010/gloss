//! 出现动画：浮层每次显示的淡入推进。
//!
//! 动画起点挂界面库（egui）居内记忆：壳侧在每次显示时清除起点，显示后
//! 首帧重新写入，进度未满时请求短重绘推进淡入，重绘节拍被呈现管道钳到
//! 垂直同步（vsync）帧率；进度满后不再请求。时长与重绘节拍取动效档。

use crate::ui::style::motion;

fn appear_t0_id() -> egui::Id {
    egui::Id::new("overlay_appear_t0")
}

fn appear_progress(elapsed_secs: f64) -> f32 {
    (elapsed_secs / f64::from(motion::APPEAR)).clamp(0.0, 1.0) as f32
}

pub(super) fn appear_fade_in(ui: &mut egui::Ui) {
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
