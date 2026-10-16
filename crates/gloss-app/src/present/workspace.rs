//! 呈现资源唯一所有者：窗口管理器、各窗口的渲染帧、各自的下一帧时刻
//! 与已施加主题。壳与编排层经这里取用，不再各自持有窗口/帧的碎片。

use std::error::Error;
use std::sync::Arc;
use std::time::Instant;

use gloss_core::config::Theme;
use gloss_core::log::{debug, thread};
use winit::event_loop::ActiveEventLoop;

use crate::present::gpu::GpuContext;
use crate::present::render::{Frame, build_frame, build_window_stack};
use crate::present::windows::WindowManager;
use crate::ui::context;

/// 呈现资源：窗口与帧建好之前各字段为 `None`（`resumed` 时 [`Self::init`]）。
pub(crate) struct Workspace {
    /// 窗口管理器：浮层 + 设置窗的生存期与显隐。
    pub(crate) windows: Option<WindowManager>,
    /// 浮层的渲染帧；随窗口栈在 `resumed` 时建好，隐藏期保留。
    pub(crate) overlay_frame: Option<Frame>,
    /// 设置窗口的渲染帧；惰性建立（首次绘制前），关闭即释放（见
    /// [`Self::ensure_settings_frame`]）。
    pub(crate) settings_frame: Option<Frame>,
    /// GPU 设备上下文：全进程一份，设置帧惰性建立时取用。
    pub(crate) gpu: Option<Arc<GpuContext>>,
    /// 浮层 egui 要求的下一帧时间点；`None` 表示等到有事件再画。
    pub(crate) overlay_repaint: Option<Instant>,
    /// 设置窗口 egui 要求的下一帧时间点，与浮层各自独立。
    pub(crate) settings_repaint: Option<Instant>,
    /// 已施加到各 egui 上下文的主题；`None` 表示还没施加过（窗口未起时
    /// 会有这个状态）。
    pub(crate) applied_theme: Option<Theme>,
}

impl Workspace {
    pub(crate) fn new() -> Self {
        Self {
            windows: None,
            overlay_frame: None,
            settings_frame: None,
            gpu: None,
            overlay_repaint: None,
            settings_repaint: None,
            applied_theme: None,
        }
    }

    /// 建窗口栈与浮层的首帧渲染状态（`resumed` 里调用；resumed 可能
    /// 连续投递，调用方先查 [`Self::is_ready`]）。系统字体由后台线程延迟
    /// 装载（秒级，不挡首帧），装好经 `fonts_ready` 通知主线程换表重绘。
    pub(crate) fn init(
        &mut self,
        event_loop: &ActiveEventLoop,
        theme: Theme,
        fonts_ready: impl FnOnce() + Send + 'static,
    ) -> Result<(), Box<dyn Error>> {
        let (windows, context, frame) = build_window_stack(event_loop, theme)?;
        let contexts = vec![frame.egui_ctx.clone()];
        self.windows = Some(windows);
        self.gpu = Some(context);
        self.overlay_frame = Some(frame);
        match std::thread::Builder::new()
            .name("gloss-fonts".to_owned())
            .spawn(move || context::apply_system_fonts(contexts))
        {
            Ok(_) => {}
            Err(err) => {
                debug!(
                    thread = thread::UI,
                    error = %err,
                    "font loading thread not started, staying on builtin glyphs"
                );
                fonts_ready();
            }
        }
        Ok(())
    }

    /// 窗口与帧是否已建立（`resumed` 是否已跑过）。
    pub(crate) fn is_ready(&self) -> bool {
        self.windows.is_some()
    }

    /// 设置窗口的渲染帧，没有则建立（惰性建立点）：surface 与渲染器只在
    /// 首次绘制前创建，[`Self::drop_settings_frame`] 随关闭释放——交换链
    /// drawable 与字体图集不随进程常驻。建帧主题取当前偏好；新建上下文
    /// 的字体按装载进度自动取内置或完整定义（`ui::context::install`），
    /// 装载完成后的强制重施加覆盖先建的情形。
    pub(crate) fn ensure_settings_frame(&mut self, theme: Theme) -> Result<(), Box<dyn Error>> {
        if self.settings_frame.is_some() {
            return Ok(());
        }
        let Some(windows) = &self.windows else {
            return Err("settings frame requested before the window stack exists".into());
        };
        let Some(context) = &self.gpu else {
            return Err("settings frame requested before the gpu context exists".into());
        };
        self.settings_frame = Some(build_frame(windows.settings_handle(), context, theme)?);
        Ok(())
    }

    /// 释放设置窗口的渲染帧（关闭设置窗口时调用）：窗口本体保留，surface
    /// 与渲染器连同字体图集一并回收，下次显示重建。
    pub(crate) fn drop_settings_frame(&mut self) {
        self.settings_frame = None;
    }

    pub(crate) fn request_redraw(&self) {
        let Some(windows) = &self.windows else {
            return;
        };
        // 隐藏的浮层不请求重绘：隐藏窗口在 macOS 上仍取得到 drawable，
        // 事件驱动的零散重绘在隐藏期是整帧空转（tessellate + surface 呈现
        // 全跑一遍）。所有显示路径（显形决策、失败即弹）都在把窗口置回
        // 可见之后才请求重绘，这里的门控不吞掉任何一帧该画的画面。
        if !windows.is_visible() {
            return;
        }
        windows.request_redraw();
    }

    /// 把主题偏好施加到两个 egui 上下文：偏好变化时才写，两个上下文各写
    /// 一次。`force` 绕过变化判定（字体补装完成时必须重施加——字体表要
    /// 换完整定义，即使主题没变）。
    pub(crate) fn apply_theme_forced(&mut self, theme: Theme) {
        let force = true;
        self.apply_theme_inner(theme, force);
    }

    pub(crate) fn apply_theme(&mut self, theme: Theme) {
        self.apply_theme_inner(theme, false);
    }

    fn apply_theme_inner(&mut self, theme: Theme, force: bool) {
        if !force && self.applied_theme == Some(theme) {
            return;
        }
        // 帧尚未建立（窗口未起）时这里是空集：只记状态，不 panic。
        let written = context::reapply(
            [self.overlay_frame.as_ref(), self.settings_frame.as_ref()]
                .into_iter()
                .flatten()
                .map(|frame| &frame.egui_ctx),
            theme,
        );
        self.applied_theme = Some(theme);
        debug!(
            thread = thread::UI,
            theme = ?theme,
            contexts = written,
            "theme preference applied"
        );
    }
}

#[cfg(test)]
mod tests {
    use gloss_core::config::{Config, Theme};

    use crate::app::test_support::driven_app;

    #[test]
    fn apply_theme_elides_writes_until_the_preference_changes() {
        let (mut app, config, _store, _pe_tx, _ac_rx, _cmd_rx, _ev_tx) = driven_app();
        assert!(
            app.workspace.applied_theme.is_none(),
            "first frame applies the theme"
        );
        assert_eq!(app.env.target_theme(), Theme::System, "出厂跟随系统");

        app.workspace.apply_theme(app.env.target_theme());
        assert_eq!(app.workspace.applied_theme, Some(Theme::System));

        config
            .save(Config {
                theme: Theme::Dark,
                ..Default::default()
            })
            .expect("save should succeed");
        app.workspace.apply_theme(app.env.target_theme());
        assert_eq!(
            app.workspace.applied_theme,
            Some(Theme::Dark),
            "the cached preference must not block the new one"
        );
    }
}
