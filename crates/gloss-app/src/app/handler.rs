//! winit 事件分发：`ApplicationHandler` 实现，把窗口事件路由到渲染、
//! 通道消费与浮层显隐，并归并各窗口的唤醒时刻。

use std::error::Error;
use std::time::Instant;

use gloss_core::log::{debug, error, milestone, thread};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{Key, NamedKey};
use winit::window::WindowId;

use super::{GlossApp, UserEvent};

impl GlossApp {
    /// 建窗口栈与两个窗口的帧状态，一次做完（呈现资源收进 workspace）。
    /// 字体装载完成经 `UserEvent::FontsReady` 回主线程换表重绘。
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

/// 两个唤醒时刻里更早的那个；都没有则不用定时唤醒。
fn sooner(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// 到点的重绘截止（`deadline <= now`）的处置：可见时返回 true、由调用方
/// 请求重绘，截止留给随后的新帧刷新；隐藏时请求会被可见性门控丢弃，返回
/// false 并把截止一并清空——过期截止不清，[`ApplicationHandler::about_to_wait`]
/// 会一直 `WaitUntil` 过去时刻，定时器立即重触发且永远画不出那一帧，
/// 调度层空转烧满一核。
fn settle_due_repaint(deadline: &mut Option<Instant>, now: Instant, visible: bool) -> bool {
    let due = deadline.is_some_and(|at| at <= now);
    if due && !visible {
        *deadline = None;
    }
    due && visible
}

/// 浮层的收起键：Escape 按下（含重复）。键盘事件只会送进持焦窗口——
/// 浮层失焦后 Esc 天然不再生效；设置窗持焦时不收浮层。
fn is_dismiss_key(logical_key: &Key, state: ElementState) -> bool {
    state == ElementState::Pressed && *logical_key == Key::Named(NamedKey::Escape)
}

impl ApplicationHandler<UserEvent> for GlossApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // resumed 可能连续投递，渲染栈只起一次
        if self.workspace.is_ready() {
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
        // 密钥预热是否此刻发送由 send_secret_prewarm 按启动权限预检结论门
        // 控：权限就绪才读——keychain 授权框不得抢在系统权限引导弹窗之前，
        // 未就绪则推迟到首次任务自然重读。
        self.send_secret_prewarm();
        // 代码着色的语法集首次构造是百毫秒级，挪到后台线程，不占首个代码卡
        // 的首帧。
        crate::ui::code_hl::warm_up();
        milestone("06_ready");
        // 启动静默检查在此补发：更新客户端的信任链构建不挤占启动窗口。
        crate::update::start_silent_check();
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        if matches!(event, UserEvent::FontsReady) {
            // 字体装载完成：按当前主题强制重施加（含完整字体表）并重绘
            // 浮层。主题在这里统一施加，装载窗口内的偏好变更不会被后台
            // 快照覆盖。设置窗开着时同样要重绘：换表使排版缓存失效，
            // 不重绘它就停在内置字形的画面上直到下一次交互。
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
            // 相位迁移只牵动设置窗：窗口可见才请求重绘，不抽干主流程通道。
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
        let Some(windows) = &self.workspace.windows else {
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

        // 其余事件先喂给对应窗口的 egui，它决定是否消化掉以及要不要重绘；
        // 帧未建（设置窗首帧前）时没有可喂的对象，事件回落到下面的状态
        // 处理（关闭请求等不依赖帧）。
        let repaint = {
            let frame = if is_settings {
                self.workspace.settings_frame.as_mut()
            } else {
                self.workspace.overlay_frame.as_mut()
            };
            match frame {
                Some(frame) => frame.egui.on_window_event(&frame.window, &event).repaint,
                None => false,
            }
        };
        if repaint {
            if is_settings {
                windows.request_redraw_settings();
            } else {
                // 走带可见性门控的入口：隐藏浮层的事件驱动重绘是整帧空转。
                self.request_redraw();
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
            WindowEvent::KeyboardInput { event: key, .. } if is_overlay => {
                // egui_winit 已在同一事件上喂过 egui（输入框等自行消化）；
                // 壳只观察收起键，不拦截事件。
                if is_dismiss_key(&key.logical_key, key.state) {
                    self.dismiss_overlay("escape key");
                }
            }
            WindowEvent::Resized(size) => {
                let frame = if is_settings {
                    self.workspace.settings_frame.as_mut()
                } else {
                    self.workspace.overlay_frame.as_mut()
                };
                if let Some(frame) = frame {
                    frame.surface.resize(size);
                }
            }
            _ => {}
        }
    }

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if !matches!(cause, StartCause::ResumeTimeReached { .. }) {
            return;
        }
        // 到点的是 egui 要的下一帧：浮层与设置窗各自的截止时刻独立检查。
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
        // 没有待处理的唤醒时刻就彻底睡下，等窗口事件或唤醒句柄把自己叫醒
        event_loop.set_control_flow(
            sooner(
                self.workspace.overlay_repaint,
                self.workspace.settings_repaint,
            )
            .map_or(ControlFlow::Wait, ControlFlow::WaitUntil),
        );
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
