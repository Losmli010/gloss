//! 主题施加：配置主题映射为 egui 偏好并写到两个独立的 egui 上下文。

use gloss_core::config::Theme;
use gloss_core::log::{debug, thread};

use super::GlossApp;

impl GlossApp {
    /// 把配置里的主题偏好施加到两个 egui 上下文：偏好变化时才写，
    /// 两个上下文各写一次。
    pub(super) fn apply_theme(&mut self) {
        let preference = theme_preference(self.config.snapshot().theme);
        if self.applied_theme == Some(preference) {
            return;
        }
        // 帧尚未建立（窗口未起）时这里是空集：只记状态，不 panic。
        let written = apply_theme_to(
            [self.frame.as_ref(), self.settings_frame.as_ref()]
                .into_iter()
                .flatten()
                .map(|frame| &frame.egui_ctx),
            preference,
        );
        self.applied_theme = Some(preference);
        debug!(
            thread = thread::UI,
            theme = ?preference,
            contexts = written,
            "theme preference applied"
        );
    }
}

/// 配置主题 → egui 主题偏好（出厂跟随系统，设置页可固定明/暗）。
fn theme_preference(theme: Theme) -> egui::ThemePreference {
    match theme {
        Theme::System => egui::ThemePreference::System,
        Theme::Light => egui::ThemePreference::Light,
        Theme::Dark => egui::ThemePreference::Dark,
    }
}

/// 把偏好写到每个已建立的 egui 上下文，返回写到的上下文个数。
///
/// 浮层与设置各持一个**独立**的 `egui::Context`（options 不共享），所以必须
/// 逐个写——只写其中一个，用户会看到「改主题只影响半个界面」。抽成自由函数。
fn apply_theme_to<'a>(
    contexts: impl IntoIterator<Item = &'a egui::Context>,
    preference: egui::ThemePreference,
) -> usize {
    let mut written = 0;
    for ctx in contexts {
        ctx.set_theme(preference);
        written += 1;
    }
    written
}

#[cfg(test)]
mod tests {
    use gloss_core::config::{Config, Theme};

    use crate::app::test_support::driven_app;

    use super::{apply_theme_to, theme_preference};

    #[test]
    fn theme_preference_covers_every_variant() {
        assert_eq!(
            theme_preference(Theme::System),
            egui::ThemePreference::System
        );
        assert_eq!(theme_preference(Theme::Light), egui::ThemePreference::Light);
        assert_eq!(theme_preference(Theme::Dark), egui::ThemePreference::Dark);
        assert_eq!(
            theme_preference(Config::default().theme),
            egui::ThemePreference::System,
            "factory default must follow the system"
        );
    }

    #[test]
    fn apply_theme_writes_every_context() {
        let overlay = egui::Context::default();
        let settings = egui::Context::default();

        for preference in [
            egui::ThemePreference::Light,
            egui::ThemePreference::Dark,
            egui::ThemePreference::System,
        ] {
            assert_eq!(
                apply_theme_to([&overlay, &settings], preference),
                2,
                "两个已建立的上下文都要写到"
            );
            for ctx in [&overlay, &settings] {
                assert_eq!(
                    ctx.options(|opt| opt.theme_preference),
                    preference,
                    "{preference:?} 必须落到每个上下文上"
                );
            }
        }

        assert_eq!(
            apply_theme_to(
                std::iter::empty::<&egui::Context>(),
                egui::ThemePreference::Dark
            ),
            0,
            "窗口尚未建立时没有上下文可写，也不能 panic"
        );
    }

    #[test]
    fn apply_theme_elides_writes_until_the_preference_changes() {
        let (mut app, config, _store, _pe_tx, _ac_rx, _cmd_rx, _ev_tx) = driven_app();
        assert!(app.applied_theme.is_none(), "first frame applies the theme");

        app.apply_theme();
        assert_eq!(app.applied_theme, Some(egui::ThemePreference::System));

        config
            .save(Config {
                theme: Theme::Dark,
                ..Default::default()
            })
            .expect("save should succeed");
        app.apply_theme();
        assert_eq!(
            app.applied_theme,
            Some(egui::ThemePreference::Dark),
            "the cached preference must not block the new one"
        );
    }
}
