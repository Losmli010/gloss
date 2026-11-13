//! 窗口库（winit）事件循环：主线程的窗口生命周期与渲染驱动。
//!
//! 壳层按职责拆内部子模块，应用组合根与其余壳级编排留在本模块：
//! - 事件装配——事件循环入口与跨线程唤醒句柄；
//! - 事件分发——窗口库事件分发协议的实现。
//!
//! 通道消费与浮层显隐、动作执行在编排层（探测、任务、显隐、动作四段），
//! 渲染帧管线与呈现资源在呈现层，设置编辑会话在编排层的会话子模块。

mod events;
mod handler;

#[cfg(target_os = "macos")]
#[path = "loop_builder_macos.rs"]
mod loop_builder;
#[cfg(not(target_os = "macos"))]
#[path = "loop_builder_other.rs"]
mod loop_builder;

pub use events::{UserEvent, Waker, run};

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use gloss_core::config::Theme;
use gloss_core::config_handle::ConfigHandle;
use gloss_core::log::{Span, debug, error, info, thread, warn};
use gloss_core::model::Locale;
use gloss_core::ports::{SceneProbe, SecretStore};
use winit::dpi::LogicalSize;
use winit::event_loop::EventLoopProxy;

use gloss_platform::bootstrap::Platform;

use crate::channel::AppEndpoints;
use crate::flow::session::Session;
use crate::machine::{AppState, TaskStateMachine};
use crate::present::render::{render_frame, render_frame_with};
use crate::present::workspace::Workspace;
use crate::runtime::cache::TaskCache;
use crate::ui::i18n::Text;
use crate::ui::popup::OverlayResize;
use crate::ui::settings::{self, SettingsAction, SettingsState};
use crate::ui::style::geometry::window::OVERLAY_WIDTH;
use crate::update::UpdateWiring;

pub(crate) struct Env {
    pub(crate) config: Arc<ConfigHandle>,
    pub(crate) store: Arc<dyn SecretStore>,
    pub(crate) scene: Arc<dyn SceneProbe>,
    pub(crate) system_locale: Locale,
    pub(crate) permissions_ready: bool,
    pub(crate) update: UpdateWiring,
    pub(crate) clipboard_watch_enabled: Arc<AtomicBool>,
    pub(crate) task_cache: Arc<TaskCache>,
    pub(crate) platform: Platform,
}

impl Env {
    pub(crate) fn locale(&self) -> Locale {
        self.config.snapshot().language.resolve(self.system_locale)
    }

    pub(crate) fn target_theme(&self) -> Theme {
        self.config.snapshot().theme
    }
}

pub(crate) struct GlossApp {
    pub(crate) workspace: Workspace,
    pub(crate) endpoints: Option<AppEndpoints>,
    pub(crate) machine: TaskStateMachine,
    pub(crate) session: Session,
    pub(crate) env: Env,
    pub(crate) settings: Option<SettingsState>,
    pub(crate) proxy: Option<EventLoopProxy<UserEvent>>,
}

impl GlossApp {
    #[allow(
        clippy::too_many_arguments,
        reason = "组装点的主入口：每项都是不同关注点的注入端点，收敛成结构体只会把清单变成字段袋"
    )]
    fn new(
        endpoints: AppEndpoints,
        config: Arc<ConfigHandle>,
        store: Arc<dyn SecretStore>,
        scene: Arc<dyn SceneProbe>,
        system_locale: Locale,
        permissions_ready: bool,
        update: UpdateWiring,
        clipboard_watch_enabled: Arc<AtomicBool>,
        task_cache: Arc<TaskCache>,
        platform: Platform,
        proxy: Option<EventLoopProxy<UserEvent>>,
    ) -> Self {
        Self {
            workspace: Workspace::new(),
            endpoints: Some(endpoints),
            machine: TaskStateMachine::new(),
            session: Session::default(),
            env: Env {
                config,
                store,
                scene,
                system_locale,
                permissions_ready,
                update,
                clipboard_watch_enabled,
                task_cache,
                platform,
            },
            settings: None,
            proxy,
        }
    }

    pub(crate) fn span_for(&self, generation: u64) -> Option<Span> {
        self.session.span_for(generation)
    }

    fn install_app_menu(&mut self) {
        let Some(proxy) = self.proxy.clone() else {
            warn!(
                thread = thread::UI,
                "no user event proxy, app menu not installed; \
                 the process runs without a menu bar"
            );
            return;
        };
        let quit_proxy = proxy.clone();
        let installed = self.env.platform.menu.install(
            self.env.system_locale,
            Box::new(move || {
                if proxy.send_event(UserEvent::OpenSettings).is_err() {
                    debug!(
                        thread = thread::UI,
                        "event loop gone, menu settings click dropped"
                    );
                }
            }),
            Box::new(move || {
                if quit_proxy.send_event(UserEvent::Quit).is_err() {
                    debug!(
                        thread = thread::UI,
                        "event loop gone, menu quit click dropped"
                    );
                }
            }),
        );
        if installed {
            info!(
                thread = thread::UI,
                locale = ?self.env.system_locale,
                "app menu installed with a localized settings entry"
            );
        } else {
            warn!(
                thread = thread::UI,
                "app menu not installed and the default menu is disabled; \
                 the process runs without a menu bar"
            );
        }
    }

    fn draw(&mut self) {
        self.workspace.apply_theme(self.env.target_theme());
        let locale = self.env.locale();
        let Some(frame) = self.workspace.overlay_frame.as_mut() else {
            return;
        };
        let overlay_view = self.machine.overlay_view();
        let attached = self.machine.attached_image().cloned();
        let (repaint, output) = render_frame(frame, overlay_view, attached.as_ref(), locale);
        self.workspace.overlay_repaint = repaint;
        if let Some(action) = output.action {
            self.handle_overlay_action(action);
        }
        let Some(windows) = self.workspace.windows.as_mut() else {
            return;
        };
        if let Some(offset) = output.drag {
            windows.apply_overlay_drag((f64::from(offset.x), f64::from(offset.y)));
        }
        match output.resize {
            Some(OverlayResize::Press {
                edges,
                window,
                pointer,
            }) => {
                windows.begin_user_resize(edges, (window.x, window.y), (pointer.x, pointer.y));
            }
            Some(OverlayResize::Drag { edges, pointer }) => {
                windows.apply_user_resize(edges, (pointer.x, pointer.y));
            }
            Some(OverlayResize::Settle) => {
                windows.settle_user_resize();
            }
            None => {
                if let Some(sizing) = output.sizing {
                    let streaming = self.machine.state() == AppState::Translating;
                    windows.set_overlay_size(
                        LogicalSize::new(OVERLAY_WIDTH, f64::from(sizing.height)),
                        streaming,
                    );
                }
            }
        }
    }

    fn draw_settings(&mut self) {
        self.workspace.apply_theme(self.env.target_theme());
        let text = Text::get(self.env.locale());
        let update_state = self.env.update.receiver.borrow().clone();
        let Some(state) = self.settings.as_mut() else {
            return;
        };
        if let Err(err) = self
            .workspace
            .ensure_settings_frame(self.env.target_theme())
        {
            error!(
                thread = thread::UI,
                error = %err,
                "failed to build the settings frame, closing the settings window"
            );
            self.close_settings();
            return;
        }
        let Some(frame) = self.workspace.settings_frame.as_mut() else {
            return;
        };
        let (repaint, action) =
            render_frame_with(frame, |ui| settings::draw(ui, state, &update_state, text));
        self.workspace.settings_repaint = repaint;
        let Some(action) = action else {
            return;
        };
        match action {
            SettingsAction::Idle => {}
            SettingsAction::Save { config, key } => self.save_settings(*config, key),
            SettingsAction::Close => self.close_settings(),
            SettingsAction::Update(msg) => (self.env.update.send)(msg),
        }
    }

    pub(crate) fn request_redraw(&self) {
        self.workspace.request_redraw();
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use gloss_core::config::Config;
    use gloss_core::config_handle::ConfigHandle;
    use gloss_core::model::Locale;
    use gloss_core::model::ScreenPoint;
    use gloss_core::ports::{ConfigStore, SceneProbe, SecretStore};
    use gloss_core::task::TaskInput;

    use crate::channel::{AcquireCommand, AppEndpoints, Command, Event, PlatformEvent, Traced};
    use crate::machine::OverlayView;
    use crate::runtime::cache::TaskCache;
    use crate::stubs::ports::{MemoryConfigStore, StubSceneProbe, stub_platform};

    use super::GlossApp;

    pub(crate) fn update_wiring() -> crate::update::UpdateWiring {
        let (_tx, receiver) =
            tokio::sync::watch::channel(crate::update::state::UpdateState::default());
        crate::update::UpdateWiring {
            receiver,
            send: Arc::new(|_msg: crate::update::UpdateMsg| {}),
        }
    }

    pub(crate) struct DrivenApp {
        pub(crate) app: GlossApp,
        pub(crate) config: Arc<ConfigHandle>,
        pub(crate) store: Arc<dyn SecretStore>,
        pub(crate) pe_tx: crossbeam_channel::Sender<PlatformEvent>,
        pub(crate) ac_rx: crossbeam_channel::Receiver<Traced<AcquireCommand>>,
        pub(crate) cmd_rx: tokio::sync::mpsc::UnboundedReceiver<Traced<Command>>,
        #[allow(
            dead_code,
            reason = "通道④的驱动发送端：保留向应用回传事件的注入面，当前无测试使用"
        )]
        pub(crate) ev_tx: crossbeam_channel::Sender<Event>,
    }

    pub(crate) fn driven_app() -> DrivenApp {
        driven_app_with(Arc::new(MemoryConfigStore::default()))
    }

    pub(crate) fn driven_app_with(store: Arc<MemoryConfigStore>) -> DrivenApp {
        driven_app_with_scene(store, Arc::new(StubSceneProbe::default()))
    }

    pub(crate) fn driven_app_with_scene(
        store: Arc<MemoryConfigStore>,
        scene: Arc<dyn SceneProbe>,
    ) -> DrivenApp {
        driven_app_wired(store, scene, true)
    }

    pub(crate) fn driven_app_without_startup_permissions() -> DrivenApp {
        driven_app_wired(
            Arc::new(MemoryConfigStore::default()),
            Arc::new(StubSceneProbe::default()),
            false,
        )
    }

    fn driven_app_wired(
        store: Arc<MemoryConfigStore>,
        scene: Arc<dyn SceneProbe>,
        permissions_ready: bool,
    ) -> DrivenApp {
        let channels = crate::channel::Channels::new();
        let pe_tx = channels.platform_events.tx;
        let pe_rx = channels.platform_events.rx;
        let ac_tx = channels.acquire_commands.tx;
        let ac_rx = channels.acquire_commands.rx;
        let ev_tx = channels.events.tx;
        let ev_rx = channels.events.rx;
        let cmd_tx = channels.commands.tx;
        let cmd_rx = channels.commands.rx;
        let config = Arc::new(ConfigHandle::with_config(
            Arc::clone(&store) as Arc<dyn ConfigStore>,
            Config::default(),
        ));
        let clipboard_watch_enabled =
            Arc::new(AtomicBool::new(config.snapshot().watch_clipboard_images));
        let app = GlossApp::new(
            AppEndpoints {
                platform_events: pe_rx,
                acquire_commands: ac_tx,
                commands: cmd_tx,
                events: ev_rx,
            },
            Arc::clone(&config),
            Arc::clone(&store) as Arc<dyn SecretStore>,
            Arc::clone(&scene),
            Locale::Zh,
            permissions_ready,
            update_wiring(),
            clipboard_watch_enabled,
            Arc::new(TaskCache::new()),
            stub_platform(&store, scene),
            None,
        );
        DrivenApp {
            app,
            config,
            store,
            pe_tx,
            ac_rx,
            cmd_rx,
            ev_tx,
        }
    }

    pub(crate) fn trigger_selection(
        app: &mut GlossApp,
        pe_tx: &crossbeam_channel::Sender<PlatformEvent>,
    ) {
        pe_tx
            .send(PlatformEvent::SelectionGesture {
                pos: ScreenPoint::new(0, 0),
            })
            .unwrap();
        app.drain_platform_events();
    }

    pub(crate) fn text_input(text: &str) -> TaskInput {
        TaskInput::Text { text: text.into() }
    }

    pub(crate) fn plain_outcome(note: &str) -> gloss_core::task::TaskOutcome {
        gloss_core::task::TaskOutcome {
            kind: gloss_core::task::TaskKind::TranslateWord,
            note: note.into(),
            structured: gloss_core::task::OutcomeStructured::Plain {
                examples: Vec::new(),
            },
            degraded: false,
            lang_violations: Vec::new(),
        }
    }

    pub(crate) fn streaming_raw(app: &GlossApp) -> &str {
        match app.machine.overlay_view() {
            Some(OverlayView::Streaming { raw, .. }) => raw,
            other => panic!("expected streaming view, got {other:?}"),
        }
    }

    pub(crate) fn outcome_note(app: &GlossApp) -> &str {
        match app.machine.overlay_view() {
            Some(OverlayView::Outcome { outcome, .. }) => &outcome.note,
            other => panic!("expected outcome view, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use gloss_core::config::{Config, Language};
    use gloss_core::model::Locale;

    use super::test_support::{DrivenApp, driven_app};

    #[test]
    fn ui_locale_follows_the_saved_language_without_a_restart() {
        let DrivenApp {
            app,
            config,
            ac_rx: _ac_rx,
            cmd_rx: _cmd_rx,
            ..
        } = driven_app();
        assert_eq!(
            app.env.locale(),
            Locale::Zh,
            "the factory default follows the system locale"
        );

        config
            .save(Config {
                language: Language::En,
                ..Default::default()
            })
            .expect("save should succeed");
        assert_eq!(
            app.env.locale(),
            Locale::En,
            "a saved language must drive the next frame's table, no restart needed"
        );

        config
            .save(Config {
                language: Language::System,
                ..Default::default()
            })
            .expect("save should succeed");
        assert_eq!(
            app.env.locale(),
            Locale::Zh,
            "System resolves through the startup system locale"
        );
    }
}
