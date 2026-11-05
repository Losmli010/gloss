//! 平台事件线程：系统事件源的唯一宿主，以消息泵（Win32）加队列定时器抽干各源。
//!
//! 取材命令（通道2）与系统事件在同一线程顺序消费，天然串行无锁；鼠标
//! 事件监听这类自带消息泵宿主的源在自己的监听线程上运行、产物经通道汇入。
//! 线程启动序里初始化组件对象模型（COM）单线程套间（STA）——选区文本
//! 主路（用户界面自动化（UIA））要求套间固定且线程固定，读取调用全部
//! 发生在本线程（见选区模块头）。消息类型由组装点注入——平台层不依赖
//! 应用层的通道类型。

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender, TryRecvError};

use gloss_core::log::{debug, error, info, thread, warn};

use crate::ffi::com::ComApartment;
use crate::ffi::hook::{self, MSG, TickTimer, WM_TIMER};

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

const TICK_MS: u32 = 33;

const TIMER_ID: usize = 1;

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

enum TickOutcome {
    Continue,
    Exit,
}

fn tick<C, E, P, F>(
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

fn run_loop<C, E, P, F>(
    commands: Receiver<C>,
    sink: EventSink<E, P>,
    on_command: F,
    sources: EventSources<P>,
) where
    F: FnMut(C, &EventSink<E, P>),
{
    let _apartment = ComApartment::init_sta();
    let Some(_timer) = TickTimer::install(TIMER_ID, TICK_MS) else {
        warn!(
            thread = thread::EVENT,
            "failed to create tick timer, event thread exits"
        );
        return;
    };
    let mut state = (commands, sink, on_command, sources);
    loop {
        let mut message = MSG::default();
        if let hook::PumpFlow::Exit = hook::pump_once(&mut message) {
            break;
        }
        if message.message == WM_TIMER && message.hwnd.is_invalid() && message.wParam.0 == TIMER_ID
        {
            let (commands, sink, on_command, sources) = &mut state;
            if let TickOutcome::Exit = tick(commands, sink, on_command, sources) {
                break;
            }
        } else {
            hook::dispatch(&message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use crossbeam_channel::unbounded;

    struct TestCommand(u64);
    struct TestEvent(u64);

    type Sink = EventSink<TestEvent, ()>;
    type NoSources = EventSources<()>;

    fn echo(command: TestCommand, sink: &Sink) {
        sink.send_event(TestEvent(command.0));
    }

    fn no_sources() -> NoSources {
        Vec::new()
    }

    #[test]
    fn pump_tick_drains_commands_before_polling_sources() {
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
    fn pump_commands_are_consumed_in_order_then_thread_exits() {
        let (cmd_tx, cmd_rx) = unbounded::<TestCommand>();
        let (ev_tx, ev_rx) = unbounded::<TestEvent>();
        let (plat_tx, plat_rx) = unbounded::<()>();
        drop(plat_rx);

        let thread = spawn(
            cmd_rx,
            EventSink::new(ev_tx, plat_tx, || {}),
            echo,
            no_sources(),
        );
        for i in 0..16 {
            cmd_tx.send(TestCommand(i)).unwrap();
        }
        for i in 0..16 {
            assert_eq!(
                ev_rx.recv().unwrap().0,
                i,
                "commands must be consumed in order"
            );
        }
        drop(cmd_tx);
        thread.join();
        assert!(ev_rx.try_recv().is_err(), "no events after exit");
    }

    #[test]
    fn pump_late_commands_are_still_consumed() {
        let (cmd_tx, cmd_rx) = unbounded::<TestCommand>();
        let (ev_tx, ev_rx) = unbounded::<TestEvent>();
        let (plat_tx, plat_rx) = unbounded::<()>();
        drop(plat_rx);

        let thread = spawn(
            cmd_rx,
            EventSink::new(ev_tx, plat_tx, || {}),
            echo,
            no_sources(),
        );
        cmd_tx.send(TestCommand(1)).unwrap();
        assert_eq!(ev_rx.recv().unwrap().0, 1);

        std::thread::sleep(Duration::from_millis(60));
        cmd_tx.send(TestCommand(2)).unwrap();
        assert_eq!(ev_rx.recv().unwrap().0, 2);

        drop(cmd_tx);
        thread.join();
    }

    #[test]
    fn pump_sources_are_polled_and_forwarded() {
        let (cmd_tx, cmd_rx) = unbounded::<TestCommand>();
        let (ev_tx, ev_rx) = unbounded::<TestEvent>();
        let (plat_tx, plat_rx) = unbounded::<()>();
        let (src_tx, src_rx) = unbounded::<()>();

        let source: Box<dyn EventSource<()>> = Box::new(move || src_rx.try_iter().collect());
        let thread = spawn(
            cmd_rx,
            EventSink::new(ev_tx, plat_tx.clone(), || {}),
            echo,
            vec![source],
        );
        src_tx.send(()).unwrap();
        src_tx.send(()).unwrap();
        assert_eq!(plat_rx.recv().unwrap(), ());
        assert_eq!(plat_rx.recv().unwrap(), ());

        cmd_tx.send(TestCommand(7)).unwrap();
        assert_eq!(ev_rx.recv().unwrap().0, 7);

        drop(cmd_tx);
        drop(plat_tx);
        thread.join();
    }

    #[test]
    fn pump_panicking_handler_does_not_kill_thread() {
        let (cmd_tx, cmd_rx) = unbounded::<TestCommand>();
        let (ev_tx, ev_rx) = unbounded::<TestEvent>();
        let (plat_tx, plat_rx) = unbounded::<()>();
        drop(plat_rx);

        let thread = spawn(
            cmd_rx,
            EventSink::new(ev_tx, plat_tx, || {}),
            |command: TestCommand, sink: &Sink| {
                if command.0 == 0 {
                    panic!("poison");
                }
                sink.send_event(TestEvent(command.0));
            },
            no_sources(),
        );
        cmd_tx.send(TestCommand(0)).unwrap();
        cmd_tx.send(TestCommand(1)).unwrap();
        assert_eq!(ev_rx.recv().unwrap().0, 1, "thread must survive the panic");
        drop(cmd_tx);
        thread.join();
    }

    #[test]
    fn pump_panicking_source_does_not_kill_thread() {
        let (cmd_tx, cmd_rx) = unbounded::<TestCommand>();
        let (ev_tx, _ev_rx) = unbounded::<TestEvent>();
        let (plat_tx, plat_rx) = unbounded::<()>();
        let (ok_tx, ok_rx) = unbounded::<()>();
        let mut poisoned = true;

        let bad: Box<dyn EventSource<()>> = Box::new(move || {
            if poisoned {
                poisoned = false;
                panic!("poison");
            }
            ok_rx.try_iter().collect()
        });
        let thread = spawn(
            cmd_rx,
            EventSink::new(ev_tx, plat_tx.clone(), || {}),
            echo,
            vec![bad],
        );
        std::thread::sleep(Duration::from_millis(60));
        ok_tx.send(()).unwrap();
        assert_eq!(
            plat_rx.recv().unwrap(),
            (),
            "source must recover after panic"
        );

        drop(cmd_tx);
        drop(plat_tx);
        thread.join();
    }

    #[test]
    fn pump_sink_send_tolerates_closed_receivers() {
        let (ev_tx, ev_rx) = unbounded::<TestEvent>();
        let (plat_tx, plat_rx) = unbounded::<()>();
        drop(ev_rx);
        drop(plat_rx);
        let sink = EventSink::new(ev_tx, plat_tx, || {});
        assert!(!sink.send_event(TestEvent(1)));
        assert!(!sink.send_platform(()));
    }

    #[test]
    fn pump_successful_send_wakes_main_thread() {
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
