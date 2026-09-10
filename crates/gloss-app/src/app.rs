//! winit 事件循环：主线程的窗口生命周期与渲染驱动。

use std::error::Error;

use gloss_core::log::{error, thread};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::WindowId;

use crate::windows::WindowManager;

/// 投递给主线程的自定义事件。
#[derive(Clone, Copy, Debug)]
pub enum UserEvent {
    /// 有跨线程消息待处理
    Wake,
}

/// 唤醒主线程的句柄：事件线程与 tokio 各持一份 clone。
///
/// 用 `EventLoopProxy` 而不是让主线程 `try_recv` 轮询（08 §7.3）：轮询只能在
/// winit 因别的事件醒来时顺带取消息，空闲时最坏延迟一帧，且要求主线程周期性
/// 空转；代理唤醒由发送方立即触发，无消息时主线程可以一直睡。
#[derive(Clone, Debug)]
pub struct Waker(EventLoopProxy<UserEvent>);

impl Waker {
    /// 唤醒主线程；返回 `false` 表示事件循环已退出。
    pub fn wake(&self) -> bool {
        self.0.send_event(UserEvent::Wake).is_ok()
    }
}

/// 启动事件循环，直到退出才返回。
///
/// `on_waker` 拿到唤醒句柄——`main.rs` 是唯一组装点，句柄要由它分发给
/// 平台事件线程与 tokio，库这边不替上层决定跨线程拓扑。
pub fn run(on_waker: impl FnOnce(Waker)) -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let waker = Waker(event_loop.create_proxy());
    on_waker(waker);
    let mut app = GlossApp::default();
    event_loop.run_app(&mut app)?;
    Ok(())
}

#[derive(Default)]
struct GlossApp {
    windows: Option<WindowManager>,
}

impl ApplicationHandler<UserEvent> for GlossApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // resumed 可能连续投递，窗口只建一次
        if self.windows.is_some() {
            return;
        }
        match WindowManager::new(event_loop) {
            Ok(windows) => {
                windows.request_redraw();
                self.windows = Some(windows);
            }
            Err(err) => {
                // 没有窗口就没有任何可做的事，带病进主循环只会静默空转
                error!(thread = thread::UI, error = %err, "failed to create overlay window");
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: UserEvent) {
        if let Some(windows) = &self.windows {
            windows.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(windows) = &self.windows else {
            return;
        };
        if !windows.matches(window_id) {
            return;
        }
        if let WindowEvent::CloseRequested = event {
            event_loop.exit();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // 没有待办时一直睡，等窗口事件或唤醒句柄把自己叫醒
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}
