//! 应用菜单（Windows）：托盘图标常驻通知区域，右键弹托盘菜单——移植
//! 方案的行为差异项（托盘替代坞菜单），替代系统（macOS）的应用菜单。
//!
//! 托盘库（tray-icon）要求装配线程跑消息泵：装配点在主线程（窗口库
//! （winit）事件循环启动后），托盘窗口消息随主线程消息循环分发。菜单
//! 词条两个——设置与退出，词条动作只在主线程触发，经回调上交壳侧，由
//! 壳送入用户事件通道；左键行为按移植方案后置，只关掉菜单随左键弹出。
//! 图标复用坞图标资产（品牌同源），解码为原始位图交给托盘。双语词条表
//! 住平台中立共享层（common），本模块按装配所需词条取用。

use std::cell::RefCell;
use std::sync::{Arc, Mutex};

use gloss_core::log::{thread, warn};
use gloss_core::model::Locale;
use gloss_core::ports::MenuController;
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::common::menu_labels::{MenuLabels, labels};

const SETTINGS_ITEM_ID: &str = "gloss-settings";

const QUIT_ITEM_ID: &str = "gloss-quit";

pub fn install(
    locale: Locale,
    on_open_settings: Box<dyn Fn() + Send>,
    on_quit: Box<dyn Fn() + Send>,
) -> bool {
    let labels = labels(locale);
    let (menu, item_ids) = match context_menu(labels) {
        Ok(installed) => installed,
        Err(err) => {
            warn!(thread = thread::UI, error = %err, "tray menu build failed, tray not installed");
            return false;
        }
    };
    let (icon_width, icon_height, icon_rgba) = match tray_icon_bitmap() {
        Ok(bitmap) => bitmap,
        Err(err) => {
            warn!(thread = thread::UI, error = %err, "tray icon asset did not decode, tray not installed");
            return false;
        }
    };
    let Ok(icon) = Icon::from_rgba(icon_rgba, icon_width, icon_height) else {
        warn!(
            thread = thread::UI,
            "tray icon bitmap rejected, tray not installed"
        );
        return false;
    };
    let Ok(tray) = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip("Gloss")
        .with_icon(icon)
        .build()
    else {
        warn!(
            thread = thread::UI,
            "tray icon build failed, tray not installed"
        );
        return false;
    };
    TRAY.with(|slot| *slot.borrow_mut() = Some(tray));
    forward_menu_events(item_ids, on_open_settings, on_quit);
    true
}

pub struct SystemMenuController;

impl MenuController for SystemMenuController {
    fn install(
        &self,
        locale: Locale,
        on_open_settings: Box<dyn Fn() + Send>,
        on_quit: Box<dyn Fn() + Send>,
    ) -> bool {
        install(locale, on_open_settings, on_quit)
    }
}

thread_local! {
    static TRAY: RefCell<Option<TrayIcon>> = const { RefCell::new(None) };
}

type ItemIds = (MenuId, MenuId);

fn context_menu(labels: &'static MenuLabels) -> Result<(Menu, ItemIds), String> {
    let settings = MenuItem::with_id(SETTINGS_ITEM_ID, labels.settings, true, None);
    let quit = MenuItem::with_id(QUIT_ITEM_ID, labels.quit, true, None);
    let ids = (settings.id().clone(), quit.id().clone());
    let menu = Menu::with_items(&[&settings, &PredefinedMenuItem::separator(), &quit])
        .map_err(|err| err.to_string())?;
    Ok((menu, ids))
}

struct MenuActions {
    open_settings: Mutex<Box<dyn Fn() + Send>>,
    quit: Mutex<Box<dyn Fn() + Send>>,
}

impl MenuActions {
    fn dispatch(&self, id: &str) {
        let lock = if id == SETTINGS_ITEM_ID {
            self.open_settings.lock()
        } else if id == QUIT_ITEM_ID {
            self.quit.lock()
        } else {
            return;
        };
        match lock {
            Ok(action) => action(),
            Err(_) => warn!(
                thread = thread::UI,
                id, "menu callback mutex poisoned, menu click dropped"
            ),
        }
    }
}

fn forward_menu_events(
    item_ids: ItemIds,
    on_open_settings: Box<dyn Fn() + Send>,
    on_quit: Box<dyn Fn() + Send>,
) {
    let (settings_id, quit_id) = item_ids;
    let actions = Arc::new(MenuActions {
        open_settings: Mutex::new(on_open_settings),
        quit: Mutex::new(on_quit),
    });
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == settings_id || event.id == quit_id {
            actions.dispatch(event.id.as_ref());
        }
    }));
}

type Bitmap = (u32, u32, Vec<u8>);

fn tray_icon_bitmap() -> Result<Bitmap, String> {
    let decoded = image::load_from_memory(TRAY_ICON_PNG).map_err(|err| err.to_string())?;
    let rgba = decoded.to_rgba8();
    let (width, height) = rgba.dimensions();
    Ok((width, height, rgba.into_raw()))
}

static TRAY_ICON_PNG: &[u8] = include_bytes!("../../../../assets/icons/gloss-dock-icon.png");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_icon_asset_decodes_into_the_brand_bitmap() {
        let (width, height, rgba) = tray_icon_bitmap().expect("the embedded brand png must decode");
        assert_eq!(
            (width, height),
            (256, 256),
            "the brand asset is the 256px dock icon"
        );
        assert_eq!(
            rgba.len(),
            width as usize * height as usize * 4,
            "the tray bitmap must be full RGBA coverage"
        );
    }
}
