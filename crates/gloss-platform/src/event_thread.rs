//! 平台事件线程的平台无关骨架：事件源契约、事件汇点、线程句柄与节拍抽干。
//!
//! 系统（macOS）与系统（Windows）的事件模块都从本模块取共享形状，各自只
//! 留运行循环宿主差异：前者以运行循环（CFRunLoop）加周期定时器驱动，后者
//! 以消息泵（Win32）加线程队列定时器驱动。线程名、节拍周期与抽干顺序
//! （通道2命令先抽干、事件源随后轮询）在此统一；命令处理器与事件源轮询
//! 的恐慌屏障随抽干循环同住本模块。

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender, TryRecvError};

use gloss_core::log::{debug, error, info, thread, warn};

pub(crate) const TICK_MS: u32 = 33;

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

type RunLoop<C, E, P, F> = fn(Receiver<C>, EventSink<E, P>, F, EventSources<P>);

pub(crate) fn spawn<C, E, P, F>(
    run_loop: RunLoop<C, E, P, F>,
    commands: Receiver<C>,
    sink: EventSink<E, P>,
    on_command: F,
    sources: EventSources<P>,
) -> EventThread
where
    C: Send + 'static,
    E: Send + 'static,
    P: Send + 'static,
    F: FnMut(C, &EventSink<E, P>) + Send + 'static,
{
    info!(thread = thread::EVENT, "spawning platform event thread");
    let spawned = std::thread::Builder::new()
        .name("gloss-event".into())
        .spawn(move || {
            run_loop(commands, sink, on_command, sources);
            info!(thread = thread::EVENT, "platform event thread stopped");
        });
    match spawned {
        Ok(join) => EventThread { join: Some(join) },
        Err(err) => {
            warn!(thread = thread::EVENT, error = %err, "failed to spawn platform event thread");
            EventThread { join: None }
        }
    }
}

fn guarded<R>(what: &str, f: impl FnOnce() -> R) -> Option<R> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => Some(value),
        Err(_) => {
            error!(
                thread = thread::EVENT,
                what, "callback panicked; event thread survives"
            );
            None
        }
    }
}

pub(crate) enum TickOutcome {
    Continue,
    Exit,
}

pub(crate) fn tick<C, E, P, F>(
    commands: &Receiver<C>,
    sink: &EventSink<E, P>,
    on_command: &mut F,
    sources: &mut [Box<dyn EventSource<P> + Send>],
) -> TickOutcome
where
    F: FnMut(C, &EventSink<E, P>),
{
    loop {
        match commands.try_recv() {
            Ok(command) => {
                let _ = guarded("acquire command handler", || on_command(command, sink));
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => return TickOutcome::Exit,
        }
    }
    for source in sources {
        let Some(events) = guarded("event source poll", || source.poll()) else {
            continue;
        };
        for event in events {
            let _ = guarded("platform event send", || sink.send_platform(event));
        }
    }
    TickOutcome::Continue
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use crossbeam_channel::unbounded;

    struct TestCommand(u64);
    struct TestEvent(u64);

    type Sink = EventSink<TestEvent, ()>;

    #[test]
    fn tick_drains_commands_before_polling_sources() {
        let order = Arc::new(Mutex::new(Vec::new()));
        let command_order = Arc::clone(&order);
        let source_order = Arc::clone(&order);
        let (cmd_tx, cmd_rx) = unbounded::<TestCommand>();
        let (ev_tx, ev_rx) = unbounded::<TestEvent>();
        let (plat_tx, _plat_rx) = unbounded::<()>();
        let mut on_command = move |command: TestCommand, sink: &Sink| {
            command_order
                .lock()
                .unwrap()
                .push(format!("command {}", command.0));
            sink.send_event(TestEvent(command.0));
        };
        let mut sources: EventSources<()> = vec![Box::new(move || {
            source_order.lock().unwrap().push("poll".to_owned());
            Vec::new()
        })];
        cmd_tx.send(TestCommand(1)).unwrap();
        cmd_tx.send(TestCommand(2)).unwrap();
        let sink = EventSink::new(ev_tx, plat_tx, || {});
        assert!(matches!(
            tick(&cmd_rx, &sink, &mut on_command, &mut sources),
            TickOutcome::Continue
        ));
        assert_eq!(
            *order.lock().unwrap(),
            ["command 1", "command 2", "poll"],
            "the whole command queue must drain before the first source poll"
        );
        assert_eq!(ev_rx.recv().unwrap().0, 1);
        assert_eq!(ev_rx.recv().unwrap().0, 2);
        assert!(ev_rx.try_recv().is_err(), "one event per command");
    }

    #[test]
    fn sink_send_tolerates_closed_receivers() {
        let (ev_tx, ev_rx) = unbounded::<TestEvent>();
        let (plat_tx, plat_rx) = unbounded::<()>();
        drop(ev_rx);
        drop(plat_rx);
        let sink = EventSink::new(ev_tx, plat_tx, || {});
        assert!(!sink.send_event(TestEvent(1)));
        assert!(!sink.send_platform(()));
    }

    #[test]
    fn successful_send_wakes_main_thread() {
        let (ev_tx, ev_rx) = unbounded::<TestEvent>();
        let (plat_tx, plat_rx) = unbounded::<()>();
        let (wake_tx, wake_rx) = unbounded::<()>();
        let sink = EventSink::new(ev_tx, plat_tx, move || {
            wake_tx.send(()).unwrap();
        });
        assert!(sink.send_event(TestEvent(1)));
        assert_eq!(
            wake_rx.recv().unwrap(),
            (),
            "send must wake the main thread"
        );

        drop(ev_rx);
        drop(plat_rx);
        assert!(!sink.send_event(TestEvent(2)));
        assert!(wake_rx.try_recv().is_err(), "failed send must not wake");
    }
}
