//! egui 上下文的统一装入点：字体与主题在此一次装好。
//!
//! 浮层与设置窗各持一个独立的 `egui::Context`（字体表与 options 都不共享），
//! 任何「每个窗口都要有」的设置都必须逐个上下文施加。本模块是唯一的施加入口：
//! 建立上下文走 [`new_context`]，运行中的偏好变化走 [`reapply`]，两者共用
//! [`install`]——新增这类设置项时只改 [`install`]，两条路径自动跟上。

use egui::{Context, ThemePreference};
use gloss_core::config::Theme;

/// 新建装好的 egui 上下文（字体与主题一次到位）——全仓唯一的上下文建立入口。
pub fn new_context(theme: Theme) -> Context {
    let ctx = Context::default();
    install(&ctx, theme);
    ctx
}

/// 把每上下文设置施加到一批已存在的上下文，返回写到的个数。
pub fn reapply<'a>(contexts: impl IntoIterator<Item = &'a Context>, theme: Theme) -> usize {
    let mut written = 0;
    for ctx in contexts {
        install(ctx, theme);
        written += 1;
    }
    written
}

/// 施加全部「每个 egui 上下文都要有」的设置；重复施加结果不变
/// （字体表按定义相等判定，相等即不重建）。
fn install(ctx: &Context, theme: Theme) {
    super::fonts::install(ctx);
    ctx.set_theme(theme_preference(theme));
}

/// 配置主题 → egui 主题偏好（出厂跟随系统，设置页可固定明/暗）。
fn theme_preference(theme: Theme) -> ThemePreference {
    match theme {
        Theme::System => ThemePreference::System,
        Theme::Light => ThemePreference::Light,
        Theme::Dark => ThemePreference::Dark,
    }
}

#[cfg(test)]
mod tests {
    use crate::ui::fonts;

    use super::*;

    #[test]
    fn theme_preference_covers_every_variant() {
        assert_eq!(theme_preference(Theme::System), ThemePreference::System);
        assert_eq!(theme_preference(Theme::Light), ThemePreference::Light);
        assert_eq!(theme_preference(Theme::Dark), ThemePreference::Dark);
        assert_eq!(
            theme_preference(Theme::default()),
            ThemePreference::System,
            "出厂默认跟随系统"
        );
    }

    #[test]
    fn new_context_carries_fonts_and_theme() {
        let ctx = new_context(Theme::Dark);
        assert_eq!(
            ctx.options(|options| options.theme_preference),
            ThemePreference::Dark
        );

        let mut fallback = false;
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            fallback = ui
                .ctx()
                .fonts(|fonts| fonts.definitions().font_data.contains_key(fonts::FONT_NAME));
        });
        // 没有渲染后端消费图集增量：丢弃（epaint 在析构时会对未处理增量断言）
        output.drop_without_applying_deltas();
        assert!(fallback, "新上下文必须已接上 CJK 后备字体");
    }

    #[test]
    fn reapply_writes_every_context() {
        let overlay = new_context(Theme::System);
        let settings = new_context(Theme::System);

        for theme in [Theme::Light, Theme::Dark, Theme::System] {
            assert_eq!(
                reapply([&overlay, &settings], theme),
                2,
                "两个已建立的上下文都要写到"
            );
            for ctx in [&overlay, &settings] {
                assert_eq!(
                    ctx.options(|options| options.theme_preference),
                    theme_preference(theme),
                    "{theme:?} 必须落到每个上下文上"
                );
            }
        }

        assert_eq!(
            reapply(std::iter::empty::<&Context>(), Theme::Dark),
            0,
            "窗口尚未建立时没有上下文可写，也不能 panic"
        );
    }
}
