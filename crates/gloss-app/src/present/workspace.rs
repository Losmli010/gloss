//! 呈现资源唯一所有者：窗口管理器、各窗口的渲染帧、各自的下一帧时刻
//! 与已施加主题。壳与编排层经这里取用，不再各自持有窗口/帧的碎片。

use std::error::Error;
use std::time::Instant;

use gloss_core::config::Theme;
use gloss_core::log::{debug, thread};
use winit::event_loop::ActiveEventLoop;

use crate::present::render::{Frame, build_window_stack};
use crate::present::windows::WindowManager;
use crate::ui::context;

/// 呈现资源：窗口与帧建好之前各字段为 `None`（`resumed` 时 [`Self::init`]）。
pub(crate) struct Workspace {
    /// 窗口管理器：浮层 + 设置窗 + 向导窗的生存期与显隐。
    pub(crate) windows: Option<WindowManager>,
    /// 浮层的渲染帧；随窗口栈在 `resumed` 时建好，隐藏期保留。
    pub(crate) overlay_frame: Option<Frame>,
    /// 设置窗口的渲染帧；同上。
    pub(crate) settings_frame: Option<Frame>,
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
            overlay_repaint: None,
            settings_repaint: None,
            applied_theme: None,
        }
    }

    /// 建窗口栈与各窗口的首帧渲染状态（`resumed` 里调用；resumed 可能
    /// 连续投递，调用方先查 [`Self::is_ready`]）。系统字体由后台线程延迟
    /// 装载（秒级，不挡首帧），装好经 `fonts_ready` 通知主线程换表重绘。
    pub(crate) fn init(
        &mut self,
        event_loop: &ActiveEventLoop,
        theme: Theme,
        fonts_ready: impl FnOnce() + Send + 'static,
    ) -> Result<(), Box<dyn Error>> {
        let (windows, frame, settings_frame) = build_window_stack(event_loop, theme)?;
        let contexts = vec![frame.egui_ctx.clone(), settings_frame.egui_ctx.clone()];
        self.windows = Some(windows);
        self.overlay_frame = Some(frame);
        self.settings_frame = Some(settings_frame);
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

    pub(crate) fn request_redraw(&self) {
        if let Some(windows) = &self.windows {
            windows.request_redraw();
        }
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
