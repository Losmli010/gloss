//! 浮层宽度收敛决策：按内容体量在默认宽与上限宽两档间滞回切换。
//!
//! 纯决策函数，不依赖 egui 绘制侧；高度记账随实测布局（视口/溢出量），
//! 留在绘制侧的 `record_scrolled_height`。

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

/// 宽度收敛决策：已加宽的浮层内容矮于 [`TALL_SHRINK`] 才收回默认宽，
/// 默认宽的内容高于 [`TALL_GROW`] 才加宽——两阈值之间的滞回带保持原档，
/// 内容高随宽度变化时不振荡。
pub(super) fn resolve_width(last_width: f32, content_h: f32) -> f32 {
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
