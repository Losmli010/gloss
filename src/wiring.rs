//! 组装段助手：平台能力装配（连接）、配置装配、四通道端点分发、启动期
//! 预检与事件源装配、异步运行时（tokio）消费桥与事件线程的启动。进程
//! 入口只留启动骨架，“拿到唤醒句柄之后才能做的事”与全部平台能力装配
//! 都收在这里；平台能力一律经装配句柄（Platform）取用，本模块是全入口
//! 唯一引用适配器层装配契约的地方。

use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use tokio::sync::mpsc::UnboundedReceiver;

use gloss_app::app::Waker;
use gloss_app::channel::{
    AcquireCommand, AppEndpoints, Channels, Command, Event, PlatformEvent, Traced,
};
use gloss_app::runtime::cache::TaskCache;
use gloss_app::runtime::pipeline::{CommandRuntime, TaskBridge, start_command_runtime};
use gloss_app::update::UpdateHandle;
use gloss_core::config::CACHE_TTL_MAX_SECS;
use gloss_core::config_handle::ConfigHandle;
use gloss_core::engine::AiTaskService;
use gloss_core::log::{debug, info, thread, warn};
use gloss_core::model::GlossError;
use gloss_core::ports::{ClipboardImageReader, SecretStore, TextAcquirer};
use gloss_core::task::TaskInput;
use gloss_platform::bootstrap::{EventSink, EventSources, Platform};

type StartupResult = Result<(), Box<dyn Error>>;

static DOCK_ICON_PNG: &[u8] = include_bytes!("../assets/icons/gloss-dock-icon.png");

pub(super) fn connect() -> Result<Platform, Box<dyn Error>> {
    Ok(Platform::connect()?)
}

type ConfigWiring = (Arc<ConfigHandle>, Arc<dyn SecretStore>);

pub(super) fn load_config(platform: &Platform) -> Result<ConfigWiring, Box<dyn Error>> {
    let handle = Arc::new(ConfigHandle::load_or_default(Arc::clone(
        &platform.config_store,
    )));
    Ok((handle, Arc::clone(&platform.secret_store)))
}

fn install_app_icon(platform: &Platform) {
    if let Err(err) = platform.app_icon.install(DOCK_ICON_PNG) {
        warn!(
            thread = thread::UI,
            error = ?err,
            "dock icon not applied, keeping the system default"
        );
    }
}

fn ensure_desktop_shortcut(platform: &Platform) {
    if let Err(err) = platform.desktop_shortcut.ensure() {
        warn!(
            thread = thread::UI,
            error = %err,
            "desktop shortcut not ensured, keeping the current installation layout"
        );
    }
}

fn preflight_accessibility(platform: &Platform) -> bool {
    if platform.permission.preflight() {
        info!(
            thread = thread::UI,
            "accessibility permission granted for selection reading"
        );
        true
    } else {
        warn!(
            thread = thread::UI,
            "accessibility permission missing, triggering the system guidance dialog"
        );
        platform.permission.request();
        false
    }
}

pub(super) fn preflight_and_tap(platform: &mut Platform) -> bool {
    let accessibility_ready = preflight_accessibility(platform);
    platform.spawn_gesture_source();
    if platform.gesture.is_some() {
        info!(
            thread = thread::UI,
            "event tap established for the selection gesture"
        );
    }
    accessibility_ready
}

pub(super) struct ChannelHalves {
    pub(super) platform_tx: Sender<PlatformEvent>,
    pub(super) acquire_rx: Receiver<Traced<AcquireCommand>>,
    pub(super) events_tx: Sender<Event>,
    pub(super) commands_rx: UnboundedReceiver<Traced<Command>>,
}

pub(super) fn distribute_channels() -> (AppEndpoints, ChannelHalves) {
    let gloss_app::channel::Channels {
        platform_events,
        acquire_commands,
        commands,
        events,
    } = Channels::new();
    let gloss_app::channel::CrossbeamPair {
        tx: platform_tx,
        rx: platform_rx,
    } = platform_events;
    let gloss_app::channel::CrossbeamPair {
        tx: acquire_tx,
        rx: acquire_rx,
    } = acquire_commands;
    let gloss_app::channel::CrossbeamPair {
        tx: events_tx,
        rx: events_rx,
    } = events;
    let gloss_app::channel::CommandChannel {
        tx: commands_tx,
        rx: commands_rx,
    } = commands;
    let endpoints = AppEndpoints {
        platform_events: platform_rx,
        acquire_commands: acquire_tx.clone(),
        commands: commands_tx,
        events: events_rx,
    };
    drop(acquire_tx);
    (
        endpoints,
        ChannelHalves {
            platform_tx,
            acquire_rx,
            events_tx,
            commands_rx,
        },
    )
}

pub(super) fn shared_task_cache(config: &ConfigHandle) -> Arc<TaskCache> {
    let ttl = Duration::from_secs(config.snapshot().cache_ttl_secs.min(CACHE_TTL_MAX_SECS));
    Arc::new(TaskCache::with_ttl(ttl))
}

pub(super) fn run_event_loop(
    mut platform: Platform,
    config: Arc<ConfigHandle>,
    store: Arc<dyn SecretStore>,
    service: Arc<AiTaskService>,
) -> StartupResult {
    let (endpoints, halves) = distribute_channels();
    let system_locale = platform.system_locale;
    info!(
        thread = thread::UI,
        locale = ?system_locale,
        "system language resolved"
    );
    let scene = Arc::clone(&platform.scene);
    let update_handle = gloss_app::update::start_once();
    let update = gloss_app::update::UpdateWiring::from_handle(update_handle);
    let accessibility_ready = preflight_and_tap(&mut platform);
    let permissions_ready = accessibility_ready;
    let clipboard_watch_enabled =
        Arc::new(AtomicBool::new(config.snapshot().watch_clipboard_images));
    let task_cache = shared_task_cache(&config);
    let mut assembly = Assembly {
        service,
        task_cache: Arc::clone(&task_cache),
        runtime_store: Arc::clone(&store),
        update_handle,
        accessibility_ready,
        clipboard_watch_enabled: Arc::clone(&clipboard_watch_enabled),
        command_runtime: None,
    };
    let result = gloss_app::app::run(
        endpoints,
        config,
        store,
        scene,
        system_locale,
        permissions_ready,
        update,
        clipboard_watch_enabled,
        task_cache,
        platform,
        |waker, platform| assembly.on_waker(waker, halves, platform),
    );

    drop(assembly.command_runtime);
    result
}

pub(super) struct Assembly {
    pub(super) service: Arc<AiTaskService>,
    pub(super) task_cache: Arc<TaskCache>,
    pub(super) runtime_store: Arc<dyn SecretStore>,
    pub(super) update_handle: &'static UpdateHandle,
    pub(super) accessibility_ready: bool,
    pub(super) clipboard_watch_enabled: Arc<AtomicBool>,
    pub(super) command_runtime: Option<CommandRuntime>,
}

impl Assembly {
    pub(super) fn on_waker(
        &mut self,
        waker: Waker,
        halves: ChannelHalves,
        platform: &mut Platform,
    ) -> StartupResult {
        let ChannelHalves {
            acquire_rx,
            platform_tx,
            events_tx,
            commands_rx,
        } = halves;
        install_app_icon(platform);
        ensure_desktop_shortcut(platform);
        let update_waker = waker.clone();
        self.update_handle.install_wake(move || {
            update_waker.wake_settings();
        });
        self.start_bridge(&events_tx, commands_rx, &waker)?;
        let sink = platform.event_sink(events_tx, platform_tx, {
            let waker = waker.clone();
            move || {
                waker.wake();
            }
        });
        self.start_event_thread(platform, sink, acquire_rx);
        gloss_core::log::milestone("05_assembly");
        Ok(())
    }

    fn start_bridge(
        &mut self,
        events_tx: &Sender<Event>,
        commands_rx: UnboundedReceiver<Traced<Command>>,
        waker: &Waker,
    ) -> Result<(), std::io::Error> {
        let bridge = TaskBridge::new(
            Arc::clone(&self.service),
            Arc::clone(&self.task_cache),
            Arc::clone(&self.runtime_store),
            events_tx.clone(),
            {
                let waker = waker.clone();
                move || {
                    waker.wake();
                }
            },
        );
        self.command_runtime = Some(start_command_runtime(bridge, commands_rx)?);
        Ok(())
    }

    fn start_event_thread(
        &mut self,
        platform: &mut Platform,
        sink: EventSink<Event, PlatformEvent>,
        acquire_rx: Receiver<Traced<AcquireCommand>>,
    ) {
        let kit = Platform::event_kit(Arc::clone(&self.clipboard_watch_enabled));
        let mut sources: EventSources<PlatformEvent> = Vec::new();

        let mut watch = platform.permission.watch(!self.accessibility_ready);
        sources.push(Box::new(move || {
            watch()
                .then_some(PlatformEvent::AccessibilityGranted)
                .into_iter()
                .collect()
        }));
        if let Some(mut gesture) = platform.gesture.take() {
            sources.push(Box::new(move || {
                gesture
                    .poll()
                    .into_iter()
                    .map(|pos| PlatformEvent::SelectionGesture { pos })
                    .collect()
            }));
        }
        let mut clipboard_watch = kit.clipboard_watcher;
        sources.push(Box::new(move || {
            clipboard_watch
                .poll()
                .into_iter()
                .map(|_| PlatformEvent::PasteboardImageObserved)
                .collect()
        }));
        let degraded = Arc::clone(&platform.gesture_degraded);
        let mut hinted = false;
        sources.push(Box::new(move || {
            if !hinted && degraded.load(Ordering::Relaxed) {
                hinted = true;
                warn!(
                    thread = thread::EVENT,
                    "mouse listener degraded mid-run, selection gesture disabled"
                );
            }
            Vec::new()
        }));
        let handler = acquire_command_handler(kit.text_acquirer, kit.clipboard_image_reader);
        platform.spawn_event_thread(acquire_rx, sink, handler, sources);
    }
}

fn acquire_command_handler(
    mut text_acquirer: Box<dyn TextAcquirer>,
    mut image_reader: Box<dyn ClipboardImageReader>,
) -> impl FnMut(Traced<AcquireCommand>, &EventSink<Event, PlatformEvent>) + Send {
    move |job, sink| {
        let _entered = job.span.enter();
        match job.payload {
            AcquireCommand::AcquireText { generation } => {
                acquire_text(&mut *text_acquirer, generation, sink);
            }
            AcquireCommand::AcquireClipboardImage { generation } => {
                acquire_clipboard_image(&mut *image_reader, generation, sink);
            }
        }
    }
}

fn acquire_text(
    text_acquirer: &mut dyn TextAcquirer,
    generation: u64,
    sink: &EventSink<Event, PlatformEvent>,
) {
    info!(thread = thread::EVENT, "acquiring text");
    let (outcome, settle) = text_acquirer.read().into_parts();
    match outcome {
        Ok(text) => {
            debug!(
                thread = thread::EVENT,
                bytes = text.len(),
                chars = text.chars().count(),
                "text input acquired"
            );
            sink.send_event(Event::InputReady {
                generation,
                input: TaskInput::Text { text },
            });
        }
        Err(err) => {
            if err == GlossError::AccessibilityDenied {
                warn!(
                    thread = thread::EVENT,
                    "accessibility permission missing, text acquisition denied"
                );
            } else {
                debug!(
                    thread = thread::EVENT,
                    error = %err,
                    "text acquisition failed"
                );
            }
            sink.send_event(Event::TaskFailed {
                generation,
                error: err,
            });
        }
    }
    settle.finish();
}

fn acquire_clipboard_image(
    image_reader: &mut dyn ClipboardImageReader,
    generation: u64,
    sink: &EventSink<Event, PlatformEvent>,
) {
    info!(thread = thread::EVENT, "acquiring clipboard image");
    match image_reader.read() {
        Ok(Some(png)) => {
            debug!(
                thread = thread::EVENT,
                bytes = png.len(),
                "clipboard image acquired"
            );
            sink.send_event(Event::InputReady {
                generation,
                input: TaskInput::Image { png },
            });
        }
        Ok(None) => {}
        Err(err) => {
            debug!(
                thread = thread::EVENT,
                error = %err,
                "clipboard image acquisition failed"
            );
            sink.send_event(Event::TaskFailed {
                generation,
                error: err,
            });
        }
    }
}
