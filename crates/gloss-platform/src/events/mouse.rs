//! 全局鼠标监听：左键按下/释放 → 划词手势判定。
//!
//! 手势策略取「拖拽释放」（按下 → 位移超阈值 → 释放）：双击会把「打开
//! 链接」这类普通操作误判成划词，修饰键确认需要额外的键盘全局钩子，停留
//! 判定需要定时器——拖拽释放是误触最少的最简状态机。其余策略（含框选
//! `RegionGesture`）待配置化与图像任务落地时引入。
//!
//! **订阅面只有左键按下与释放**（见 [`SUBSCRIBED_EVENT_TYPES`]），坐标直接取自
//! 事件自身。窄订阅面不只是省事：订阅「全部事件」的实现会把键盘事件也送进
//! 回调，而把按键翻成字符要走输入法 API（macOS 的 TSM/HIToolbox 要求主线程），
//! 回调一旦跑在监听线程上就会以 SIGILL 打死整个进程——跑起来的 Gloss 只要用户
//! 按任意键（含 Ctrl-C）就消失。键盘事件从此不进入本模块，热键归 `global-hotkey`。
//!
//! 线程约束：监听在**当前线程**建立事件 tap 并阻塞运行（回调跑在它自己的
//! RunLoop 上，满足 08 §7.2 的亲和要求），不能在平台事件线程内运行，也没
//! 有停止 API——监听线程随进程退出消亡。需要辅助功能权限，未授权时监听
//! 失败 → 手势功能整体降级，热键路径不受影响；降级经 [`MouseSource::spawn`]
//! 返回的标志对外可观测，由组装点做一次性提示。

use crossbeam_channel::Receiver;

/// 判定为「拖拽选择」所需的最小位移，单位是系统坐标（逻辑点，不必按 DPI
/// 折算）——远高于普通点击的抖动。
const DRAG_THRESHOLD: f64 = 12.0;

/// 左键的两种动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeftButton {
    Pressed,
    Released,
}

/// tap 层交给手势层的事件：动作 + **该事件自身**的坐标。
///
/// 坐标不做「沿用最近一次移动位置」的缓存：真实拖拽期间系统只投递「拖拽」
/// 事件（不是「移动」），缓存下来的位置会停在按下之前，释放时算出的位移恒为
/// 零，手势一次也判不出来。让每个事件自带坐标，判定的输入就不再依赖系统是否
/// 恰好投递了移动事件。
#[derive(Debug, Clone, Copy, PartialEq)]
struct ButtonEvent {
    action: LeftButton,
    pos: (f64, f64),
}

/// CGEventTypes.h 的事件类型原始值。用原始数字而不是 core-graphics 的
/// `CGEventType`：订阅规则（尤其「键盘绝不订阅」）不依赖系统头文件即可单测。
const CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
const CG_EVENT_LEFT_MOUSE_UP: u32 = 2;

/// 必须排除的类型：键盘（10/11/12）与滚轮（22）。只在测试里用得到——列出来是
/// 为了让「键盘事件绝不进订阅面」这条可断言，见 tests 的
/// `keyboard_events_are_never_subscribed`。
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

/// 划词手势状态机：Idle →（左键按下）→ Pressed →（位移超阈值后释放）
/// → 判定一次拖拽选择。模块私有——公共面只经 [`MouseSource`] 使用它。
#[derive(Default)]
struct GestureDetector {
    pressed_at: Option<(f64, f64)>,
}

impl GestureDetector {
    /// 抽干事件流并驱动状态机，返回本轮判定的拖拽选择次数。
    fn poll(&mut self, events: &Receiver<ButtonEvent>) -> usize {
        let mut fired = 0;
        while let Ok(event) = events.try_recv() {
            if self.feed(event) {
                fired += 1;
            }
        }
        fired
    }

    /// 喂入单个事件，返回是否判定为一次完整的拖拽选择（释放且位移超阈值）。
    fn feed(&mut self, event: ButtonEvent) -> bool {
        match (event.action, self.pressed_at) {
            (LeftButton::Pressed, _) => {
                self.pressed_at = Some(event.pos);
                false
            }
            (LeftButton::Released, Some(start)) => {
                self.pressed_at = None;
                let dx = event.pos.0 - start.0;
                let dy = event.pos.1 - start.1;
                dx * dx + dy * dy > DRAG_THRESHOLD * DRAG_THRESHOLD
            }
            // 无按下记录的释放（如监听启动前就按下的拖拽），静默忽略。
            (LeftButton::Released, None) => false,
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

    use super::{ButtonEvent, GestureDetector};
    use crate::events::EventSource;

    /// 划词手势产物（platform 本地类型；组装点映射为 ① 的平台事件）。
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum MouseGesture {
        /// 用户完成了一次典型的文本选择动作（拖拽释放）。
        Selection,
    }

    /// 监听启动失败的原因——只用于日志与降级判定（手势可降级，失败不向上
    /// 传播错误，取向与热键的降级契约一致）。
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
        /// 启动全局鼠标监听，返回事件线程侧源与降级标志。监听线程启动
        /// 失败返回 `None`；监听器提前退出（如未授权辅助功能）是异步发生
        /// 的，两者都经标志置位对外可观测，由组装点做一次性提示。返回
        /// `None` 时手势功能整体降级，热键路径不受影响。
        pub fn spawn() -> (Option<Self>, Arc<AtomicBool>) {
            let degraded = Arc::new(AtomicBool::new(false));
            let events = spawn_tap(Arc::clone(&degraded));
            if events.is_none() {
                degraded.store(true, Ordering::Relaxed);
            }
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
            (0..self.detector.poll(&self.events))
                .map(|_| MouseGesture::Selection)
                .collect()
        }
    }

    /// 启动 tap 监听线程，返回事件通道；线程起不来返回 `None` 并置位降级标志。
    fn spawn_tap(degraded: Arc<AtomicBool>) -> Option<Receiver<ButtonEvent>> {
        let (tx, rx) = bounded(256);
        let spawned: Result<JoinHandle<()>, _> = std::thread::Builder::new()
            .name("gloss-mouse-tap".into())
            .spawn(move || {
                // 事件 tap 回调是 C 回调，panic 穿过它直接 abort 进程：与事件
                // 线程的 guarded 同一纪律，兜底后监听继续。兜底放在这里而不在
                // tap 实现里，是模块与实现之间的分工——实现侧只需保证自己不
                // panic（它的活只是常量查表加读一次坐标）。
                let sink = move |event: ButtonEvent| {
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
                match imp::listen(sink) {
                    // listen 正常返回只发生在系统层面停止投递时（如 tap 失效）：
                    // 手势从此收不到，置位降级标志并留 info 便于诊断。
                    Ok(()) => {
                        degraded.store(true, Ordering::Relaxed);
                        info!(thread = thread::MOUSE_TAP, "mouse listener stopped");
                    }
                    Err(err) => {
                        degraded.store(true, Ordering::Relaxed);
                        warn!(
                            thread = thread::MOUSE_TAP,
                            error = ?err,
                            "mouse listener failed, selection gesture disabled"
                        );
                    }
                }
            });
        match spawned {
            Ok(_join) => Some(rx),
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

    /// 测试缝隙：把生产侧订阅掩码暴露给回归测试（tests 直接断言它的返回
    /// 值），避免测试自算掩码与生产实现分叉。
    #[cfg(test)]
    pub(super) fn subscribed_mask() -> u64 {
        imp::subscribed_mask()
    }

    /// 自建只订阅左键的 CGEventTap。
    ///
    /// 不用 rdev 的 `listen`：它的 tap 订阅 `kCGEventMaskForAllEvents`，键盘事件
    /// 也进回调，而它把按键翻成字符要调 `TISCopyCurrentKeyboardInputSource`
    /// （TSM/HIToolbox，要求主线程）——回调跑在 `gloss-mouse-tap` 线程上，
    /// libdispatch 的 `dispatch_assert_queue` 断言失败后以 SIGILL 打死进程
    /// （`EXC_BAD_INSTRUCTION`，崩溃线程 `gloss-mouse-tap`）。rdev 在本仓库
    /// 仍用于按键注入（剪贴板兜底）。
    mod imp {
        use std::ffi::c_void;

        use core_foundation_sys::base::{CFRelease, kCFAllocatorDefault};
        use core_foundation_sys::mach_port::{CFMachPortCreateRunLoopSource, CFMachPortRef};
        use core_foundation_sys::runloop::{
            CFRunLoopAddSource, CFRunLoopGetCurrent, CFRunLoopRun, kCFRunLoopCommonModes,
        };

        use super::super::{ButtonEvent, SUBSCRIBED_EVENT_TYPES, classify};
        use super::ListenError;

        /// CoreGraphics 的事件引用：回调期间由系统借用，我们只读它的位置。
        type CGEventRef = *const c_void;
        /// tap 回调的 proxy 参数：本实现只观察、不改写事件流，用不到。
        type CGEventTapProxy = *const c_void;

        /// 指针位置的返回结构（CGGeometry.h 的 CGPoint，两个 f64 按 C ABI 返回）。
        #[repr(C)]
        #[derive(Clone, Copy, Debug)]
        struct CGPoint {
            x: f64,
            y: f64,
        }

        /// CGEventTapLocation 的 kCGHIDEventTap：事件链最早的一层。
        const K_CG_HID_EVENT_TAP: u32 = 0;
        /// CGEventTapPlacement 的 kCGHeadInsertEventTap。
        const K_CG_HEAD_INSERT_EVENT_TAP: u32 = 0;
        /// CGEventTapOptions 的 kCGEventTapOptionListenOnly：只观察，不改写事件流。
        const K_CG_EVENT_TAP_OPTION_LISTEN_ONLY: u32 = 1;

        /// tap 回调的 C 签名。
        type TapCallback =
            unsafe extern "C" fn(CGEventTapProxy, u32, CGEventRef, *mut c_void) -> CGEventRef;

        #[link(name = "CoreGraphics", kind = "framework")]
        unsafe extern "C" {
            /// 建立事件 tap；未获辅助功能授权（或系统 tap 名额耗尽）时返回 NULL。
            fn CGEventTapCreate(
                location: u32,
                placement: u32,
                options: u32,
                mask: u64,
                callback: TapCallback,
                user_info: *mut c_void,
            ) -> CFMachPortRef;
            /// 启用/停用 tap。
            fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
            /// 事件发生时的指针位置（全局坐标，原点在左上）。
            fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
        }

        /// 订阅掩码：由共享的 [`SUBSCRIBED_EVENT_TYPES`] 推出——掩码不另写一份，
        /// 否则键盘事件会从两处口径的缝隙里回来（代价见模块注释）。
        pub(super) fn subscribed_mask() -> u64 {
            SUBSCRIBED_EVENT_TYPES
                .iter()
                .fold(0, |mask, (raw, _)| mask | (1_u64 << raw))
        }

        /// 在**当前线程**建立只订阅左键按下/释放的 tap，然后阻塞运行该线程的
        /// run loop。返回 `Err` 只表示 tap 没建起来（未授权是最常见的原因），
        /// 调用方按降级处理。
        ///
        /// 调用方保证：`sink` 不向外抛 panic——它由 C 回调调用，panic 穿过 C
        /// 边界会 abort 进程（兜底在 `spawn_tap`）。
        pub fn listen(sink: impl Fn(ButtonEvent) + 'static) -> Result<(), ListenError> {
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
                // 阻塞运行当前线程的 run loop：订阅的事件从这里投递进 `raw_callback`。
                CFRunLoopRun();
            }
            Ok(())
        }

        /// tap 回调：把订阅到的事件翻译成 [`ButtonEvent`] 交给 sink，未订阅的
        /// 一律原样放行（listen-only 的 tap 也必须返回事件本身）。
        ///
        /// 翻译只用常量查表加一次位置读取（不分配、不调用会 panic 的 API），
        /// panic 兜底在 sink 侧（见 [`listen`] 的调用方保证）。
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
                // SAFETY: `event` 是系统在回调期间借给我们的事件引用，只读位置，
                // 不转移所有权。
                let CGPoint { x, y } = unsafe { CGEventGetLocation(event) };
                sink(ButtonEvent {
                    action,
                    pos: (x, y),
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

    fn pressed(pos: (f64, f64)) -> ButtonEvent {
        ButtonEvent {
            action: LeftButton::Pressed,
            pos,
        }
    }

    fn released(pos: (f64, f64)) -> ButtonEvent {
        ButtonEvent {
            action: LeftButton::Released,
            pos,
        }
    }

    #[test]
    fn drag_release_emits_selection() {
        let mut detector = GestureDetector::default();
        assert!(!detector.feed(pressed((10.0, 10.0))));
        assert!(detector.feed(released((140.0, 30.0))));
    }

    /// 验收标准「普通点击不误触发」：原地点击与阈值内抖动都不产出手势。
    #[test]
    fn plain_click_and_jitter_do_not_trigger() {
        let mut detector = GestureDetector::default();
        assert!(!detector.feed(pressed((10.0, 10.0))));
        assert!(!detector.feed(released((10.0, 10.0))));

        assert!(!detector.feed(pressed((50.0, 50.0))));
        assert!(
            !detector.feed(released((56.0, 55.0))),
            "jitter below threshold"
        );
    }

    /// 位移一律取自按下与释放事件自身的坐标——真实拖拽期间系统只投递「拖拽」
    /// 事件，若位置另有来源（例如缓存最近一次移动），这里的位移会算成 0。
    #[test]
    fn displacement_comes_from_the_events_themselves() {
        let mut detector = GestureDetector::default();
        assert!(!detector.feed(pressed((100.0, 100.0))));
        assert!(
            detector.feed(released((400.0, 100.0))),
            "release position is the release event's own position"
        );
    }

    #[test]
    fn state_resets_after_each_gesture() {
        let mut detector = GestureDetector::default();
        assert!(!detector.feed(pressed((0.0, 0.0))));
        assert!(detector.feed(released((200.0, 0.0))));
        // 第二次拖拽照常工作。
        assert!(!detector.feed(pressed((500.0, 500.0))));
        assert!(detector.feed(released((700.0, 500.0))));
    }

    #[test]
    fn stray_events_are_ignored() {
        let mut detector = GestureDetector::default();
        // 无按下记录的释放、以及双重按下（重置起点而非 panic）。
        assert!(!detector.feed(released((1.0, 1.0))));
        assert!(!detector.feed(pressed((0.0, 0.0))));
        assert!(!detector.feed(pressed((100.0, 100.0))));
        assert!(detector.feed(released((200.0, 100.0))));
    }

    /// 状态机经通道驱动（事件线程的实际用法）。
    #[test]
    fn poll_drains_channel_through_state_machine() {
        let (tx, rx) = unbounded::<ButtonEvent>();
        let mut detector = GestureDetector::default();
        tx.send(pressed((0.0, 0.0))).unwrap();
        tx.send(released((100.0, 0.0))).unwrap();
        assert_eq!(detector.poll(&rx), 1);
        assert_eq!(detector.poll(&rx), 0, "drained queue stays empty");
    }

    /// 订阅面只有左键按下与释放——**键盘事件绝不订阅**，滚轮也不。
    ///
    /// 这条是回归测试：订阅「全部事件」的实现会把按键送进回调，进而调起
    /// 要求主线程的输入法 API（TSM/HIToolbox），以 SIGILL 打死整个进程。
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
        // 掩码断言直接对准生产侧的订阅掩码（经 [`tap::subscribed_mask`] 测试
        // 缝隙，唯一出处）：键盘位必须一位不置，除左键两位外没有别的位。
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

/// L4 opt-in 真机注入测试（OS 事件边界，分层测试说明见 AGENTS.md）：
/// 只在授权真机以 `cargo test -- --ignored` 运行，不进 CI。
#[cfg(test)]
mod injected_gesture_live_tests {
    use std::time::{Duration, Instant};

    use rdev::{Button, EventType};

    use super::tap::MouseSource;
    use crate::events::EventSource;

    /// 验收：辅助功能授权下，注入的拖拽序列（按下→移动→释放）经真实
    /// 系统 tap 被监听并判定为划词手势；监听器未降级。
    ///
    /// 判定的位移来自注入的按下与释放事件各自的坐标：先把光标移到已知起
    /// 点再按下，保证按下点 (100,100) 与释放点 (400,300) 之间确有位移
    /// （真实拖拽同理）。
    #[test]
    #[ignore = "注入真实全局鼠标事件：先把运行测试的终端 App 加入 系统设置→隐私与安全性→辅助功能（未授权时由 live_test_support 快速失败）"]
    fn injected_drag_yields_selection_gesture() {
        crate::live_test_support::require_accessibility("mouse_drag_gesture");
        let (source, degraded) = MouseSource::spawn();
        let mut source = source.expect("tap must start under accessibility grant");

        // 拖拽：先移到 (100,100) 再按下（按下事件自带当时坐标），分五段
        // 移动到 (400,300)，释放。
        rdev::simulate(&EventType::MouseMove { x: 100.0, y: 100.0 }).expect("move injection");
        rdev::simulate(&EventType::ButtonPress(Button::Left)).expect("button press injection");
        for step in 1..=5 {
            rdev::simulate(&EventType::MouseMove {
                x: 100.0 + 60.0 * f64::from(step),
                y: 100.0 + 40.0 * f64::from(step),
            })
            .expect("move injection");
        }
        rdev::simulate(&EventType::ButtonRelease(Button::Left)).expect("button release injection");

        // tap 回调异步进入通道：轮询直到手势出现或超时。
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
