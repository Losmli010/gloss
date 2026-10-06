//! egui 上下文的统一装入点：字体与主题在此一次装好。
//!
//! 浮层与设置窗各持一个独立的 `egui::Context`（字体表与 options 都不共享），
//! 任何「每个窗口都要有」的设置都必须逐个上下文施加。本模块是唯一的施加入口：
//! 建立上下文走 [`new_context`]，运行中的偏好变化走 [`reapply`]，系统字体的
//! 延迟补装走 [`apply_system_fonts`]，三者共用 [`install`]——新增这类设置项
//! 时只改 [`install`]，各条路径自动跟上。

use std::time::Instant;

use egui::{Context, ThemePreference};
use gloss_core::config::Theme;
use gloss_core::log::{info, thread};

/// 新建装好基础设置的 egui 上下文——全仓唯一的上下文建立入口。
///
/// 字体只装内置字形快路径（命名字体族恒绑定）：系统字体装载是秒级的，
/// 不挡首帧，由 [`apply_system_fonts`] 在后台线程装载后补装。
pub fn new_context(theme: Theme) -> Context {
    let ctx = Context::default();
    install(&ctx, theme);
    info!(
        thread = thread::UI,
        "egui context created, system fonts deferred"
    );
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

/// 装载系统字体并补装到一批上下文的字体表（阻塞调用，供启动后的后台
/// 线程执行）。装载完成前各上下文一直用内置字形快路径；完成后逐上下文
/// 换完整字体表。重绘不在这里请求——egui 的重绘请求没有跨线程唤醒通路
/// （egui-winit 不注册 repaint callback），完成信号由调用方经 Waker 送到
/// 主线程（`UserEvent::FontsReady`），主题也由主线程按当前偏好重施加，
/// 装载窗口内的主题变更因此不会被这里的快照覆盖。
pub fn apply_system_fonts(contexts: impl IntoIterator<Item = Context>) {
    let started = Instant::now();
    // 先走一次完整装载把字体字节缓存填上，下面 install 的就绪判定才会
    // 选完整定义；字节在 OnceLock 里，重复调用命中缓存。
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

/// 只换字体表：补装路径专用。主题随 `UserEvent::FontsReady` 在主线程由
/// reapply 统一施加，这里不碰主题。
fn install_fonts(ctx: &Context) {
    let definitions = if super::fonts::system_fonts_ready() {
        super::fonts::definitions().0
    } else {
        super::fonts::builtin_definitions()
    };
    ctx.set_fonts(definitions);
}

/// 施加全部「每个 egui 上下文都要有」的设置。系统字体字节就位前走内置
/// 快路径（[`fonts::builtin_definitions`]），就位后走完整定义（字节级
/// 缓存命中，秒回）。对 egui 上下文的写入都收在这里。
fn install(ctx: &Context, theme: Theme) {
    let (definitions, _cjk_fallback) = if super::fonts::system_fonts_ready() {
        super::fonts::definitions()
    } else {
        (super::fonts::builtin_definitions(), false)
    };
    ctx.set_fonts(definitions);
    ctx.set_theme(theme_preference(theme));
}

/// kittest 自建上下文的字体绑定（测试专用）：与启动快路径同一套内置
/// 定义，排版字号照常生效、字形不依赖宿主字体（快照 tofu 约定）。
/// 对 set_fonts 的调用收在本模块（统一装入点），测试也不例外。
#[cfg(test)]
pub(crate) fn install_kittest_fonts(ctx: &Context) {
    ctx.set_fonts(super::fonts::builtin_definitions());
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
                        .get(&egui::FontFamily::Name(fonts::FONT_SERIF_NAME.into()))
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
