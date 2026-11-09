//! 装配契约：平台装配句柄（Platform）、事件源工厂的取材包（EventKit）
//! 与事件汇点的重新导出，平台无关——字段全部是端口契约对象与已解析的
//! 系统能力值。
//!
//! 平台句柄只在进程入口的组装段构造一次（Platform::connect），随后一次
//! 性移交：入口先把事件线程消费的取材包与手势源从句柄上取走并移交平台
//! 事件线程，再把句柄本体交给应用层（gloss-app）持有，应用层的菜单装配
//! 与剪贴板写入都从句柄取。方向单一：入口 → 应用层，应用层不回触构造，
//! 本包不依赖应用层。
//!
//! 构造与工厂方法跨平台同形：各平台的同名镜像实现（组合存储、场景探针、
//! 系统语言、图标与各端口适配）经平台分派后重新导出的模块路径取用，构造
//! 体只写一份。手势监听不在构造期建立（授权对话框与监听线程的时点留在
//! 进程入口的既有的预检段），由装配句柄的手势源装配（spawn_gesture_source）
//! 显式建立；建立失败时手势源保持缺席、降级标志留作事件源消费。
//!
//! 事件线程句柄由句柄本体持有，拉起后随句柄释放而汇合——
//! 通道接收端先于句柄释放，事件线程随之退出，汇合立即返回。事件汇点
//! （EventSink）与事件源清单类型（EventSources）是装配面的一部分（事件
//! 源工厂的发送端与容器），随本模块重新导出，消费方经装配契约引用它们，
//! 不触平台目录。

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
    pub scene: Arc<dyn SceneProbe>,
    pub config_store: Arc<dyn ConfigStore>,
    pub secret_store: Arc<dyn SecretStore>,
    pub system_locale: Locale,
    pub app_icon: Box<dyn AppIcon>,
    pub desktop_shortcut: Box<dyn DesktopShortcut>,
    pub menu: Box<dyn MenuController>,
    pub clipboard: Box<dyn ClipboardWriter>,
    pub permission: Box<dyn PermissionProbe>,
    pub gesture: Option<Box<dyn SelectionGestureSource>>,
    pub gesture_degraded: Arc<AtomicBool>,
    pub events: Option<EventKit>,
    pub event_thread: Option<crate::events::EventThread>,
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
            config_store: Arc::clone(&composite) as Arc<dyn ConfigStore>,
            secret_store: composite,
            scene: Arc::new(crate::scene::SystemSceneProbe),
            system_locale: crate::locale::system_locale(),
            app_icon: Box::new(crate::appearance::SystemAppIcon::new()),
            desktop_shortcut: Box::new(crate::shortcut::SystemDesktopShortcut),
            menu: Box::new(crate::menu::SystemMenuController),
            clipboard: Box::new(crate::clipboard::SystemClipboardWriter),
            permission: Box::new(crate::permissions::SystemPermissionProbe),
            gesture: None,
            gesture_degraded: Arc::new(AtomicBool::new(false)),
            events: None,
            event_thread: None,
        })
    }

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
        self.event_thread = Some(crate::events::spawn(commands, sink, on_command, sources));
    }
}

impl Drop for Platform {
    fn drop(&mut self) {
        if let Some(event_thread) = self.event_thread.take() {
            event_thread.join();
        }
    }
}
