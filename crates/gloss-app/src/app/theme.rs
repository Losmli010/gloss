//! 主题施加：配置里的主题偏好经 egui 上下文的统一装入点写到两个窗口。

use gloss_core::config::Theme;
use gloss_core::log::{debug, thread};

use crate::ui::context;

use super::GlossApp;

impl GlossApp {
    /// 配置快照里的主题偏好：建帧（上下文建立时装入）与逐帧施加共用这一处取值。
    pub(super) fn target_theme(&self) -> Theme {
        self.config.snapshot().theme
    }

    /// 把主题偏好施加到两个 egui 上下文：偏好变化时才写，两个上下文各写一次。
    pub(super) fn apply_theme(&mut self) {
        let theme = self.target_theme();
        if self.applied_theme == Some(theme) {
            return;
        }
        // 帧尚未建立（窗口未起）时这里是空集：只记状态，不 panic。
        let written = context::reapply(
            [self.frame.as_ref(), self.settings_frame.as_ref()]
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
        assert!(app.applied_theme.is_none(), "first frame applies the theme");
        assert_eq!(app.target_theme(), Theme::System, "出厂跟随系统");

        app.apply_theme();
        assert_eq!(app.applied_theme, Some(Theme::System));

        config
            .save(Config {
                theme: Theme::Dark,
                ..Default::default()
            })
            .expect("save should succeed");
        app.apply_theme();
        assert_eq!(
            app.applied_theme,
            Some(Theme::Dark),
            "the cached preference must not block the new one"
        );
    }
}
