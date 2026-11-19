//! 事件循环入口与跨线程唤醒句柄。

use std::error::Error;
use winit::event_loop::{EventLoop, EventLoopProxy};

use crate::channel::AppEndpoints;

use super::{AppEnv, GlossApp};

#[derive(Clone, Copy, Debug)]
pub enum UserEvent {
    Wake,
    RedrawSettings,
    FontsReady,
    OpenSettings,
    Quit,
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

pub fn run(
    endpoints: AppEndpoints,
    env: AppEnv,
    permissions_ready: bool,
    on_waker: impl FnOnce(Waker) -> Result<(), Box<dyn Error>>,
) -> Result<(), Box<dyn Error>> {
    let mut builder = EventLoop::<UserEvent>::with_user_event();
    super::loop_builder::suppress_default_menu(&mut builder);
    let event_loop = builder.build()?;
    let waker = Waker(event_loop.create_proxy());
    on_waker(waker)?;
    let proxy = event_loop.create_proxy();
    let mut app = GlossApp::new(endpoints, env, permissions_ready, Some(proxy));
    event_loop.run_app(&mut app)?;
    Ok(())
}
