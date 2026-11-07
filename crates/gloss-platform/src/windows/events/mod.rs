//! 平台事件线程：系统事件源的唯一宿主，以消息泵（Win32）加队列定时器抽干各源。
//!
//! 取材命令（通道2）与系统事件在同一线程顺序消费，天然串行无锁；鼠标
//! 事件监听这类自带消息泵宿主的源在自己的监听线程上运行、产物经通道汇入。
//! 线程启动序里初始化组件对象模型（COM）单线程套间（STA）——选区文本
//! 主路（用户界面自动化（UIA））要求套间固定且线程固定，读取调用全部
//! 发生在本线程（见选区模块头）。消息类型由组装点注入——平台层不依赖
//! 应用层的通道类型。事件源契约、事件汇点与节拍抽干等平台无关形状住
//! 事件线程骨架（event_thread），本模块只留消息泵宿主。

use crossbeam_channel::Receiver;

use gloss_core::log::{thread, warn};

use crate::event_thread::{TICK_MS, TickOutcome, tick};
use crate::ffi::com::ComApartment;
use crate::ffi::hook::{self, MSG, TickTimer, WM_TIMER};

pub mod mouse;

pub use crate::event_thread::{EventSink, EventSource, EventSources, EventThread};

const TIMER_ID: usize = 1;

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
    crate::event_thread::spawn(run_loop::<C, E, P, F>, commands, sink, on_command, sources)
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
    let Some(timer) = TickTimer::install(TIMER_ID, TICK_MS) else {
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
        if message.message == WM_TIMER
            && message.hwnd.is_invalid()
            && message.wParam.0 == timer.id()
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
}
