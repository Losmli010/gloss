//! 事件循环入口与跨线程唤醒句柄。

use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use gloss_core::config_handle::ConfigHandle;
use gloss_core::model::Locale;
use gloss_core::ports::{ConfigStore, SceneProbe};
use winit::event_loop::{EventLoop, EventLoopProxy};
use winit::platform::macos::EventLoopBuilderExtMacOS;

use crate::channel::AppEndpoints;
use crate::update::UpdateWiring;

use super::GlossApp;

#[derive(Clone, Copy, Debug)]
pub enum UserEvent {
    Wake,
    RedrawSettings,
    FontsReady,
    OpenSettings,
}

#[derive(Clone, Debug)]
pub struct Waker(EventLoopProxy<UserEvent>);

impl Waker {
    pub fn wake(&self) -> bool {
        self.0.send_event(UserEvent::Wake).is_ok()
    }

    pub fn wake_settings(&self) -> bool {
        self.0.send_event(UserEvent::RedrawSettings).is_ok()
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "组装点的主入口：每项都是不同关注点的注入端点，收敛成结构体只会把清单变成字段袋"
)]
pub fn run(
    endpoints: AppEndpoints,
    config: Arc<ConfigHandle>,
    store: Arc<dyn ConfigStore>,
    scene: Arc<dyn SceneProbe>,
    system_locale: Locale,
    permissions_ready: bool,
    update: UpdateWiring,
    clipboard_watch_enabled: Arc<AtomicBool>,
    on_waker: impl FnOnce(Waker) -> Result<(), Box<dyn Error>>,
) -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .with_default_menu(false)
        .build()?;
    let waker = Waker(event_loop.create_proxy());
    on_waker(waker)?;
    let proxy = event_loop.create_proxy();
    let mut app = GlossApp::new(
        endpoints,
        config,
        store,
        scene,
        system_locale,
        permissions_ready,
        update,
        clipboard_watch_enabled,
        Some(proxy),
    );
    event_loop.run_app(&mut app)?;
    Ok(())
}
