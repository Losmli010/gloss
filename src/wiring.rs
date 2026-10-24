//! 组装段助手：四通道端点分发、启动期预检与事件源装配、异步运行时
//! （tokio）消费桥与事件线程的启动。进程入口只留启动骨架与配置和
//! 服务装配，“拿到唤醒句柄之后才能做的事”都收在这里。

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
use gloss_core::ports::{AppIcon, ConfigStore};
use gloss_core::task::TaskInput;
use gloss_platform::appearance::MacAppIcon;
use gloss_platform::clipboard::{ClipboardImageReader, ClipboardWatchSource, WatchRoundFlag};
use gloss_platform::events::mouse::{MouseGesture, MouseSource};
use gloss_platform::events::{EventSink, EventSource, EventSources, EventThread};
use gloss_platform::permissions::{self, AccessibilityWatch};
use gloss_platform::selection::composite::CompositeReader;

type StartupResult = Result<(), Box<dyn Error>>;

static DOCK_ICON_PNG: &[u8] = include_bytes!("../assets/icons/gloss-dock-icon.png");

fn install_app_icon() {
    let icon: Box<dyn AppIcon> = Box::new(MacAppIcon::new());
    if !icon.install(DOCK_ICON_PNG) {
        warn!(
            thread = thread::UI,
            "dock icon not applied, keeping the system default"
        );
    }
}

fn preflight_accessibility() -> bool {
    if permissions::preflight_accessibility() {
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
        permissions::request_accessibility();
        false
    }
}

pub(super) fn preflight_and_tap() -> (bool, Option<MouseSource>, Arc<AtomicBool>) {
    let accessibility_ready = preflight_accessibility();
    let (mouse_source, mouse_degraded) = MouseSource::spawn();
    if mouse_source.is_some() {
        info!(
            thread = thread::UI,
            "event tap established for the selection gesture"
        );
    }
    (accessibility_ready, mouse_source, mouse_degraded)
}

pub(super) fn resolve_system_locale() -> gloss_core::model::Locale {
    let locale = gloss_platform::locale::system_locale();
    info!(
        thread = thread::UI,
        locale = ?locale,
        "system language resolved"
    );
    locale
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

pub(super) struct Assembly {
    pub(super) service: Arc<AiTaskService>,
    pub(super) cache_config: Arc<ConfigHandle>,
    pub(super) runtime_store: Arc<dyn ConfigStore>,
    pub(super) update_handle: &'static UpdateHandle,
    pub(super) accessibility_ready: bool,
    pub(super) mouse_source: Option<MouseSource>,
    pub(super) mouse_degraded: Arc<AtomicBool>,
    pub(super) clipboard_watch_enabled: Arc<AtomicBool>,
    pub(super) command_runtime: Option<CommandRuntime>,
    pub(super) event_thread: Option<EventThread>,
}

impl Assembly {
    pub(super) fn on_waker(&mut self, waker: Waker, halves: ChannelHalves) -> StartupResult {
        let ChannelHalves {
            acquire_rx,
            platform_tx,
            events_tx,
            commands_rx,
        } = halves;
        install_app_icon();
        let update_waker = waker.clone();
        self.update_handle.install_wake(move || {
            update_waker.wake_settings();
        });
        self.start_bridge(&events_tx, commands_rx, &waker)?;
        self.start_event_thread(acquire_rx, platform_tx, events_tx, &waker);
        gloss_core::log::milestone("05_assembly");
        Ok(())
    }

    fn start_bridge(
        &mut self,
        events_tx: &Sender<Event>,
        commands_rx: UnboundedReceiver<Traced<Command>>,
        waker: &Waker,
    ) -> Result<(), std::io::Error> {
        let ttl = Duration::from_secs(
            self.cache_config
                .snapshot()
                .cache_ttl_secs
                .min(CACHE_TTL_MAX_SECS),
        );
        let bridge = TaskBridge::new(
            Arc::clone(&self.service),
            Arc::new(TaskCache::with_ttl(ttl)),
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
        acquire_rx: Receiver<Traced<AcquireCommand>>,
        platform_tx: Sender<PlatformEvent>,
        events_tx: Sender<Event>,
        waker: &Waker,
    ) {
        let sink = EventSink::new(events_tx, platform_tx, {
            let waker = waker.clone();
            move || {
                waker.wake();
            }
        });
        let clipboard_watch = ClipboardWatchSource::new(Arc::clone(&self.clipboard_watch_enabled));
        let round_flag = clipboard_watch.round_flag();
        let image_reader = ClipboardImageReader::new(clipboard_watch.observer());
        self.event_thread = Some(gloss_platform::events::spawn(
            acquire_rx,
            sink,
            acquire_command_handler(image_reader, round_flag),
            event_sources(
                self.mouse_source.take(),
                Arc::clone(&self.mouse_degraded),
                self.accessibility_ready,
                clipboard_watch,
            ),
        ));
    }
}

fn event_sources(
    mouse_source: Option<MouseSource>,
    mouse_degraded: Arc<AtomicBool>,
    accessibility_ready: bool,
    clipboard_watch: ClipboardWatchSource,
) -> EventSources<PlatformEvent> {
    let mut sources: EventSources<PlatformEvent> = Vec::new();

    let mut watch = AccessibilityWatch::armed_if(!accessibility_ready);
    sources.push(Box::new(move || {
        watch
            .poll()
            .then_some(PlatformEvent::AccessibilityGranted)
            .into_iter()
            .collect()
    }));

    if let Some(mut source) = mouse_source {
        sources.push(Box::new(move || {
            source
                .poll()
                .into_iter()
                .map(|gesture| match gesture {
                    MouseGesture::Selection { pos } => PlatformEvent::SelectionGesture { pos },
                })
                .collect()
        }));
    }
    let mut clipboard_watch = clipboard_watch;
    sources.push(Box::new(move || {
        clipboard_watch
            .poll()
            .into_iter()
            .map(|_| PlatformEvent::PasteboardImageObserved)
            .collect()
    }));
    let mut hinted = false;
    sources.push(Box::new(move || {
        if !hinted && mouse_degraded.load(Ordering::Relaxed) {
            hinted = true;
            warn!(
                thread = thread::EVENT,
                "mouse listener degraded mid-run, selection gesture disabled"
            );
        }
        Vec::new()
    }));
    sources
}

fn acquire_command_handler(
    mut image_reader: ClipboardImageReader,
    round_flag: WatchRoundFlag,
) -> impl FnMut(Traced<AcquireCommand>, &EventSink<Event, PlatformEvent>) + Send {
    let mut reader = CompositeReader::new(round_flag);
    move |job, sink| {
        let _entered = job.span.enter();
        match job.payload {
            AcquireCommand::AcquireText { generation } => {
                acquire_text(&mut reader, generation, sink);
            }
            AcquireCommand::AcquireClipboardImage { generation } => {
                acquire_clipboard_image(&mut image_reader, generation, sink);
            }
        }
    }
}

fn acquire_text(
    reader: &mut CompositeReader,
    generation: u64,
    sink: &EventSink<Event, PlatformEvent>,
) {
    info!(thread = thread::EVENT, "acquiring text");
    let (outcome, settle) = reader.read().into_parts();
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
    image_reader: &mut ClipboardImageReader,
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
