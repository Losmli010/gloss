//! 窗口库（winit）事件分发：实现窗口库的事件分发协议，把窗口事件路由到渲染、
//! 通道消费与浮层显隐，并归并各窗口的唤醒时刻。

use std::error::Error;
use std::time::Instant;

use gloss_core::log::{debug, error, milestone, thread};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{Key, NamedKey};
use winit::window::WindowId;

use super::{GlossApp, UserEvent};

impl GlossApp {
    fn init(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Box<dyn Error>> {
        let theme = self.env.target_theme();
        let proxy = self.proxy.clone();
        self.workspace.init(event_loop, theme, move || {
            if let Some(proxy) = &proxy
                && proxy.send_event(UserEvent::FontsReady).is_err()
            {
                debug!(
                    thread = thread::FONTS,
                    "event loop gone before fonts ready, redraw skipped"
                );
            }
        })?;
        self.draw();
        Ok(())
    }
}

fn sooner(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

fn settle_due_repaint(deadline: &mut Option<Instant>, now: Instant, visible: bool) -> bool {
    let due = deadline.is_some_and(|at| at <= now);
    if due && !visible {
        *deadline = None;
    }
    due && visible
}

fn is_dismiss_key(logical_key: &Key, state: ElementState) -> bool {
    state == ElementState::Pressed && *logical_key == Key::Named(NamedKey::Escape)
}

impl ApplicationHandler<UserEvent> for GlossApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.workspace.is_ready() {
            return;
        }
        if let Err(err) = self.init(event_loop) {
            error!(thread = thread::UI, error = %err, "failed to start window and render stack");
            event_loop.exit();
            return;
        }
        self.draw();
        self.send_secret_prewarm();
        crate::ui::code_hl::warm_up();
        milestone("06_ready");
        crate::update::start_silent_check();
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        if matches!(event, UserEvent::FontsReady) {
            self.workspace.apply_theme_forced(self.env.target_theme());
            self.request_redraw();
            if let Some(windows) = &self.workspace.windows
                && windows.is_settings_visible()
            {
                windows.request_redraw_settings();
            }
            return;
        }
        if matches!(event, UserEvent::RedrawSettings) {
            if let Some(windows) = &self.workspace.windows
                && windows.is_settings_visible()
            {
                windows.request_redraw_settings();
            }
            return;
        }
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
        let (is_overlay, is_settings) = match &self.workspace.windows {
            Some(windows) => {
                let is_overlay = windows.matches_overlay(window_id);
                (
                    is_overlay,
                    !is_overlay && windows.matches_settings(window_id),
                )
            }
            None => return,
        };
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

        let repaint = self.egui_wants_repaint(is_settings, &event);
        if repaint {
            if is_settings {
                if let Some(windows) = &self.workspace.windows {
                    windows.request_redraw_settings();
                }
            } else {
                self.request_redraw();
            }
        }

        self.apply_window_state_event(event_loop, is_overlay, is_settings, event);
    }

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if !matches!(cause, StartCause::ResumeTimeReached { .. }) {
            return;
        }
        let now = Instant::now();
        let overlay_visible = self
            .workspace
            .windows
            .as_ref()
            .is_some_and(|windows| windows.is_visible());
        if settle_due_repaint(&mut self.workspace.overlay_repaint, now, overlay_visible) {
            self.request_redraw();
        }
        if self
            .workspace
            .settings_repaint
            .is_some_and(|deadline| deadline <= now)
            && let Some(windows) = &self.workspace.windows
        {
            windows.request_redraw_settings();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(
            sooner(
                self.workspace.overlay_repaint,
                self.workspace.settings_repaint,
            )
            .map_or(ControlFlow::Wait, ControlFlow::WaitUntil),
        );
    }
}

impl GlossApp {
    fn egui_wants_repaint(&mut self, is_settings: bool, event: &WindowEvent) -> bool {
        let frame = if is_settings {
            self.workspace.settings_frame.as_mut()
        } else {
            self.workspace.overlay_frame.as_mut()
        };
        match frame {
            Some(frame) => frame.egui.on_window_event(&frame.window, event).repaint,
            None => false,
        }
    }

    fn apply_window_state_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        is_overlay: bool,
        is_settings: bool,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                if is_settings {
                    self.close_settings();
                } else {
                    event_loop.exit();
                }
            }
            WindowEvent::KeyboardInput { event: key, .. } if is_overlay => {
                if is_dismiss_key(&key.logical_key, key.state) {
                    self.dismiss_overlay("escape key");
                }
            }
            WindowEvent::Resized(size) => {
                self.resize_window_surface(is_settings, size);
            }
            _ => {}
        }
    }

    fn resize_window_surface(&mut self, is_settings: bool, size: PhysicalSize<u32>) {
        let frame = if is_settings {
            self.workspace.settings_frame.as_mut()
        } else {
            self.workspace.overlay_frame.as_mut()
        };
        if let Some(frame) = frame {
            frame.surface.resize(size);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use winit::event::ElementState;
    use winit::keyboard::{Key, NamedKey};

    use super::{is_dismiss_key, settle_due_repaint, sooner};

    #[test]
    fn escape_press_is_the_dismiss_key() {
        assert!(is_dismiss_key(
            &Key::Named(NamedKey::Escape),
            ElementState::Pressed
        ));
        assert!(!is_dismiss_key(
            &Key::Named(NamedKey::Escape),
            ElementState::Released
        ));
        assert!(!is_dismiss_key(
            &Key::Character("a".into()),
            ElementState::Pressed
        ));
    }

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

    #[test]
    fn due_deadline_with_visible_overlay_requests_the_frame() {
        let t0 = Instant::now();
        let mut deadline = Some(t0);
        assert!(settle_due_repaint(
            &mut deadline,
            t0 + Duration::from_millis(16),
            true
        ));
        assert_eq!(deadline, Some(t0));
    }

    #[test]
    fn due_deadline_with_hidden_overlay_is_cleared_without_a_frame_request() {
        let t0 = Instant::now();
        let mut deadline = Some(t0);
        assert!(!settle_due_repaint(
            &mut deadline,
            t0 + Duration::from_millis(16),
            false
        ));
        assert_eq!(deadline, None);
    }

    #[test]
    fn pending_deadline_is_left_untouched() {
        let t0 = Instant::now();
        let future = t0 + Duration::from_secs(1);
        let mut deadline = Some(future);
        assert!(!settle_due_repaint(&mut deadline, t0, true));
        assert!(!settle_due_repaint(&mut deadline, t0, false));
        assert_eq!(deadline, Some(future));
    }

    #[test]
    fn missing_deadline_is_never_due() {
        let mut deadline = None;
        assert!(!settle_due_repaint(&mut deadline, Instant::now(), true));
        assert!(!settle_due_repaint(&mut deadline, Instant::now(), false));
        assert_eq!(deadline, None);
    }
}
