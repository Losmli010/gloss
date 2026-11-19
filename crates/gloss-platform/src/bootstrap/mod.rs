//! 装配契约：装配包装（Platform）、平台端口袋（PlatformPorts）、事件宿主
//! （EventHost）与事件源工厂的取材包（EventKit）、事件汇点的重新导出，
//! 平台无关——字段全部是端口契约对象与已解析的系统能力值。
//!
//! 平台装配只在进程入口的组装段构造一次（Platform::connect），随后一次性
//! 分发：事件线程消费的取材包由入口经静态工厂（EventHost::event_kit）构造
//! 并直接移交平台事件线程，不经宿主中转；手势源经宿主的装配
//! （spawn_gesture_source）建立后由事件源清单消费；端口袋按消费方分发——
//! 应用层只收它消费的端口对象（菜单、剪贴板写入），图标、桌面入口与权限
//! 预检这些只在入口使用的端口留在入口组装段，配置与密钥存储供配置装配
//! 取用。事件宿主由入口持有，在应用壳的事件循环返回后显式释放，析构即
//! 汇合事件线程——应用端点已随事件循环入口（run）返回而释放，事件线程见
//! 通道断连退出，汇合立即返回；退出顺序由入口代码显式表达。方向单一：
//! 入口 → 应用层，应用层不回触构造，本包不依赖应用层。
//!
//! 构造与工厂方法跨平台同形：各平台的同名镜像实现（组合存储、场景探针、
//! 系统语言、图标与各端口适配）经平台分派后重新导出的模块路径取用，构造
//! 体只写一份。手势监听不在构造期建立（授权对话框与监听线程的时点留在
//! 进程入口的既有的预检段），由事件宿主的装配（spawn_gesture_source）显式
//! 建立；建立失败时手势源保持缺席、降级标志留作事件源消费。
//!
//! 事件汇点（EventSink）与事件源清单类型（EventSources）是装配面的一部分
//! （事件源工厂的发送端与容器），随本模块重新导出，消费方经装配契约引用
//! 它们，不触平台目录。

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crossbeam_channel::{Receiver, Sender};

use gloss_core::model::{GlossError, Locale};
use gloss_core::ports::{
    AppIcon, ClipboardImageReader, ClipboardWatcher, ClipboardWriter, ConfigStore, DesktopShortcut,
    MenuController, PermissionProbe, SceneProbe, SecretStore, SelectionGestureSource, TextAcquirer,
};

pub use crate::events::{EventSink, EventSources};

pub struct Platform {
    pub ports: PlatformPorts,
    pub host: EventHost,
}

pub struct PlatformPorts {
    pub scene: Arc<dyn SceneProbe>,
    pub config_store: Arc<dyn ConfigStore>,
    pub secret_store: Arc<dyn SecretStore>,
    pub system_locale: Locale,
    pub app_icon: Box<dyn AppIcon>,
    pub desktop_shortcut: Box<dyn DesktopShortcut>,
    pub menu: Box<dyn MenuController>,
    pub clipboard: Box<dyn ClipboardWriter>,
    pub permission: Box<dyn PermissionProbe>,
}

pub struct EventHost {
    pub gesture: Option<Box<dyn SelectionGestureSource>>,
    pub gesture_degraded: Arc<AtomicBool>,
    event_thread: Option<crate::events::EventThread>,
}

pub struct EventKit {
    pub text_acquirer: Box<dyn TextAcquirer>,
    pub clipboard_watcher: Box<dyn ClipboardWatcher>,
    pub clipboard_image_reader: Box<dyn ClipboardImageReader>,
}

impl Platform {
    pub fn connect() -> Result<Self, GlossError> {
        let composite = Arc::new(crate::storage::CompositeConfigStore::new()?);
        Ok(Self {
            ports: PlatformPorts {
                config_store: Arc::clone(&composite) as Arc<dyn ConfigStore>,
                secret_store: composite,
                scene: Arc::new(crate::scene::SystemSceneProbe),
                system_locale: crate::locale::system_locale(),
                app_icon: Box::new(crate::appearance::SystemAppIcon::new()),
                desktop_shortcut: Box::new(crate::shortcut::SystemDesktopShortcut),
                menu: Box::new(crate::menu::SystemMenuController),
                clipboard: Box::new(crate::common::clipboard::SystemClipboardWriter),
                permission: Box::new(crate::permissions::SystemPermissionProbe),
            },
            host: EventHost {
                gesture: None,
                gesture_degraded: Arc::new(AtomicBool::new(false)),
                event_thread: None,
            },
        })
    }
}

impl EventHost {
    pub fn spawn_gesture_source(&mut self) {
        let (source, degraded) = crate::events::mouse::MouseSource::spawn();
        self.gesture = source.map(|source| Box::new(source) as Box<dyn SelectionGestureSource>);
        self.gesture_degraded = degraded;
    }

    pub fn event_kit(clipboard_watch_enabled: Arc<AtomicBool>) -> EventKit {
        let watch = crate::clipboard::ClipboardWatchSource::new(clipboard_watch_enabled);
        let round = watch.round_flag();
        let observer = watch.observer();
        EventKit {
            text_acquirer: Box::new(crate::selection::composite::CompositeReader::new(round)),
            clipboard_watcher: Box::new(watch),
            clipboard_image_reader: Box::new(crate::clipboard::SystemImageReader::new(observer)),
        }
    }

    pub fn event_sink<E, P>(
        &self,
        events: Sender<E>,
        platform: Sender<P>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> crate::events::EventSink<E, P> {
        crate::events::EventSink::new(events, platform, wake)
    }

    pub fn spawn_event_thread<C, E, P, F>(
        &mut self,
        commands: Receiver<C>,
        sink: crate::events::EventSink<E, P>,
        on_command: F,
        sources: crate::events::EventSources<P>,
    ) where
        C: Send + 'static,
        E: Send + 'static,
        P: Send + 'static,
        F: FnMut(C, &crate::events::EventSink<E, P>) + Send + 'static,
    {
        self.event_thread = Some(crate::event_thread::spawn(
            crate::events::run_loop::<C, E, P, F>,
            commands,
            sink,
            on_command,
            sources,
        ));
    }
}

impl Drop for EventHost {
    fn drop(&mut self) {
        if let Some(event_thread) = self.event_thread.take() {
            event_thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Platform;
    use crate::events::EventSink;
    use crossbeam_channel::unbounded;

    #[test]
    fn drop_joins_the_event_thread_once_command_senders_are_released() {
        let Platform { mut host, .. } = Platform::connect().unwrap();
        let (cmd_tx, cmd_rx) = unbounded::<u64>();
        let (ev_tx, ev_rx) = unbounded::<u64>();
        let (plat_tx, _plat_rx) = unbounded::<()>();
        host.spawn_event_thread(
            cmd_rx,
            EventSink::new(ev_tx, plat_tx, || {}),
            |command: u64, sink: &EventSink<u64, ()>| {
                sink.send_event(command);
            },
            Vec::new(),
        );
        cmd_tx.send(1).unwrap();
        assert_eq!(
            ev_rx.recv().unwrap(),
            1,
            "the thread must be consuming before the sender is released"
        );
        drop(cmd_tx);
        drop(host);
        assert!(ev_rx.try_recv().is_err(), "no events after the joined exit");
    }
}
