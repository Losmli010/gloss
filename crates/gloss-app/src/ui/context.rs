//! 界面库（egui）上下文的统一装入点：字体、主题与界面样式在此一次装好。
//!
//! 浮层与设置窗各持一个独立的界面库上下文（字体表与选项都不共享），
//! 任何“每个窗口都要有”的设置都必须逐个上下文施加。本模块是唯一的施加入口：
//! 建立上下文、运行中的偏好重套、系统字体的
//! 延迟补装三条路径共用同一个装入助手——新增这类设置项
//! 时只改该助手，各条路径自动跟上。
//! 界面样式的灌装是语义调色板到界面库两棵样式树的一次性写入：
//! 明暗两棵树都灌满品牌语义，控件随主题自动跟随，视图层不再有取色分支。

use std::time::Instant;

use egui::{Context, ThemePreference};
use gloss_core::config::Theme;
use gloss_core::log::{info, thread};

pub fn new_context(theme: Theme) -> Context {
    let ctx = Context::default();
    install(&ctx, theme);
    info!(
        thread = thread::UI,
        "egui context created, system fonts deferred"
    );
    ctx
}

pub fn reapply<'a>(contexts: impl IntoIterator<Item = &'a Context>, theme: Theme) -> usize {
    let mut written = 0;
    for ctx in contexts {
        install(ctx, theme);
        written += 1;
    }
    written
}

pub fn apply_system_fonts(contexts: impl IntoIterator<Item = Context>) {
    let started = Instant::now();
    let (_definitions, cjk_fallback) = super::fonts::definitions();
    let mut applied = 0;
    for ctx in contexts {
        install_fonts(&ctx);
        applied += 1;
    }
    info!(
        thread = thread::FONTS,
        cjk_fallback,
        contexts = applied,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "system fonts applied"
    );
}

fn install_fonts(ctx: &Context) {
    let definitions = if super::fonts::system_fonts_ready() {
        super::fonts::definitions().0
    } else {
        super::fonts::builtin_definitions()
    };
    ctx.set_fonts(definitions);
    super::popup::clear_cached_text_galleys(ctx);
}

fn install(ctx: &Context, theme: Theme) {
    let (definitions, _cjk_fallback) = if super::fonts::system_fonts_ready() {
        super::fonts::definitions()
    } else {
        (super::fonts::builtin_definitions(), false)
    };
    ctx.set_fonts(definitions);
    super::popup::clear_cached_text_galleys(ctx);
    ctx.set_theme(theme_preference(theme));
    apply_visuals(ctx);
}

pub(crate) fn apply_visuals(ctx: &Context) {
    ctx.style_mut_of(egui::Theme::Light, |style| {
        fill_style(style, &super::style::LIGHT);
    });
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        fill_style(style, &super::style::DARK);
    });
}

fn fill_style(style: &mut egui::Style, colors: &super::style::Palette) {
    fill_visuals(&mut style.visuals, colors);
    fill_spacing(&mut style.spacing);
    style.animation_time = super::style::motion::APPEAR;
}

fn fill_visuals(visuals: &mut egui::Visuals, colors: &super::style::Palette) {
    visuals.selection.bg_fill = colors.accent;
}

fn fill_spacing(spacing: &mut egui::Spacing) {
    use super::style::space;
    spacing.item_spacing = egui::vec2(space::ITEM, space::ITEM);
    spacing.button_padding = egui::vec2(space::TIGHT, space::TIGHT);
    let card_margin = egui::Margin::from(space::CARD_PADDING);
    spacing.menu_margin = card_margin;
    spacing.window_margin = card_margin;
}

#[cfg(test)]
pub(crate) fn install_kittest_fonts(ctx: &Context) {
    ctx.set_fonts(super::fonts::builtin_definitions());
    super::popup::clear_cached_text_galleys(ctx);
}

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

        let has_cjk = |ctx: &Context| {
            let mut has = false;
            let output = ctx.run_ui(egui::RawInput::default(), |ui| {
                has = ui
                    .ctx()
                    .fonts(|fonts| fonts.definitions().font_data.contains_key(fonts::FONT_NAME));
            });
            output.drop_without_applying_deltas();
            has
        };
        let serif_bound = |ctx: &Context| {
            let mut bound = false;
            let output = ctx.run_ui(egui::RawInput::default(), |ui| {
                bound = ui.ctx().fonts(|fonts| {
                    fonts
                        .definitions()
                        .families
                        .get(&fonts::serif_family())
                        .is_some_and(|chain| !chain.is_empty())
                });
            });
            output.drop_without_applying_deltas();
            bound
        };
        assert!(
            serif_bound(&ctx),
            "命名字体族必须恒绑定（epaint 对未绑定族 panic）"
        );

        apply_system_fonts([ctx.clone()]);
        assert!(
            has_cjk(&ctx),
            "延迟装载后 CJK 后备必须接上（宿主机需有系统 CJK 字体）"
        );
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
