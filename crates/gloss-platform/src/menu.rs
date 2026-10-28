//! 应用菜单（macOS）：随系统语言装配本地化的应用菜单，设置项直开设置窗。
//!
//! 窗口库（winit）的缺省菜单词条硬编码英文，且窗口库未暴露菜单
//! 应用程序接口（API）；本模块经系统框架（AppKit）绑定自建菜单并整体
//! 替换缺省菜单，词条取系统语言（进程启动期读一次）。菜单动作回调只在
//! 主线程触发，设置项经回调上交壳侧，由壳送入用户事件通道。
//! 词条属平台层启动期文案，早于应用文案表装入点，随放行标记显式豁免
//! 文案门禁。

use std::cell::RefCell;

use gloss_core::model::Locale;
use objc2::AnyThread;
use objc2::DefinedClass;
use objc2::define_class;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::rc::autoreleasepool;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
use objc2_foundation::{NSString, ns_string};

struct Labels {
    about: &'static str,
    settings: &'static str,
    services: &'static str,
    hide: &'static str,
    hide_others: &'static str,
    show_all: &'static str,
    quit: &'static str,
}

const ZH: Labels = Labels {
    about: "关于 Gloss",     // i18n:allow 平台层启动期菜单词条，文案表装入点之前
    settings: "设置…",       // i18n:allow 平台层启动期菜单词条，文案表装入点之前
    services: "服务",        // i18n:allow 平台层启动期菜单词条，文案表装入点之前
    hide: "隐藏 Gloss",      // i18n:allow 平台层启动期菜单词条，文案表装入点之前
    hide_others: "隐藏其他", // i18n:allow 平台层启动期菜单词条，文案表装入点之前
    show_all: "显示全部",    // i18n:allow 平台层启动期菜单词条，文案表装入点之前
    quit: "退出 Gloss",      // i18n:allow 平台层启动期菜单词条，文案表装入点之前
};

const EN: Labels = Labels {
    about: "About Gloss",
    settings: "Settings…", // i18n:allow platform-layer startup menu copy, installed before the app catalog load point
    services: "Services",
    hide: "Hide Gloss",
    hide_others: "Hide Others",
    show_all: "Show All",
    quit: "Quit Gloss",
};

fn labels(locale: Locale) -> &'static Labels {
    match locale {
        Locale::Zh => &ZH,
        Locale::En => &EN,
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[ivars = Box<dyn Fn() + Send>]
    struct SettingsAction;

    impl SettingsAction {
        #[unsafe(method(glossOpenSettings:))]
        fn open_settings(&self, _sender: Option<&AnyObject>) {
            self.ivars()();
        }
    }
);

impl SettingsAction {
    fn new(on_open_settings: Box<dyn Fn() + Send>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(on_open_settings);
        // SAFETY: `this` was just allocated with our ivars and has not been
        // initialized; `init` is NSObject's designated initializer and the
        // object is used nowhere else before it returns.
        unsafe { msg_send![super(this), init] }
    }
}

thread_local! {
    static SETTINGS_ACTION: RefCell<Option<Retained<SettingsAction>>> =
        const { RefCell::new(None) };
}

pub fn install(locale: Locale, on_open_settings: impl Fn() + Send + 'static) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    autoreleasepool(|_| {
        let app = NSApplication::sharedApplication(mtm);
        let menubar = NSMenu::initWithTitle(mtm.alloc(), ns_string!("Gloss"));
        let app_item = NSMenuItem::new(mtm);
        menubar.addItem(&app_item);
        let submenu = app_submenu(mtm, labels(locale), on_open_settings);
        app_item.setSubmenu(Some(&submenu));
        app.setMainMenu(Some(&menubar));
        true
    })
}

fn app_submenu(
    mtm: MainThreadMarker,
    labels: &'static Labels,
    on_open_settings: impl Fn() + Send + 'static,
) -> Retained<NSMenu> {
    let app_menu = NSMenu::initWithTitle(mtm.alloc(), ns_string!("Gloss"));
    app_menu.addItem(&menu_item(
        mtm,
        labels.about,
        Some(sel!(orderFrontStandardAboutPanel:)),
        None,
        None,
    ));
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));
    app_menu.addItem(&settings_item(mtm, labels.settings, on_open_settings));
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));

    let services_title = NSString::from_str(labels.services);
    let services_menu = NSMenu::initWithTitle(mtm.alloc(), &services_title);
    let services_item = menu_item(mtm, labels.services, None, None, None);
    services_item.setSubmenu(Some(&services_menu));
    app_menu.addItem(&services_item);
    NSApplication::sharedApplication(mtm).setServicesMenu(Some(&services_menu));
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));

    app_menu.addItem(&menu_item(
        mtm,
        labels.hide,
        Some(sel!(hide:)),
        Some("h"),
        None,
    ));
    app_menu.addItem(&menu_item(
        mtm,
        labels.hide_others,
        Some(sel!(hideOtherApplications:)),
        Some("h"),
        Some(NSEventModifierFlags::Option | NSEventModifierFlags::Command),
    ));
    app_menu.addItem(&menu_item(
        mtm,
        labels.show_all,
        Some(sel!(unhideAllApplications:)),
        None,
        None,
    ));
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));
    app_menu.addItem(&menu_item(
        mtm,
        labels.quit,
        Some(sel!(terminate:)),
        Some("q"),
        None,
    ));
    app_menu
}

fn settings_item(
    mtm: MainThreadMarker,
    title: &'static str,
    on_open_settings: impl Fn() + Send + 'static,
) -> Retained<NSMenuItem> {
    let action = SettingsAction::new(Box::new(on_open_settings));
    let item = menu_item(mtm, title, Some(sel!(glossOpenSettings:)), Some(","), None);
    // SAFETY: `action` stays alive in the main-thread slot below for the whole
    // process lifetime, matching AppKit's unretained-target contract; the menu
    // item only reads it while dispatching on the main thread.
    unsafe { item.setTarget(Some(&action)) };
    SETTINGS_ACTION.with(|slot| *slot.borrow_mut() = Some(action));
    item
}

fn menu_item(
    mtm: MainThreadMarker,
    title: &str,
    action: Option<objc2::runtime::Sel>,
    key_equivalent: Option<&str>,
    modifier_mask: Option<NSEventModifierFlags>,
) -> Retained<NSMenuItem> {
    let title = NSString::from_str(title);
    let key = NSString::from_str(key_equivalent.unwrap_or(""));
    // SAFETY: a standard designated initializer invoked with owned strings
    // and a selector from `sel!`; the returned item is retained by the caller.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(mtm.alloc(), &title, action, &key)
    };
    if let Some(mask) = modifier_mask {
        item.setKeyEquivalentModifierMask(mask);
    }
    item
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_follow_their_language_per_locale() {
        for (table, expect_cjk) in [(&ZH, true), (&EN, false)] {
            let rows = [
                table.about,
                table.settings,
                table.services,
                table.hide,
                table.hide_others,
                table.show_all,
                table.quit,
            ];
            for row in rows {
                let has_cjk = row.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c));
                assert_eq!(has_cjk, expect_cjk, "label table language mismatch: {row}");
                assert!(!row.trim().is_empty(), "menu labels must not be blank");
            }
        }
    }

    #[test]
    fn menu_install_degrades_off_the_main_thread() {
        let applied = std::thread::spawn(|| install(Locale::Zh, || {}))
            .join()
            .expect("test thread must not panic");
        assert!(
            !applied,
            "off-main-thread call must report failure, not install a menu"
        );
    }
}
