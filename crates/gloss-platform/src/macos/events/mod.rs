//! 平台事件线程：系统事件源的唯一宿主，以运行循环（CFRunLoop）加周期定时器抽干各源。
//!
//! 取材命令（通道2）与系统事件在同一线程顺序消费，天然串行无锁；鼠标
//! 事件监听这类自带运行循环的源在自己的监听线程上运行、产物经通道汇入。
//! 消息类型由组装点注入——平台层不依赖应用层的通道类型。事件源契约、
//! 事件汇点与节拍抽干等平台无关形状住事件线程骨架（event_thread），
//! 本模块只留运行循环宿主。

use std::ffi::c_void;
use std::time::Duration;

use crossbeam_channel::Receiver;

use gloss_core::log::{thread, warn};

use crate::event_thread::{TICK_MS, TickOutcome, tick};
use crate::ffi::cf::{
    CFAbsoluteTimeGetCurrent, CFRelease, CFRunLoopAddTimer, CFRunLoopGetCurrent, CFRunLoopRef,
    CFRunLoopRun, CFRunLoopStop, CFRunLoopTimerContext, CFRunLoopTimerCreate,
    CFRunLoopTimerInvalidate, CFRunLoopTimerRef, kCFAllocatorDefault, kCFRunLoopCommonModes,
};

pub mod mouse;

pub use crate::event_thread::{EventSink, EventSource, EventSources, EventThread};

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

struct LoopState<C, E, P, F> {
    commands: Receiver<C>,
    sink: EventSink<E, P>,
    on_command: F,
    sources: EventSources<P>,
    run_loop: CFRunLoopRef,
}

extern "C" fn fire<C, E, P, F>(_timer: CFRunLoopTimerRef, info: *mut c_void)
where
    F: FnMut(C, &EventSink<E, P>),
{
    // SAFETY: `info` is the `Box<LoopState>` pointer stored via
    // `CFRunLoopTimerContext.info` when the timer was created, passed back
    // verbatim by CF; the box is released only after `CFRunLoopRun` returns,
    // and CFRunLoop callbacks fire only serially on the host thread, so the
    // pointer is valid and has no concurrent aliasing.
    let state = unsafe { &mut *(info as *mut LoopState<C, E, P, F>) };
    if let TickOutcome::Exit = tick(
        &state.commands,
        &state.sink,
        &mut state.on_command,
        &mut state.sources,
    ) {
        // SAFETY: `run_loop` is this thread's own run loop (`CFRunLoopGetCurrent`
        // returns a +0 reference that is never released and stays valid forever);
        // `CFRunLoopStop` has no additional preconditions.
        unsafe { CFRunLoopStop(state.run_loop) };
    }
}

fn install_timer<C, E, P, F>(state: &mut LoopState<C, E, P, F>) -> Option<CFRunLoopTimerRef>
where
    F: FnMut(C, &EventSink<E, P>),
{
    let mut context = CFRunLoopTimerContext {
        version: 0,
        info: state as *mut LoopState<C, E, P, F> as *mut c_void,
        retain: None,
        release: None,
        copyDescription: None,
    };
    let interval = Duration::from_millis(u64::from(TICK_MS)).as_secs_f64();
    // SAFETY: all arguments are valid — the allocator is the default allocator,
    // `fire` matches the C callback signature, and `context` points to a valid
    // `CFRunLoopTimerContext` on this stack whose `info` points to a
    // `Box<LoopState>` that stays alive until the timer is invalidated and
    // released.
    let timer = unsafe {
        CFRunLoopTimerCreate(
            kCFAllocatorDefault,
            CFAbsoluteTimeGetCurrent() + interval,
            interval,
            0,
            0,
            fire::<C, E, P, F>,
            &mut context,
        )
    };
    if timer.is_null() {
        return None;
    }
    // SAFETY: `timer` has been checked non-null, `run_loop` is a valid run loop
    // for this thread, and the modes argument is a valid constant.
    unsafe { CFRunLoopAddTimer(state.run_loop, timer, kCFRunLoopCommonModes) };
    Some(timer)
}

fn run_loop<C, E, P, F>(
    commands: Receiver<C>,
    sink: EventSink<E, P>,
    on_command: F,
    sources: EventSources<P>,
) where
    F: FnMut(C, &EventSink<E, P>),
{
    let mut state = Box::new(LoopState {
        commands,
        sink,
        on_command,
        sources,
        // SAFETY: a pure query FFI with no preconditions; the current thread
        // always has a run loop, so this never returns NULL.
        run_loop: unsafe { CFRunLoopGetCurrent() },
    });
    let Some(timer) = install_timer(&mut state) else {
        warn!(
            thread = thread::EVENT,
            "failed to create run loop timer, event thread exits"
        );
        return;
    };
    // SAFETY: `timer` has been added to this thread's run loop; the timer keeps
    // firing callbacks until `CFRunLoopRun` returns, and the state its `info`
    // points to stays alive until the end of this function, so it never dangles.
    unsafe { CFRunLoopRun() };
    // SAFETY: `timer` holds the +1 reference returned by `CFRunLoopTimerCreate`;
    // at this point the run loop has been exited and we are not inside a
    // callback, so invalidating before releasing follows the CF timer cleanup
    // order.
    unsafe {
        CFRunLoopTimerInvalidate(timer);
        CFRelease(timer as *const c_void);
    }
    drop(state);
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
    fn commands_are_consumed_in_order_then_thread_exits() {
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
    fn late_commands_are_still_consumed() {
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
    fn sources_are_polled_and_forwarded() {
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
    fn panicking_handler_does_not_kill_thread() {
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
    fn panicking_source_does_not_kill_thread() {
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
