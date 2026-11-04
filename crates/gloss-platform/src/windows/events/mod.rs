//! 平台事件线程（系统（Windows）占位）：事件搬运机制的类型面镜像。
//!
//! 事件源契约、汇点与线程句柄是平台无关形状，与系统（macOS）实现同名
//! 同签名；运行循环宿主在系统（macOS）跑运行循环（CFRunLoop）、系统
//! （Windows）跑消息泵（Win32）（移植方案后续阶段补齐），本桩启动期即
//! 报告未支持，不拉起事件线程。

use std::sync::Arc;
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender};

use gloss_core::log::{debug, error, thread, warn};

pub mod mouse;

pub trait EventSource<P>: Send {
    fn poll(&mut self) -> Vec<P>;
}

impl<P, F> EventSource<P> for F
where
    F: FnMut() -> Vec<P> + Send,
{
    fn poll(&mut self) -> Vec<P> {
        self()
    }
}

pub type EventSources<P> = Vec<Box<dyn EventSource<P> + Send>>;

pub struct EventSink<E, P> {
    events: Sender<E>,
    platform: Sender<P>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl<E, P> EventSink<E, P> {
    pub fn new(
        events: Sender<E>,
        platform: Sender<P>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        Self {
            events,
            platform,
            wake: Arc::new(wake),
        }
    }

    pub fn send_event(&self, event: E) -> bool {
        match self.events.send(event) {
            Ok(()) => {
                self.notify();
                true
            }
            Err(_) => {
                debug!(thread = thread::EVENT, "event receiver gone, event dropped");
                false
            }
        }
    }

    pub fn send_platform(&self, event: P) -> bool {
        match self.platform.send(event) {
            Ok(()) => {
                self.notify();
                true
            }
            Err(_) => {
                debug!(
                    thread = thread::EVENT,
                    "platform receiver gone, event dropped"
                );
                false
            }
        }
    }

    fn notify(&self) {
        (self.wake)();
    }
}

pub struct EventThread {
    join: Option<JoinHandle<()>>,
}

impl EventThread {
    pub fn join(mut self) {
        if let Some(join) = self.join.take()
            && join.join().is_err()
        {
            error!(thread = thread::EVENT, "platform event thread died");
        }
    }
}

pub fn spawn<C, E, P, F>(
    _commands: Receiver<C>,
    _sink: EventSink<E, P>,
    _on_command: F,
    _sources: EventSources<P>,
) -> EventThread
where
    C: Send + 'static,
    E: Send + 'static,
    P: Send + 'static,
    F: FnMut(C, &EventSink<E, P>) + Send + 'static,
{
    warn!(
        thread = thread::EVENT,
        "platform event thread is not implemented on this platform yet"
    );
    EventThread { join: None }
}
