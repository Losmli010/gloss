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

pub(crate) struct Workspace {
    pub(crate) windows: Option<WindowManager>,
    pub(crate) overlay_frame: Option<Frame>,
    pub(crate) settings_frame: Option<Frame>,
    pub(crate) gpu: Option<Arc<GpuContext>>,
    pub(crate) overlay_repaint: Option<Instant>,
    pub(crate) settings_repaint: Option<Instant>,
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

    pub(crate) fn is_ready(&self) -> bool {
        self.windows.is_some()
    }

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

    pub(crate) fn drop_settings_frame(&mut self) {
        self.settings_frame = None;
    }

    pub(crate) fn request_redraw(&self) {
        let Some(windows) = &self.windows else {
            return;
        };
        if !windows.is_visible() {
            return;
        }
        windows.request_redraw();
    }

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
