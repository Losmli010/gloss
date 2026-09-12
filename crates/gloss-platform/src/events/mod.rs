//! 平台事件线程：全局热键、鼠标监听等系统事件源的唯一宿主（08 §7.2）。
//!
//! 事件线程先跑起 RunLoop 再注册事件源；取材命令（通道②）与系统事件在同一线程
//! 顺序消费，天然串行无锁。消息类型由组装点注入——platform 不依赖 gloss-app 的
//! 通道类型，测试用本地桩类型即可驱动整条循环。

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};

use gloss_core::log::{debug, error, info, thread, warn};
use gloss_core::task::HotkeyBinding;

pub mod hotkey;

use hotkey::HotkeyPump;

/// macOS RunLoop 的 drain 周期。run loop 无法阻塞等待 crossbeam 通道，
/// 只能定时抽干；热键到浮层的端到端延迟上界即此值，33ms 低于可感知阈值。
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
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// 启动平台事件线程。
///
/// `on_command` 在事件线程上顺序消费通道②（取材接线前可先挂测试桩）；
/// `hotkeys` 给出热键泵与「绑定 → ①平台事件」的映射后，热键按下经同一
/// sink 送出。退出协议：调用方 drop 通道② Sender，线程抽干剩余命令后
/// 自行结束，`join` 返回即线程已终止。
pub fn spawn<C, E, P, F, G>(
    commands: Receiver<C>,
    sink: EventSink<E, P>,
    on_command: F,
    hotkeys: Option<(HotkeyPump, G)>,
) -> EventThread
where
    C: Send + 'static,
    E: Send + 'static,
    P: Send + 'static,
    F: FnMut(C, &EventSink<E, P>) + Send + 'static,
    G: FnMut(&HotkeyBinding) -> P + Send + 'static,
{
    info!(thread = thread::EVENT, "spawning platform event thread");
    let spawned = std::thread::Builder::new()
        .name("gloss-event".into())
        .spawn(move || run(commands, sink, on_command, hotkeys));
    match spawned {
        Ok(join) => EventThread { join: Some(join) },
        // 线程缺失时应用仍能启动，但热键/取材全部失效——必须留痕。
        Err(err) => {
            warn!(thread = thread::EVENT, error = %err, "failed to spawn platform event thread");
            EventThread { join: None }
        }
    }
}

fn run<C, E, P, F, G>(
    commands: Receiver<C>,
    sink: EventSink<E, P>,
    on_command: F,
    hotkeys: Option<(HotkeyPump, G)>,
) where
    F: FnMut(C, &EventSink<E, P>),
    G: FnMut(&HotkeyBinding) -> P,
{
    match hotkeys {
        #[cfg(target_os = "macos")]
        hotkeys => run_loop(commands, sink, on_command, hotkeys),
        #[cfg(not(target_os = "macos"))]
        hotkeys => select_loop(commands, sink, on_command, hotkeys),
    }
    info!(thread = thread::EVENT, "platform event thread stopped");
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

fn dispatch_command<C, E, P, F>(sink: &EventSink<E, P>, on_command: &mut F, command: C)
where
    F: FnMut(C, &EventSink<E, P>),
{
    let _ = guarded("acquire command handler", || on_command(command, sink));
}

/// 非 macOS 的驱动：select 阻塞等待，无事件时零空转。
#[cfg(not(target_os = "macos"))]
fn select_loop<C, E, P, F, G>(
    commands: Receiver<C>,
    sink: EventSink<E, P>,
    mut on_command: F,
    mut hotkeys: Option<(HotkeyPump, G)>,
) where
    F: FnMut(C, &EventSink<E, P>),
    G: FnMut(&HotkeyBinding) -> P,
{
    use crossbeam_channel::{never, select};

    // select 的接收端个数固定：未启用热键时挂一个永不就绪的占位端。
    let hotkey_rx: Receiver<global_hotkey::GlobalHotKeyEvent> = match &hotkeys {
        Some(_) => global_hotkey::GlobalHotKeyEvent::receiver().clone(),
        None => never(),
    };
    loop {
        select! {
            recv(commands) -> msg => match msg {
                Ok(command) => dispatch_command(&sink, &mut on_command, command),
                // 通道② Sender 全部 drop：事件线程使命结束。
                Err(_) => break,
            },
            recv(hotkey_rx) -> _ => {
                if let Some((pump, to_platform)) = &mut hotkeys {
                    for binding in pump.poll() {
                        sink.send_platform(to_platform(&binding));
                    }
                }
            }
        }
    }
}

/// macOS 的驱动：RunLoop 不能阻塞在 crossbeam 上（08 §7.2），挂一个周期
/// 定时器 drain 通道②与热键队列；后续 CGEventTap 等事件源也挂同一 run loop。
#[cfg(target_os = "macos")]
fn run_loop<C, E, P, F, G>(
    commands: Receiver<C>,
    sink: EventSink<E, P>,
    on_command: F,
    hotkeys: Option<(HotkeyPump, G)>,
) where
    F: FnMut(C, &EventSink<E, P>),
    G: FnMut(&HotkeyBinding) -> P,
{
    use std::ffi::c_void;

    use core_foundation_sys::base::{CFRelease, kCFAllocatorDefault};
    use core_foundation_sys::date::CFAbsoluteTimeGetCurrent;
    use core_foundation_sys::runloop::{
        CFRunLoopAddTimer, CFRunLoopGetCurrent, CFRunLoopRun, CFRunLoopStop, CFRunLoopTimerContext,
        CFRunLoopTimerCreate, CFRunLoopTimerRef, kCFRunLoopCommonModes,
    };

    struct LoopState<C, E, P, F, G> {
        commands: Receiver<C>,
        sink: EventSink<E, P>,
        on_command: F,
        hotkeys: Option<(HotkeyPump, G)>,
        run_loop: core_foundation_sys::runloop::CFRunLoopRef,
    }

    extern "C" fn fire<C, E, P, F, G>(_timer: CFRunLoopTimerRef, info: *mut c_void)
    where
        F: FnMut(C, &EventSink<E, P>),
        G: FnMut(&HotkeyBinding) -> P,
    {
        let state = unsafe { &mut *(info as *mut LoopState<C, E, P, F, G>) };
        loop {
            match state.commands.try_recv() {
                Ok(command) => dispatch_command(&state.sink, &mut state.on_command, command),
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                // 通道② Sender 全部 drop：停掉 RunLoop，CFRunLoopRun 返回后线程结束。
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    unsafe { CFRunLoopStop(state.run_loop) };
                    break;
                }
            }
        }
        if let Some((pump, to_platform)) = &mut state.hotkeys {
            for binding in pump.poll() {
                let sink = &state.sink;
                let _ = guarded("hotkey forward", || {
                    sink.send_platform(to_platform(&binding))
                });
            }
        }
    }

    unsafe {
        let mut state = Box::new(LoopState {
            commands,
            sink,
            on_command,
            hotkeys,
            run_loop: CFRunLoopGetCurrent(),
        });
        let mut context = CFRunLoopTimerContext {
            version: 0,
            info: state.as_mut() as *mut LoopState<C, E, P, F, G> as *mut c_void,
            retain: None,
            release: None,
            copyDescription: None,
        };
        let interval = TICK.as_secs_f64();
        let timer = CFRunLoopTimerCreate(
            kCFAllocatorDefault,
            CFAbsoluteTimeGetCurrent() + interval,
            interval,
            0,
            0,
            fire::<C, E, P, F, G>,
            &mut context,
        );
        if timer.is_null() {
            warn!(
                thread = thread::EVENT,
                "failed to create run loop timer, event thread exits"
            );
            return;
        }
        CFRunLoopAddTimer(state.run_loop, timer, kCFRunLoopCommonModes);
        CFRunLoopRun();
        // RunLoop 已停止且不在回调中：先释放定时器，再收回状态盒。
        CFRelease(timer as *const c_void);
        drop(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;

    /// platform 不依赖 gloss-app：本地桩类型足以驱动整条循环。
    struct TestCommand(u64);
    struct TestEvent(u64);

    type Sink = EventSink<TestEvent, ()>;
    type NoHotkeys = Option<(HotkeyPump, fn(&HotkeyBinding) -> ())>;

    fn echo(command: TestCommand, sink: &Sink) {
        sink.send_event(TestEvent(command.0));
    }

    fn no_hotkeys() -> NoHotkeys {
        None
    }

    /// 通道②按顺序消费、产物按顺序到达；drop Sender 后线程自行退出。
    #[test]
    fn commands_are_consumed_in_order_then_thread_exits() {
        let (cmd_tx, cmd_rx) = unbounded::<TestCommand>();
        let (ev_tx, ev_rx) = unbounded::<TestEvent>();
        let (plat_tx, plat_rx) = unbounded::<()>();
        drop(plat_rx);

        let thread = spawn(cmd_rx, EventSink::new(ev_tx, plat_tx), echo, no_hotkeys());
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

    /// 初始抽干之后线程仍持续轮询：晚到的命令也能被消费（macOS 走 RunLoop
    /// 定时器路径，其余平台走 select，同一协议两种驱动）。
    #[test]
    fn late_commands_are_still_consumed() {
        let (cmd_tx, cmd_rx) = unbounded::<TestCommand>();
        let (ev_tx, ev_rx) = unbounded::<TestEvent>();
        let (plat_tx, plat_rx) = unbounded::<()>();
        drop(plat_rx);

        let thread = spawn(cmd_rx, EventSink::new(ev_tx, plat_tx), echo, no_hotkeys());
        cmd_tx.send(TestCommand(1)).unwrap();
        assert_eq!(ev_rx.recv().unwrap().0, 1);

        std::thread::sleep(TICK + Duration::from_millis(20));
        cmd_tx.send(TestCommand(2)).unwrap();
        assert_eq!(ev_rx.recv().unwrap().0, 2);

        drop(cmd_tx);
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
            no_hotkeys(),
        );
        cmd_tx.send(TestCommand(0)).unwrap();
        cmd_tx.send(TestCommand(1)).unwrap();
        assert_eq!(ev_rx.recv().unwrap().0, 1, "thread must survive the panic");
        drop(cmd_tx);
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
