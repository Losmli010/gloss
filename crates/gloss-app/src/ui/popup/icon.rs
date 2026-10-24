//! 页头应用图标的矢量栅格化：按目标物理像素边长渲染（含失败），失败隔离
//! 降级。
//!
//! 纯数据变换，不依赖界面库（egui）绘制侧（像素容器只是数据载体）；
//! 纹理装入随界面库上下文，留在绘制侧的图标纹理装入点。

use gloss_core::log::{thread, warn};

const APP_ICON_SVG: &[u8] = include_bytes!("../../../../../assets/icons/gloss-app-icon.svg");
const ICON_MARGIN: u32 = 100;
const ICON_CONTENT: u32 = 824;

pub(super) fn app_icon_image(side_px: u32) -> Option<egui::ColorImage> {
    render_app_icon(APP_ICON_SVG, side_px)
}

fn render_app_icon(svg: &[u8], side_px: u32) -> Option<egui::ColorImage> {
    use resvg::tiny_skia::Pixmap;
    use resvg::usvg::{Options, Transform, Tree};

    let tree = match Tree::from_data(svg, &Options::default()) {
        Ok(tree) => tree,
        Err(error) => {
            warn!(
                thread = thread::UI,
                error = %error,
                "app icon failed to parse, header renders without it"
            );
            return None;
        }
    };
    debug_assert_eq!(
        tree.size().width(),
        tree.size().height(),
        "app icon canvas is expected to be square"
    );
    let Some(mut pixmap) = Pixmap::new(side_px, side_px) else {
        warn!(
            thread = thread::UI,
            side_px = side_px,
            "app icon rasterization buffer rejected, header renders without it"
        );
        return None;
    };
    let scale = side_px as f32 / ICON_CONTENT as f32;
    let shift = -(ICON_MARGIN as f32) * scale;
    resvg::render(
        &tree,
        Transform::from_row(scale, 0.0, 0.0, scale, shift, shift),
        &mut pixmap.as_mut(),
    );
    Some(egui::ColorImage::from_rgba_premultiplied(
        [side_px as usize, side_px as usize],
        pixmap.data(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{APP_ICON_SVG, render_app_icon};

    #[test]
    fn render_app_icon_rejects_bad_bytes() {
        assert!(render_app_icon(b"not an svg", 40).is_none());
    }

    #[test]
    fn render_app_icon_rasterizes_the_content_square() {
        let image = render_app_icon(APP_ICON_SVG, 40).expect("embedded icon must render");
        assert_eq!(image.width(), 40);
        assert_eq!(image.height(), 40);
        let center = image.pixels[image.height() / 2 * image.width() + image.width() / 2];
        assert!(center.is_opaque(), "rasterized body must cover the center");
    }
}
