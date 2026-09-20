//! winit 事件分发：`ApplicationHandler` 实现，把窗口事件路由到渲染、
//! 通道消费与浮层显隐，并归并各窗口的唤醒时刻。

use std::error::Error;
use std::time::Instant;

use gloss_core::log::{error, thread};
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::WindowId;

use super::{GlossApp, UserEvent};

impl GlossApp {
    /// 建窗口栈 → 建两个窗口的帧状态，一次做完。
    fn init(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Box<dyn Error>> {
        let (windows, frame, settings_frame) = super::build_window_stack(event_loop)?;
        self.frame = Some(frame);
        self.settings_frame = Some(settings_frame);
        self.windows = Some(windows);
        self.draw();
        Ok(())
    }
}

/// 两个唤醒时刻里更早的那个；都没有则不用定时唤醒。
fn sooner(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

impl ApplicationHandler<UserEvent> for GlossApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // resumed 可能连续投递，渲染栈只起一次
        if self.windows.is_some() {
            return;
        }
        if let Err(err) = self.init(event_loop) {
            // 没有窗口与渲染栈就没有可做的事，带病进主循环只会静默空转
            error!(thread = thread::UI, error = %err, "failed to start window and render stack");
            event_loop.exit();
            return;
        }
        // 浮层预创建即隐藏（Idle 态）；先在隐藏状态画一帧预热——egui 图集构建、
        // Metal 管线编译与纹理上传都发生在首帧，不预热的话首次显示会超预算
        self.draw();
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _event: UserEvent) {
        self.drain_platform_events();
        self.drain_events(event_loop);
        self.request_redraw();
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
        let is_overlay = windows.matches_overlay(window_id);
        let is_settings = !is_overlay && windows.matches_settings(window_id);
        if !is_overlay && !is_settings {
            return;
        }

        if matches!(event, WindowEvent::RedrawRequested) {
            if is_settings {
                self.draw_settings();
            } else {
                self.draw();
            }
            return;
        }

        // 其余事件先喂给对应窗口的 egui，它决定是否消化掉以及要不要重绘
        let repaint = {
            let frame = if is_settings {
                self.settings_frame.as_mut()
            } else {
                self.frame.as_mut()
            };
            let Some(frame) = frame else {
                return;
            };
            frame.egui.on_window_event(&frame.window, &event).repaint
        };
        if repaint {
            if is_settings {
                windows.request_redraw_settings();
            } else {
                windows.request_redraw();
            }
        }

        match event {
            WindowEvent::CloseRequested => {
                if is_settings {
                    // 设置窗口的关闭是「取消编辑」：隐藏丢弃草稿，进程照常
                    self.close_settings();
                } else {
                    event_loop.exit();
                }
            }
            WindowEvent::Focused(false) if is_overlay => {
                // 浮层失焦回 Idle：只隐藏不销毁；设置窗口失焦保持打开。
                // 清掉渲染截止与出现动画起点，防空转、让重显重新淡入。
                self.overlay_repaint = None;
                if let Some(frame) = self.frame.as_ref() {
                    crate::ui::popup::reset_appear_animation(&frame.egui_ctx);
                }
                if let Some(windows) = &self.windows {
                    windows.hide();
                    self.auto_hide = None;
                    self.machine.hide_overlay();
                }
            }
            WindowEvent::Resized(size) => {
                let frame = if is_settings {
                    self.settings_frame.as_mut()
                } else {
                    self.frame.as_mut()
                };
                if let Some(frame) = frame {
                    frame.surface.resize(size);
                }
            }
            _ => {}
        }
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if !matches!(cause, StartCause::ResumeTimeReached { .. }) {
            return;
        }
        // 到点的可能是自动隐藏，也可能是 egui 要的下一帧，也可能两者都是
        let now = Instant::now();
        if self.auto_hide.is_some_and(|deadline| deadline <= now) {
            self.on_auto_hide(event_loop);
        }
        if self.overlay_repaint.is_some_and(|deadline| deadline <= now) {
            self.request_redraw();
        }
        if self
            .settings_repaint
            .is_some_and(|deadline| deadline <= now)
            && let Some(windows) = &self.windows
        {
            windows.request_redraw_settings();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // 没有待处理的唤醒时刻就彻底睡下，等窗口事件或唤醒句柄把自己叫醒
        event_loop.set_control_flow(
            sooner(self.overlay_repaint, self.settings_repaint)
                .and_then(|repaint| sooner(Some(repaint), self.auto_hide))
                .map_or(ControlFlow::Wait, ControlFlow::WaitUntil),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::sooner;

    #[test]
    fn sooner_picks_the_earliest_deadline() {
        let now = Instant::now();
        let a = now + Duration::from_secs(1);
        let b = now + Duration::from_secs(2);
        assert_eq!(sooner(Some(a), Some(b)), Some(a));
        assert_eq!(sooner(Some(b), Some(a)), Some(a));
        assert_eq!(sooner(Some(a), None), Some(a));
        assert_eq!(sooner(None, Some(a)), Some(a));
        assert_eq!(sooner(None, None), None);
    }
}
