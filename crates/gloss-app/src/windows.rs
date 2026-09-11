//! 浮层窗口：预创建复用，只显隐不反复销毁（06 §6.2）。

use std::sync::Arc;

use winit::dpi::LogicalSize;
use winit::error::OsError;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId, WindowLevel};

/// 浮层默认宽度（UI 规范 §2：默认 380px，长文本自适应上限 480px）
const OVERLAY_WIDTH: f64 = 380.0;
/// 浮层默认高度：M1 只有渲染自检面板，按内容给一个紧凑初值
const OVERLAY_HEIGHT: f64 = 200.0;

/// 窗口管理器：持有各窗口的生存期，对上层只暴露「谁的窗口」「显示/隐藏」。
pub struct WindowManager {
    overlay: Arc<Window>,
}

impl WindowManager {
    /// 创建浮层窗口。必须在 `resumed` 里调用——部分平台（macOS/Android）
    /// 只有进入 resumed 才允许创建窗口与 surface。
    pub fn new(event_loop: &ActiveEventLoop) -> Result<Self, OsError> {
        let attributes = Window::default_attributes()
            .with_title("Gloss")
            .with_inner_size(LogicalSize::new(OVERLAY_WIDTH, OVERLAY_HEIGHT))
            // 浮层是「内容即窗口」的卡片：无系统标题栏、置顶、尺寸由内容决定
            .with_decorations(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_resizable(false)
            // 透明：卡片圆角之外要让桌面透出来，因此 surface 也得选带 alpha 的合成模式
            .with_transparent(true);
        let overlay = event_loop.create_window(attributes)?;
        Ok(Self {
            overlay: Arc::new(overlay),
        })
    }

    /// 浮层窗口本体（事件处理用）。
    pub fn overlay(&self) -> &Window {
        &self.overlay
    }

    /// 浮层窗口的共享句柄（surface 要持有窗口到 'static）。
    pub fn overlay_handle(&self) -> Arc<Window> {
        Arc::clone(&self.overlay)
    }

    /// 事件是否来自浮层窗口。
    pub fn matches(&self, id: WindowId) -> bool {
        self.overlay.id() == id
    }

    /// 请求重绘浮层。
    pub fn request_redraw(&self) {
        self.overlay.request_redraw();
    }
}
