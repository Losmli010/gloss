//! 全局鼠标监听：左键按下/释放 → 划词手势判定。
//!
//! 手势策略取「拖拽释放」（按下 → 时长与位移双达标 → 释放）：最短按压时长
//! 滤除瞬时甩动，位移阈值区分普通点击。
//!
//! **订阅面只有左键按下与释放**（见 [`SUBSCRIBED_EVENT_TYPES`]），坐标直接取自
//! 事件自身；键盘事件不进入本模块。
//!
//! 线程约束：监听在**当前线程**建立事件 tap 并阻塞运行（回调跑在它自己的
//! RunLoop 上），不能在平台事件线程内运行，也没有停止 API——监听线程随进程
//! 退出消亡。需要辅助功能授权，未授权时 tap 建立失败 → 手势功能
//! 整体降级：建立结果经 [`MouseSource::spawn`] **同步**可知（等创建相结果，
//! 无须轮询），运行中失效才经标志异步置位。

use crossbeam_channel::Receiver;

/// 判定为「拖拽选择」所需的最小位移，单位是系统坐标（逻辑点，不必按 DPI
/// 折算）——远高于普通点击的抖动。
const DRAG_THRESHOLD: f64 = 12.0;

/// 判定为「拖拽选择」所需的最短按压时长（纳秒）：瞬时甩动（按下与释放几乎
/// 同时到达）在此被滤除。取 150ms 的缘由：人手有意的拖拽选择从按下到释放
/// 稳定在数百毫秒量级（点击本身的按压就有约 100ms，再加上拖动时间），而
/// 甩动、HID 层连滑这类误触的按压与释放间隔趋近于零——150ms 在两者之间
/// 两侧都留足裕量：快甩滤得掉，慢拖不受影响。
const MIN_PRESS_NANOS: u64 = 150_000_000;

/// 左键的两种动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeftButton {
    Pressed,
    Released,
}

/// tap 层交给手势层的事件：动作 + **该事件自身**的坐标与时间戳。
///
/// 坐标不做「沿用最近一次移动位置」的缓存：真实拖拽期间系统只投递「拖拽」
/// 事件（不是「移动」），缓存下来的位置会停在按下之前，释放时算出的位移恒为
/// 零，手势一次也判不出来。让每个事件自带坐标，判定的输入就不再依赖系统是否
/// 恰好投递了移动事件。时间戳同理取事件自身的（纳秒，单调），按压时长的
/// 判定因此不依赖回调投递的先后延迟。
#[derive(Debug, Clone, Copy, PartialEq)]
struct ButtonEvent {
    action: LeftButton,
    pos: (f64, f64),
    /// 事件自身的时间戳（纳秒，自系统启动起）。
    time: u64,
}

/// CGEventTypes.h 的事件类型原始值。用原始数字而不是 core-graphics 的
/// `CGEventType`：订阅规则（尤其「键盘绝不订阅」）不依赖系统头文件即可单测。
const CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
const CG_EVENT_LEFT_MOUSE_UP: u32 = 2;

/// 必须排除的类型：键盘（10/11/12）与滚轮（22）。
#[cfg(test)]
const CG_EVENT_KEY_DOWN: u32 = 10;
#[cfg(test)]
const CG_EVENT_KEY_UP: u32 = 11;
#[cfg(test)]
const CG_EVENT_FLAGS_CHANGED: u32 = 12;
#[cfg(test)]
const CG_EVENT_SCROLL_WHEEL: u32 = 22;

/// 订阅清单——**唯一事实来源**：订阅掩码与回调过滤都由它推出，不要在
/// 实现里另写第二份口径。
const SUBSCRIBED_EVENT_TYPES: [(u32, LeftButton); 2] = [
    (CG_EVENT_LEFT_MOUSE_DOWN, LeftButton::Pressed),
    (CG_EVENT_LEFT_MOUSE_UP, LeftButton::Released),
];

/// 原始类型码 → 左键动作；未订阅的类型一律 `None`（回调据此原样放行事件）。
fn classify(raw_type: u32) -> Option<LeftButton> {
    SUBSCRIBED_EVENT_TYPES
        .iter()
        .find(|(raw, _)| *raw == raw_type)
        .map(|(_, action)| *action)
}

/// 划词手势状态机：Idle →（左键按下）→ Pressed →（时长与位移双达标后释放）
/// → 判定一次拖拽选择。模块私有——公共面只经 [`MouseSource`] 使用它。
#[derive(Default)]
struct GestureDetector {
    press: Option<Press>,
}

/// 一次按下的快照：位置与事件时间戳，释放时算位移与按压时长用。
#[derive(Debug, Clone, Copy, PartialEq)]
struct Press {
    pos: (f64, f64),
    time: u64,
}

impl GestureDetector {
    /// 抽干事件流并驱动状态机，返回本轮判定的拖拽选择与各自的释放坐标。
    fn poll(&mut self, events: &Receiver<ButtonEvent>) -> Vec<(f64, f64)> {
        let mut fired = Vec::new();
        while let Ok(event) = events.try_recv() {
            if let Some(pos) = self.feed(event) {
                fired.push(pos);
            }
        }
        fired
    }

    /// 喂入单个事件，判定为一次完整的拖拽选择（释放且按压时长与位移双达标）
    /// 时返回释放坐标——浮层跟随划词位置的输入。
    fn feed(&mut self, event: ButtonEvent) -> Option<(f64, f64)> {
        match (event.action, self.press) {
            (LeftButton::Pressed, _) => {
                self.press = Some(Press {
                    pos: event.pos,
                    time: event.time,
                });
                None
            }
            (LeftButton::Released, Some(start)) => {
                self.press = None;
                let dx = event.pos.0 - start.pos.0;
                let dy = event.pos.1 - start.pos.1;
                let held = event.time.saturating_sub(start.time);
                (held >= MIN_PRESS_NANOS && dx * dx + dy * dy > DRAG_THRESHOLD * DRAG_THRESHOLD)
                    .then_some(event.pos)
            }
            // 无按下记录的释放（如监听启动前就按下的拖拽），静默忽略。
            (LeftButton::Released, None) => None,
        }
    }
}

mod tap {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::JoinHandle;

    use crossbeam_channel::{Receiver, bounded};

    use gloss_core::log::{debug, info, thread, warn};
    use gloss_core::model::ScreenPoint;

    use super::{ButtonEvent, GestureDetector};
    use crate::events::EventSource;

    /// 划词手势产物（platform 本地类型；组装点映射为 ① 的平台事件）。
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum MouseGesture {
        /// 用户完成了一次典型的文本选择动作（拖拽释放），载荷是释放坐标
        /// （系统全局坐标，逻辑点、左上原点）——浮层跟随划词位置用。
        Selection {
            /// 释放坐标（整型化，与 [`gloss_core::model::ScreenRect`] 同一口径）。
            pos: ScreenPoint,
        },
    }

    /// 监听启动失败的原因——只用于日志与降级判定（手势可降级，失败不向上
    /// 传播错误，与「外观类功能不拦启动」同一取向）。
    #[derive(Debug)]
    enum ListenError {
        /// 系统拒绝建立事件 tap：最常见的原因是未授予辅助功能权限。
        EventTap,
        /// 建立 tap 所需的 run loop source 没建起来（资源耗尽等）。
        RunLoopSource,
    }

    /// 事件线程侧的鼠标手势源：tap 事件通道 + 状态机。
    pub struct MouseSource {
        events: Receiver<ButtonEvent>,
        detector: GestureDetector,
    }

    impl MouseSource {
        /// 启动全局鼠标监听，返回事件线程侧源与「运行中失效」标志。
        ///
        /// tap 的建立结果在本调用内**同步**可知（等创建相结果的有界通道，
        /// 非计时等待）：返回 `None` 即 tap 没建起来（未授权辅助功能是最
        /// 常见原因），组装点据此做启动期引导，无须等轮询；返回
        /// `Some` 时 tap 已在投递事件。标志仅在 tap 中途停止投递时置位
        /// （运行中失效的观测点），启动失败不经它表达。
        pub fn spawn() -> (Option<Self>, Arc<AtomicBool>) {
            let degraded = Arc::new(AtomicBool::new(false));
            let events = spawn_tap(Arc::clone(&degraded));
            (
                events.map(|events| Self {
                    events,
                    detector: GestureDetector::default(),
                }),
                degraded,
            )
        }
    }

    impl EventSource<MouseGesture> for MouseSource {
        fn poll(&mut self) -> Vec<MouseGesture> {
            self.detector
                .poll(&self.events)
                .into_iter()
                .map(|(x, y)| MouseGesture::Selection {
                    pos: ScreenPoint::new(x.round() as i32, y.round() as i32),
                })
                .collect()
        }
    }

    /// 启动 tap 监听线程并**同步等待**创建相结果：成功返回事件通道；失败
    /// 返回 `None`（原因已记日志）。创建相与阻塞相分离（见 `imp`），失败
    /// 在进 run loop 前就确定——本等待是通道事件且有界，创建本身无用户
    /// 交互。
    fn spawn_tap(degraded: Arc<AtomicBool>) -> Option<Receiver<ButtonEvent>> {
        let (tx, rx) = bounded(256);
        let (created_tx, created_rx) = bounded::<Result<(), ListenError>>(1);
        let spawned: Result<JoinHandle<()>, _> = std::thread::Builder::new()
            .name("gloss-mouse-tap".into())
            .spawn(move || {
                // 事件 tap 回调是 C 回调，panic 穿过它直接 abort 进程：与事件
                // 线程的 guarded 同一纪律，兜底后监听继续。兜底放在这里而不在
                // tap 实现里，是模块与实现之间的分工——实现侧只需保证自己不
                // panic（它的活只是常量查表加读一次坐标）。
                let sink = move |event: ButtonEvent| {
                    // panic 屏障：闭包的结果（含 panic 信号）有意整体丢弃，
                    // 监听循环只关心是否继续。
                    #[allow(clippy::let_underscore_must_use)]
                    let _ = catch_unwind(AssertUnwindSafe(|| {
                        // 通道满时丢弃：手势判定不需要完整事件流，反压 tap 回调
                        // 的代价远大于丢一次触发机会。
                        if tx.try_send(event).is_err() {
                            debug!(
                                thread = thread::MOUSE_TAP,
                                "mouse event queue full, event dropped"
                            );
                        }
                    }));
                };
                match imp::create(sink) {
                    Ok(handle) => {
                        // 创建成功先回报再进阻塞的 run loop（回报通道只有一
                        // 位且调用方正同步等待；即便对方已提前放弃也不影响
                        // 监听）。此后只有「运行中停止投递」一条退出路径
                        // （tap 没有停止 API），置位降级标志并留 info 便于
                        // 诊断。
                        drop(created_tx.send(Ok(())));
                        imp::run(handle);
                        degraded.store(true, Ordering::Relaxed);
                        info!(thread = thread::MOUSE_TAP, "mouse listener stopped");
                    }
                    Err(err) => {
                        drop(created_tx.send(Err(err)));
                    }
                }
            });
        match spawned {
            Ok(_join) => match created_rx.recv() {
                Ok(Ok(())) => Some(rx),
                Ok(Err(err)) => {
                    warn!(
                        thread = thread::MOUSE_TAP,
                        error = ?err,
                        "mouse tap creation failed, selection gesture disabled"
                    );
                    None
                }
                // 创建相在回报前就消亡（panic 于纯 FFI 与装箱路径，理论不可
                // 达）：按缺失降级，与创建失败同一出口。
                Err(_) => {
                    warn!(
                        thread = thread::MOUSE_TAP,
                        "mouse tap thread vanished before reporting creation"
                    );
                    None
                }
            },
            Err(err) => {
                warn!(
                    thread = thread::MOUSE_TAP,
                    error = %err,
                    "failed to spawn mouse tap thread, selection gesture disabled"
                );
                None
            }
        }
    }

    #[cfg(test)]
    pub(super) fn subscribed_mask() -> u64 {
        imp::subscribed_mask()
    }

    /// 自建只订阅左键的 CGEventTap。
    ///
    mod imp {
        use std::ffi::c_void;

        use crate::ffi::cf::{
            CFMachPortCreateRunLoopSource, CFRelease, CFRunLoopAddSource, CFRunLoopGetCurrent,
            CFRunLoopRun, kCFAllocatorDefault, kCFRunLoopCommonModes,
        };
        use crate::ffi::eventtap::{
            CGEventGetLocation, CGEventGetTimestamp, CGEventRef, CGEventTapCreate,
            CGEventTapEnable, CGEventTapProxy, CGPoint, K_CG_EVENT_TAP_OPTION_LISTEN_ONLY,
            K_CG_HEAD_INSERT_EVENT_TAP, K_CG_HID_EVENT_TAP,
        };

        use super::super::{ButtonEvent, SUBSCRIBED_EVENT_TYPES, classify};
        use super::ListenError;

        /// 订阅掩码：由共享的 [`SUBSCRIBED_EVENT_TYPES`] 推出——掩码不另写一份，
        /// 否则键盘事件会从两处口径的缝隙里回来（代价见模块注释）。
        pub(super) fn subscribed_mask() -> u64 {
            SUBSCRIBED_EVENT_TYPES
                .iter()
                .fold(0, |mask, (raw, _)| mask | (1_u64 << raw))
        }

        /// 已建成待运行的 tap：`create` 与 `run` 两相的交接凭证。tap 与其
        /// run loop source 已挂上当前线程的 run loop，句柄本身无状态——
        /// 它让「没建成就不能运行」成为类型层面的约束。
        pub struct TapHandle;

        /// 在**当前线程**建立只订阅左键按下/释放的 tap（创建相）：建 tap、
        /// 建 run loop source、挂上当前线程 run loop 并启用。不阻塞运行——
        /// 那是 [`run`]（阻塞相）的事，两相之间创建结果才得以同步发回调用
        /// 方（启动预检据此判定授权，见 `spawn_tap`）。
        ///
        /// 调用方保证：`sink` 不向外抛 panic——它由 C 回调调用，panic 穿过
        /// C 边界会 abort 进程（兜底在 `spawn_tap`）。
        pub fn create(sink: impl Fn(ButtonEvent) + 'static) -> Result<TapHandle, ListenError> {
            // 回调上下文必须活到进程结束：tap 没有停止 API，本线程的 run loop
            // 一直跑到进程退出。故 **tap 建成后**有意泄漏这份 Box（每进程至多
            // 一份）——注意它是「泄漏」而非 `mem::forget` 式不可达：tap 的
            // user_info 一直指着它。两条失败路径都会把它收回（见下）。二次装箱
            // 是为了给 `user_info` 一个瘦指针。
            let ctx: *mut Box<dyn Fn(ButtonEvent)> = Box::into_raw(Box::new(Box::new(sink)));

            // SAFETY: 参数都是常量或本进程内的有效指针；`raw_callback` 的签名与
            // CGEventTapCallback 一致；`ctx` 刚由 Box::into_raw 得到，与 tap 同寿。
            let tap = unsafe {
                CGEventTapCreate(
                    K_CG_HID_EVENT_TAP,
                    K_CG_HEAD_INSERT_EVENT_TAP,
                    K_CG_EVENT_TAP_OPTION_LISTEN_ONLY,
                    subscribed_mask(),
                    raw_callback,
                    ctx.cast(),
                )
            };
            if tap.is_null() {
                // 没建成 tap：回调永远不会来，把上下文收回，别白泄漏一份。
                // SAFETY: `ctx` 来自上面的 Box::into_raw，此刻仍是唯一引用。
                drop(unsafe { Box::from_raw(ctx) });
                return Err(ListenError::EventTap);
            }

            // SAFETY: `tap` 非空（上面判过）；NULL allocator 表示默认分配器；
            // 返回 +1 引用，下面交给 run loop 后归还。
            let source = unsafe { CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0) };
            if source.is_null() {
                // 没建起 run loop source：tap 与上下文都不会有人用，一并收回——
                // 这条路径之后手势就永久降级了，没有「活到进程结束」的理由。
                // SAFETY: `tap` 是上面 CGEventTapCreate 的 +1 引用，归我们归还；
                // `ctx` 仍是 Box::into_raw 得到的唯一引用。
                unsafe {
                    CFRelease(tap.cast::<c_void>());
                    drop(Box::from_raw(ctx));
                }
                return Err(ListenError::RunLoopSource);
            }
            // SAFETY: 取当前线程的 run loop（不转移所有权）；`source` 有效，加入
            // 后 run loop 自己持有一份，故随后释放我们这份 +1 引用。
            unsafe {
                let run_loop = CFRunLoopGetCurrent();
                CFRunLoopAddSource(run_loop, source, kCFRunLoopCommonModes);
                CFRelease(source.cast::<c_void>());
                CGEventTapEnable(tap, true);
            }
            Ok(TapHandle)
        }

        /// 阻塞运行当前线程的 run loop（阻塞相）：订阅的事件从这里投递进
        /// [`raw_callback`]。只在系统层面停止投递时返回（如 tap 失效），
        /// 没有停止 API——返回即手势失效，调用方按降级处理。
        pub fn run(_handle: TapHandle) {
            // SAFETY: `create` 已把 source 挂到当前线程的 run loop 上，这里
            // 只是进入阻塞运行。参数只承载两相顺序约束，进入运行即完成使命。
            unsafe { CFRunLoopRun() };
        }

        /// tap 回调：把订阅到的事件翻译成 [`ButtonEvent`] 交给 sink，未订阅的
        /// 一律原样放行（listen-only 的 tap 也必须返回事件本身）。
        ///
        /// 翻译只用常量查表加位置与时间戳各一次读取（不分配、不调用会 panic
        /// 的 API），panic 兜底在 sink 侧（见 [`listen`] 的调用方保证）。
        unsafe extern "C" fn raw_callback(
            _proxy: CGEventTapProxy,
            raw_type: u32,
            event: CGEventRef,
            user_info: *mut c_void,
        ) -> CGEventRef {
            if let Some(action) = classify(raw_type) {
                // SAFETY: `user_info` 是 `listen` 传进来、有意泄漏到进程结束的
                // Box<dyn Fn(ButtonEvent)> 指针；C 侧不释放它。
                let sink = unsafe { &*(user_info as *const Box<dyn Fn(ButtonEvent)>) };
                // SAFETY: `event` 是系统在回调期间借给我们的事件引用，只读位置
                // 与时间戳，不转移所有权。
                let CGPoint { x, y } = unsafe { CGEventGetLocation(event) };
                // SAFETY: 同上，`event` 在回调期间有效；时间戳是纯读取。
                let time = unsafe { CGEventGetTimestamp(event) };
                sink(ButtonEvent {
                    action,
                    pos: (x, y),
                    time,
                });
            }
            event
        }
    }
}

pub use tap::{MouseGesture, MouseSource};

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;

    fn pressed(pos: (f64, f64), at_ns: u64) -> ButtonEvent {
        ButtonEvent {
            action: LeftButton::Pressed,
            pos,
            time: at_ns,
        }
    }

    fn released(pos: (f64, f64), at_ns: u64) -> ButtonEvent {
        ButtonEvent {
            action: LeftButton::Released,
            pos,
            time: at_ns,
        }
    }

    /// 慢拖用的常量时刻：按下 1s，释放 1.4s——按压时长 400ms，稳过下限。
    const SLOW_PRESS: u64 = 1_000_000_000;
    const SLOW_RELEASE: u64 = 1_400_000_000;

    #[test]
    fn drag_release_emits_selection() {
        let mut detector = GestureDetector::default();
        assert_eq!(detector.feed(pressed((10.0, 10.0), SLOW_PRESS)), None);
        assert_eq!(
            detector.feed(released((140.0, 30.0), SLOW_RELEASE)),
            Some((140.0, 30.0))
        );
    }

    #[test]
    fn plain_click_and_jitter_do_not_trigger() {
        let mut detector = GestureDetector::default();
        assert_eq!(detector.feed(pressed((10.0, 10.0), SLOW_PRESS)), None);
        assert_eq!(
            detector.feed(released((10.0, 10.0), SLOW_RELEASE)),
            None,
            "no displacement, no gesture even with a long press"
        );

        assert_eq!(detector.feed(pressed((50.0, 50.0), SLOW_PRESS)), None);
        assert_eq!(
            detector.feed(released((56.0, 55.0), SLOW_RELEASE)),
            None,
            "jitter below threshold"
        );
    }

    #[test]
    fn flick_shorter_than_the_minimum_press_is_filtered() {
        let mut detector = GestureDetector::default();
        let press = 2_000_000_000;
        assert_eq!(detector.feed(pressed((10.0, 10.0), press)), None);
        assert_eq!(
            detector.feed(released((140.0, 30.0), press + 10_000_000)),
            None,
            "a 10ms flick with ample displacement must not fire"
        );
    }

    #[test]
    fn press_duration_is_observed_at_the_boundary() {
        let mut detector = GestureDetector::default();
        let press = 3_000_000_000;
        assert_eq!(detector.feed(pressed((0.0, 0.0), press)), None);
        assert_eq!(
            detector.feed(released((200.0, 0.0), press + MIN_PRESS_NANOS - 1)),
            None,
            "one nanosecond short of the floor stays filtered"
        );

        assert_eq!(detector.feed(pressed((0.0, 0.0), press)), None);
        assert_eq!(
            detector.feed(released((200.0, 0.0), press + MIN_PRESS_NANOS)),
            Some((200.0, 0.0)),
            "exactly the floor counts as a deliberate drag"
        );
    }

    #[test]
    fn displacement_comes_from_the_events_themselves() {
        let mut detector = GestureDetector::default();
        assert_eq!(detector.feed(pressed((100.0, 100.0), SLOW_PRESS)), None);
        assert_eq!(
            detector.feed(released((400.0, 100.0), SLOW_RELEASE)),
            Some((400.0, 100.0)),
            "release position is the release event's own position"
        );
    }

    #[test]
    fn state_resets_after_each_gesture() {
        let mut detector = GestureDetector::default();
        assert_eq!(detector.feed(pressed((0.0, 0.0), SLOW_PRESS)), None);
        assert_eq!(
            detector.feed(released((200.0, 0.0), SLOW_RELEASE)),
            Some((200.0, 0.0))
        );
        assert_eq!(detector.feed(pressed((500.0, 500.0), SLOW_RELEASE)), None);
        assert_eq!(
            detector.feed(released((700.0, 500.0), SLOW_RELEASE + 400_000_000)),
            Some((700.0, 500.0))
        );
    }

    #[test]
    fn stray_events_are_ignored() {
        let mut detector = GestureDetector::default();
        assert_eq!(detector.feed(released((1.0, 1.0), SLOW_PRESS)), None);
        assert_eq!(detector.feed(pressed((0.0, 0.0), SLOW_PRESS)), None);
        assert_eq!(detector.feed(pressed((100.0, 100.0), SLOW_PRESS)), None);
        assert_eq!(
            detector.feed(released((200.0, 100.0), SLOW_RELEASE)),
            Some((200.0, 100.0))
        );
    }

    #[test]
    fn poll_drains_channel_through_state_machine() {
        let (tx, rx) = unbounded::<ButtonEvent>();
        let mut detector = GestureDetector::default();
        tx.send(pressed((0.0, 0.0), SLOW_PRESS)).unwrap();
        tx.send(released((100.0, 0.0), SLOW_RELEASE)).unwrap();
        assert_eq!(detector.poll(&rx), vec![(100.0, 0.0)]);
        assert!(detector.poll(&rx).is_empty(), "drained queue stays empty");
    }

    #[test]
    fn spawn_resolves_synchronously_and_startup_failure_stays_off_the_flag() {
        let (source, degraded) = MouseSource::spawn();
        assert!(
            !degraded.load(std::sync::atomic::Ordering::Relaxed),
            "startup failure must not set the mid-run flag"
        );
        drop(source);
    }

    #[test]
    fn keyboard_events_are_never_subscribed() {
        assert_eq!(
            SUBSCRIBED_EVENT_TYPES.map(|(raw, _)| raw),
            [CG_EVENT_LEFT_MOUSE_DOWN, CG_EVENT_LEFT_MOUSE_UP],
            "the subscription list holds the left button only"
        );
        assert_eq!(
            classify(CG_EVENT_LEFT_MOUSE_DOWN),
            Some(LeftButton::Pressed)
        );
        assert_eq!(classify(CG_EVENT_LEFT_MOUSE_UP), Some(LeftButton::Released));

        for raw in [
            CG_EVENT_KEY_DOWN,
            CG_EVENT_KEY_UP,
            CG_EVENT_FLAGS_CHANGED,
            CG_EVENT_SCROLL_WHEEL,
        ] {
            assert_eq!(classify(raw), None, "type {raw} must never be handled");
        }
        let mask = tap::subscribed_mask();
        assert_eq!(
            mask,
            (1 << CG_EVENT_LEFT_MOUSE_DOWN) | (1 << CG_EVENT_LEFT_MOUSE_UP)
        );
        for raw in [
            CG_EVENT_KEY_DOWN,
            CG_EVENT_KEY_UP,
            CG_EVENT_FLAGS_CHANGED,
            CG_EVENT_SCROLL_WHEEL,
        ] {
            assert_eq!(mask & (1 << raw), 0, "mask bit {raw} must stay clear");
        }
        assert_eq!(classify(0), None, "unsubscribed types fall through");
        assert_eq!(classify(999), None);
    }
}

#[cfg(test)]
mod injected_gesture_live_tests {
    use std::time::{Duration, Instant};

    use rdev::{Button, EventType};

    use super::tap::MouseSource;
    use crate::events::EventSource;

    #[test]
    #[ignore = "注入真实全局鼠标事件：先把运行测试的终端 App 加入 系统设置→隐私与安全性→辅助功能（未授权时由 live_test_support 快速失败）"]
    fn injected_drag_yields_selection_gesture() {
        crate::live_test_support::require_accessibility("mouse_drag_gesture");
        let (source, degraded) = MouseSource::spawn();
        let mut source = source.expect("tap must start under accessibility grant");

        rdev::simulate(&EventType::MouseMove { x: 100.0, y: 100.0 }).expect("move injection");
        rdev::simulate(&EventType::ButtonPress(Button::Left)).expect("button press injection");
        for step in 1..=5 {
            rdev::simulate(&EventType::MouseMove {
                x: 100.0 + 60.0 * f64::from(step),
                y: 100.0 + 40.0 * f64::from(step),
            })
            .expect("move injection");
        }
        // 注入的事件自带真实时间戳：释放前真实睡过最短按压时长，否则手势会
        // 被时长下限滤除（快甩滤除是本就想要的行为）。
        std::thread::sleep(Duration::from_millis(200));
        rdev::simulate(&EventType::ButtonRelease(Button::Left)).expect("button release injection");

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if !source.poll().is_empty() {
                assert!(
                    !degraded.load(std::sync::atomic::Ordering::Relaxed),
                    "listener must not be degraded under a valid grant"
                );
                return;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        panic!("selection gesture not observed within 2s");
    }
}
