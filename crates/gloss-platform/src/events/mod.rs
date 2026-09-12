//! 平台事件线程：全局热键、鼠标监听等系统事件源的唯一宿主（08 §7.2）。
//!
//! 事件线程必须是 RunLoop 线程而非裸 `std::thread`：线程宿主为 CFRunLoop，
//! 事件源（定时器与后续 CGEventTap source）都挂同一 run loop。取材命令
//! （通道②）与系统事件在同一线程顺序消费，天然串行无锁。消息类型由组装点
//! 注入——platform 不依赖 gloss-app 的通道类型，测试用本地桩类型即可驱动
//! 整条循环。

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, TryRecvError};

use gloss_core::log::{debug, error, info, thread, warn};

pub mod hotkey;
pub mod mouse;

/// 所有事件源（热键泵、鼠标手势等）每轮 tick 抽干一次，产出 ① 的载荷。
/// 组装点用闭包把 platform 本地类型映射为真实的平台事件，映射闭包与命令
/// 处理器同等对待——panic 不允许带倒事件线程。
pub trait EventSource<P>: Send {
    /// 抽干一轮，返回本轮产出的平台事件载荷。
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

/// 事件线程挂载的事件源集合。
pub type EventSources<P> = Vec<Box<dyn EventSource<P> + Send>>;

/// 源事件的 drain 周期，全平台一致：run loop 无法阻塞等待 crossbeam 通道，
/// 非 macOS 的 `recv_timeout` 也复用同一节奏。热键/手势到浮层的端到端延迟
/// 上界即此值，33ms 低于可感知阈值。
const TICK: Duration = Duration::from_millis(33);

/// 事件线程的产物出口：④ 回传事件与 ① 平台事件，由组装点接上真实通道。
pub struct EventSink<E, P> {
    events: Sender<E>,
    platform: Sender<P>,
}

impl<E, P> EventSink<E, P> {
    pub fn new(events: Sender<E>, platform: Sender<P>) -> Self {
        Self { events, platform }
    }

    /// 发送 ④ 回传事件；返回 `false` 表示主线程已退出，消息被丢弃。
    pub fn send_event(&self, event: E) -> bool {
        match self.events.send(event) {
            Ok(()) => true,
            Err(_) => {
                debug!(thread = thread::EVENT, "event receiver gone, event dropped");
                false
            }
        }
    }

    /// 发送 ① 平台事件；返回 `false` 表示主线程已退出，消息被丢弃。
    pub fn send_platform(&self, event: P) -> bool {
        match self.platform.send(event) {
            Ok(()) => true,
            Err(_) => {
                debug!(
                    thread = thread::EVENT,
                    "platform receiver gone, event dropped"
                );
                false
            }
        }
    }
}

/// 平台事件线程句柄；`join` 等待线程退出。
pub struct EventThread {
    join: Option<JoinHandle<()>>,
}

impl EventThread {
    /// 等待事件线程退出。退出由调用方触发：drop 通道②的 Sender 即可。
    pub fn join(mut self) {
        if let Some(join) = self.join.take()
            && join.join().is_err()
        {
            // 防护齐备时线程 panic 近乎不可能，但一旦发生必须留痕，
            // 否则事件线程死了应用无任何信号。
            error!(thread = thread::EVENT, "platform event thread died");
        }
    }
}

/// 启动平台事件线程。
///
/// `on_command` 在事件线程上顺序消费通道②（取材接线前可先挂测试桩）；
/// `sources` 是挂载到本线程的事件源（热键、鼠标手势……），每个 tick 抽干
/// 一轮。退出协议：调用方 drop 通道② Sender，线程抽干剩余命令后自行结束，
/// `join` 返回即线程已终止。
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
            match sources {
                #[cfg(target_os = "macos")]
                sources => run_loop(commands, sink, on_command, sources),
                #[cfg(not(target_os = "macos"))]
                sources => tick_loop(commands, sink, on_command, sources),
            }
            info!(thread = thread::EVENT, "platform event thread stopped");
        });
    match spawned {
        Ok(join) => EventThread { join: Some(join) },
        // 线程缺失时应用仍能启动，但热键/取材全部失效——必须留痕。
        Err(err) => {
            warn!(thread = thread::EVENT, error = %err, "failed to spawn platform event thread");
            EventThread { join: None }
        }
    }
}

/// 应用代码注入的回调不允许把 panic 带进事件线程；捕获后循环继续服务。
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

/// 一轮消费：先抽干通道②（出现 Disconnected 即退出信号），再抽干全部
/// 事件源。两条驱动路径（macOS RunLoop 定时器 / 其余平台 recv_timeout）
/// 共用，保证「与系统事件同线程顺序处理」的语义只有一份实现。
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
            // 通道② Sender 全部 drop：事件线程使命结束。
            Err(TryRecvError::Disconnected) => return TickOutcome::Exit,
        }
    }
    for source in sources {
        let Some(events) = guarded("event source poll", || source.poll()) else {
            continue;
        };
        for event in events {
            sink.send_platform(event);
        }
    }
    TickOutcome::Continue
}

/// 非 macOS 的驱动：命令到达即刻唤醒，源事件按 TICK 节奏抽干。
#[cfg(not(target_os = "macos"))]
fn tick_loop<C, E, P, F>(
    commands: Receiver<C>,
    sink: EventSink<E, P>,
    mut on_command: F,
    mut sources: EventSources<P>,
) where
    F: FnMut(C, &EventSink<E, P>),
{
    use crossbeam_channel::RecvTimeoutError;

    loop {
        match commands.recv_timeout(TICK) {
            Ok(command) => {
                let _ = guarded("acquire command handler", || on_command(command, &sink));
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if let TickOutcome::Exit = tick(&commands, &sink, &mut on_command, &mut sources) {
            break;
        }
    }
}

/// macOS 的驱动：RunLoop 不能阻塞在 crossbeam 上（08 §7.2），挂一个周期
/// 定时器执行与 [`tick`] 相同的一轮消费；后续 CGEventTap 等事件源也挂同一
/// run loop。
#[cfg(target_os = "macos")]
fn run_loop<C, E, P, F>(
    commands: Receiver<C>,
    sink: EventSink<E, P>,
    on_command: F,
    sources: EventSources<P>,
) where
    F: FnMut(C, &EventSink<E, P>),
{
    use std::ffi::c_void;

    use core_foundation_sys::base::{CFRelease, kCFAllocatorDefault};
    use core_foundation_sys::date::CFAbsoluteTimeGetCurrent;
    use core_foundation_sys::runloop::{
        CFRunLoopAddTimer, CFRunLoopGetCurrent, CFRunLoopRun, CFRunLoopStop, CFRunLoopTimerContext,
        CFRunLoopTimerCreate, CFRunLoopTimerInvalidate, CFRunLoopTimerRef, kCFRunLoopCommonModes,
    };

    struct LoopState<C, E, P, F> {
        commands: Receiver<C>,
        sink: EventSink<E, P>,
        on_command: F,
        sources: EventSources<P>,
        run_loop: core_foundation_sys::runloop::CFRunLoopRef,
    }

    extern "C" fn fire<C, E, P, F>(_timer: CFRunLoopTimerRef, info: *mut c_void)
    where
        F: FnMut(C, &EventSink<E, P>),
    {
        let state = unsafe { &mut *(info as *mut LoopState<C, E, P, F>) };
        if let TickOutcome::Exit = tick(
            &state.commands,
            &state.sink,
            &mut state.on_command,
            &mut state.sources,
        ) {
            // 通道② Sender 全部 drop：停掉 RunLoop，CFRunLoopRun 返回后线程结束。
            unsafe { CFRunLoopStop(state.run_loop) };
        }
    }

    let mut state = Box::new(LoopState {
        commands,
        sink,
        on_command,
        sources,
        run_loop: unsafe { CFRunLoopGetCurrent() },
    });
    let mut context = CFRunLoopTimerContext {
        version: 0,
        info: state.as_mut() as *mut LoopState<C, E, P, F> as *mut c_void,
        retain: None,
        release: None,
        copyDescription: None,
    };
    let interval = TICK.as_secs_f64();
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
        warn!(
            thread = thread::EVENT,
            "failed to create run loop timer, event thread exits"
        );
        return;
    }
    unsafe {
        CFRunLoopAddTimer(state.run_loop, timer, kCFRunLoopCommonModes);
        CFRunLoopRun();
    }
    // RunLoop 已停止且不在回调中：先 invalidate 再释放定时器（CF timer 的
    // 规范清理步骤），最后收回状态盒。
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

    /// platform 不依赖 gloss-app：本地桩类型足以驱动整条循环。
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

    /// 通道②按顺序消费、产物按顺序到达；drop Sender 后线程自行退出。
    #[test]
    fn commands_are_consumed_in_order_then_thread_exits() {
        let (cmd_tx, cmd_rx) = unbounded::<TestCommand>();
        let (ev_tx, ev_rx) = unbounded::<TestEvent>();
        let (plat_tx, plat_rx) = unbounded::<()>();
        drop(plat_rx);

        let thread = spawn(cmd_rx, EventSink::new(ev_tx, plat_tx), echo, no_sources());
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

    /// 初始抽干之后线程仍持续轮询：晚到的命令也能被消费。
    #[test]
    fn late_commands_are_still_consumed() {
        let (cmd_tx, cmd_rx) = unbounded::<TestCommand>();
        let (ev_tx, ev_rx) = unbounded::<TestEvent>();
        let (plat_tx, plat_rx) = unbounded::<()>();
        drop(plat_rx);

        let thread = spawn(cmd_rx, EventSink::new(ev_tx, plat_tx), echo, no_sources());
        cmd_tx.send(TestCommand(1)).unwrap();
        assert_eq!(ev_rx.recv().unwrap().0, 1);

        // 等过首个消费窗口再发第二条：macOS 需跨一个定时器周期（33ms），
        // recv_timeout 路径的命令到达即刻唤醒，但源事件同样按 tick 抽干。
        std::thread::sleep(Duration::from_millis(60));
        cmd_tx.send(TestCommand(2)).unwrap();
        assert_eq!(ev_rx.recv().unwrap().0, 2);

        drop(cmd_tx);
        thread.join();
    }

    /// 事件源每轮 tick 被抽干，产出经 sink 送出（热键/手势走的同一条路）。
    #[test]
    fn sources_are_polled_and_forwarded() {
        let (cmd_tx, cmd_rx) = unbounded::<TestCommand>();
        let (ev_tx, ev_rx) = unbounded::<TestEvent>();
        let (plat_tx, plat_rx) = unbounded::<()>();
        let (src_tx, src_rx) = unbounded::<()>();

        let source: Box<dyn EventSource<()>> = Box::new(move || src_rx.try_iter().collect());
        let thread = spawn(
            cmd_rx,
            EventSink::new(ev_tx, plat_tx.clone()),
            echo,
            vec![source],
        );
        src_tx.send(()).unwrap();
        src_tx.send(()).unwrap();
        assert_eq!(plat_rx.recv().unwrap(), ());
        assert_eq!(plat_rx.recv().unwrap(), ());

        // 命令通道不受源影响，照常消费。
        cmd_tx.send(TestCommand(7)).unwrap();
        assert_eq!(ev_rx.recv().unwrap().0, 7);

        drop(cmd_tx);
        drop(plat_tx);
        thread.join();
    }

    /// 命令处理器 panic 不允许带倒事件线程：后续命令照常消费。
    #[test]
    fn panicking_handler_does_not_kill_thread() {
        let (cmd_tx, cmd_rx) = unbounded::<TestCommand>();
        let (ev_tx, ev_rx) = unbounded::<TestEvent>();
        let (plat_tx, plat_rx) = unbounded::<()>();
        drop(plat_rx);

        let thread = spawn(
            cmd_rx,
            EventSink::new(ev_tx, plat_tx),
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

    /// 事件源 poll panic 同样被拦截：线程存活，后续轮次照常抽干。
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
            EventSink::new(ev_tx, plat_tx.clone()),
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

    /// 组装点尚未接线时，sink 发送失败只返回 false，不允许 panic。
    #[test]
    fn sink_send_tolerates_closed_receivers() {
        let (ev_tx, ev_rx) = unbounded::<TestEvent>();
        let (plat_tx, plat_rx) = unbounded::<()>();
        drop(ev_rx);
        drop(plat_rx);
        let sink = EventSink::new(ev_tx, plat_tx);
        assert!(!sink.send_event(TestEvent(1)));
        assert!(!sink.send_platform(()));
    }
}
