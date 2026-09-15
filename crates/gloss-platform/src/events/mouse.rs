//! 全局鼠标监听：rdev 事件流 → 划词手势判定。
//!
//! 手势策略取「拖拽释放」（按下 → 位移超阈值 → 释放）：双击会把「打开
//! 链接」这类普通操作误判成划词，修饰键确认需要额外的键盘全局钩子，停留
//! 判定需要定时器——拖拽释放是误触最少的最简状态机。其余策略（含框选
//! `RegionGesture`）待配置化与图像任务落地时引入。
//!
//! 线程约束：rdev 的 `listen` 自建事件 tap 并阻塞运行（回调跑在它自己的
//! RunLoop/消息循环上，满足 08 §7.2 的亲和要求），不能在平台事件线程内
//! 运行，也没有停止 API——监听线程随进程退出消亡。macOS 上需要辅助功能
//! 权限，未授权时监听失败 → 手势功能整体降级，热键路径不受影响；降级经
//! [`MouseSource::spawn`] 返回的标志对外可观测，由组装点做一次性提示。
//!
//! 平台门控与纯逻辑切分：公共类型（[`MouseSource`] / [`MouseGesture`]）
//! 仅在目标平台提供（macOS/Windows；Linux 只跑 CI，无监听能力就没有可
//! 构造的源），拖拽判定的状态机无平台依赖，照常全平台单测。

use crossbeam_channel::Receiver;

/// 判定为「拖拽选择」所需的最小位移，单位是 rdev 坐标（macOS 逻辑点 /
/// Windows 物理像素，不必按 DPI 折算）——两者都远高于普通点击的抖动。
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
const DRAG_THRESHOLD: f64 = 12.0;

/// tap 回调转发给手势状态机的最小事件集：仅左键按下/释放，坐标取事件
/// 时刻的最新位置。
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
struct RawButtonEvent {
    pressed: bool,
    pos: (f64, f64),
}

/// 划词手势状态机：Idle →（左键按下）→ Pressed →（位移超阈值后释放）
/// → 判定一次拖拽选择。模块私有——公共面只经目标平台的 [`MouseSource`]
/// 使用它。
#[derive(Default)]
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
struct GestureDetector {
    pressed_at: Option<(f64, f64)>,
}

#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
impl GestureDetector {
    /// 抽干原始事件流并驱动状态机，返回本轮判定的拖拽选择次数。
    fn poll(&mut self, raw: &Receiver<RawButtonEvent>) -> usize {
        let mut fired = 0;
        while let Ok(event) = raw.try_recv() {
            if self.feed(event) {
                fired += 1;
            }
        }
        fired
    }

    /// 喂入单个原始事件，返回是否判定为一次完整的拖拽选择（释放且位移
    /// 超阈值）。
    fn feed(&mut self, event: RawButtonEvent) -> bool {
        match (event.pressed, self.pressed_at) {
            (true, _) => {
                self.pressed_at = Some(event.pos);
                false
            }
            (false, Some(start)) => {
                self.pressed_at = None;
                let dx = event.pos.0 - start.0;
                let dy = event.pos.1 - start.1;
                dx * dx + dy * dy > DRAG_THRESHOLD * DRAG_THRESHOLD
            }
            // 无按下记录的释放（如监听启动前就按下的拖拽），静默忽略。
            (false, None) => false,
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod tap {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::JoinHandle;

    use crossbeam_channel::{Receiver, Sender, bounded};
    use rdev::{Button, Event, EventType, listen};

    use gloss_core::log::{debug, info, thread, warn};

    use super::{GestureDetector, RawButtonEvent};
    use crate::events::EventSource;

    /// 划词手势产物（platform 本地类型；组装点映射为 ① 的平台事件）。
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum MouseGesture {
        /// 用户完成了一次典型的文本选择动作（拖拽释放）。
        Selection,
    }

    /// 事件线程侧的鼠标手势源：tap 原始事件通道 + 状态机。
    pub struct MouseSource {
        raw_rx: Receiver<RawButtonEvent>,
        detector: GestureDetector,
    }

    impl MouseSource {
        /// 启动全局鼠标监听，返回事件线程侧源与降级标志。监听线程启动
        /// 失败返回 `None`；监听器提前退出（如 macOS 未授权辅助功能）是
        /// 异步发生的，两者都经标志置位对外可观测，由组装点做一次性提示。
        /// 返回 `None` 时手势功能整体降级，热键路径不受影响。
        pub fn spawn() -> (Option<Self>, Arc<AtomicBool>) {
            let degraded = Arc::new(AtomicBool::new(false));
            let raw_rx = spawn_tap(Arc::clone(&degraded));
            if raw_rx.is_none() {
                degraded.store(true, Ordering::Relaxed);
            }
            (
                raw_rx.map(|raw_rx| Self {
                    raw_rx,
                    detector: GestureDetector::default(),
                }),
                degraded,
            )
        }
    }

    impl EventSource<MouseGesture> for MouseSource {
        fn poll(&mut self) -> Vec<MouseGesture> {
            (0..self.detector.poll(&self.raw_rx))
                .map(|_| MouseGesture::Selection)
                .collect()
        }
    }

    /// 启动 rdev 全局鼠标监听线程，返回原始按键事件通道；启动失败返回
    /// `None` 并置位降级标志。
    fn spawn_tap(degraded: Arc<AtomicBool>) -> Option<Receiver<RawButtonEvent>> {
        let (tx, rx) = bounded(256);
        let spawned: Result<JoinHandle<()>, _> = std::thread::Builder::new()
            .name("gloss-mouse-tap".into())
            .spawn(move || {
                let mut last_pos: Option<(f64, f64)> = None;
                match listen(move |event: Event| {
                    // panic 穿过 rdev 的 C 回调会直接 abort 进程：与事件线程
                    // 的 guarded 同一纪律，兜底后监听继续。last_pos 若在
                    // panic 中被破坏，后续事件会重新播种坐标。
                    let _ = catch_unwind(AssertUnwindSafe(|| {
                        forward_rdev_event(&event, &mut last_pos, &tx);
                    }));
                }) {
                    // listen 正常返回只发生在系统层面停止投递时（如 tap 失
                    // 效）：手势从此收不到，置位降级标志并留 info 便于诊断。
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

    /// tap 回调的事件过滤：MouseMove 只更新本地坐标（移动事件频率远超消费
    /// 需求，全量转发会洪泛通道）；左键按下/释放带「事件时刻」坐标入队，
    /// 其余按键与滚轮忽略。
    fn forward_rdev_event(
        event: &Event,
        last_pos: &mut Option<(f64, f64)>,
        tx: &Sender<RawButtonEvent>,
    ) {
        match event.event_type {
            EventType::MouseMove { x, y } => *last_pos = Some((x, y)),
            EventType::ButtonPress(Button::Left) => push_raw(tx, true, *last_pos),
            EventType::ButtonRelease(Button::Left) => push_raw(tx, false, *last_pos),
            _ => {}
        }
    }

    fn push_raw(tx: &Sender<RawButtonEvent>, pressed: bool, last_pos: Option<(f64, f64)>) {
        let Some(pos) = last_pos else {
            // 尚无移动坐标（监听启动前就按住不放的场景），丢弃本次按键事件。
            return;
        };
        // 通道满时丢弃：手势判定不需要完整事件流，反压 tap 回调的代价远大于
        // 丢一次触发机会。
        if tx.try_send(RawButtonEvent { pressed, pos }).is_err() {
            debug!(
                thread = thread::MOUSE_TAP,
                "mouse raw queue full, event dropped"
            );
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::time::SystemTime;

        use crossbeam_channel::unbounded;

        fn rdev_event(event_type: EventType) -> Event {
            Event {
                event_type,
                time: SystemTime::now(),
                name: None,
            }
        }

        fn raw(pressed: bool, pos: (f64, f64)) -> RawButtonEvent {
            RawButtonEvent { pressed, pos }
        }

        /// tap 回调只转发左键事件；MouseMove 仅更新坐标，不产生通道流量；
        /// 按键事件携带的是「事件时刻」的最新位置。
        #[test]
        fn tap_callback_filters_to_left_button_with_event_time_position() {
            let (tx, rx) = unbounded::<RawButtonEvent>();
            let mut last_pos = None;

            forward_rdev_event(
                &rdev_event(EventType::ButtonPress(Button::Left)),
                &mut last_pos,
                &tx,
            );
            assert!(
                rx.try_recv().is_err(),
                "no position known yet, event dropped"
            );

            forward_rdev_event(
                &rdev_event(EventType::MouseMove { x: 30.0, y: 40.0 }),
                &mut last_pos,
                &tx,
            );
            assert!(rx.try_recv().is_err(), "moves never enter the channel");

            forward_rdev_event(
                &rdev_event(EventType::ButtonPress(Button::Left)),
                &mut last_pos,
                &tx,
            );
            assert_eq!(rx.try_recv().unwrap(), raw(true, (30.0, 40.0)));

            forward_rdev_event(
                &rdev_event(EventType::MouseMove { x: 60.0, y: 80.0 }),
                &mut last_pos,
                &tx,
            );
            forward_rdev_event(
                &rdev_event(EventType::ButtonRelease(Button::Left)),
                &mut last_pos,
                &tx,
            );
            assert_eq!(rx.try_recv().unwrap(), raw(false, (60.0, 80.0)));

            // 右键与滚轮不进通道。
            forward_rdev_event(
                &rdev_event(EventType::ButtonPress(Button::Right)),
                &mut last_pos,
                &tx,
            );
            forward_rdev_event(
                &rdev_event(EventType::Wheel {
                    delta_x: 0,
                    delta_y: -3,
                }),
                &mut last_pos,
                &tx,
            );
            assert!(rx.try_recv().is_err());
        }

        /// 端到端：tap 过滤 → 状态机，拖拽释放产出手势。
        #[test]
        fn end_to_end_drag_produces_selection() {
            let (tx, rx) = bounded(16);
            let mut last_pos = None;
            let mut detector = GestureDetector::default();

            for event in [
                EventType::MouseMove { x: 0.0, y: 0.0 },
                EventType::ButtonPress(Button::Left),
                EventType::MouseMove { x: 90.0, y: 0.0 },
                EventType::MouseMove { x: 180.0, y: 0.0 },
                EventType::ButtonRelease(Button::Left),
            ] {
                forward_rdev_event(&rdev_event(event), &mut last_pos, &tx);
            }
            assert_eq!(detector.poll(&rx), 1);
            assert_eq!(detector.poll(&rx), 0, "drained queue stays empty");
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use tap::{MouseGesture, MouseSource};

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;

    fn raw(pressed: bool, pos: (f64, f64)) -> RawButtonEvent {
        RawButtonEvent { pressed, pos }
    }

    #[test]
    fn drag_release_emits_selection() {
        let mut detector = GestureDetector::default();
        assert!(!detector.feed(raw(true, (10.0, 10.0))));
        assert!(detector.feed(raw(false, (140.0, 30.0))));
    }

    /// 验收标准「普通点击不误触发」：原地点击与阈值内抖动都不产出手势。
    #[test]
    fn plain_click_and_jitter_do_not_trigger() {
        let mut detector = GestureDetector::default();
        assert!(!detector.feed(raw(true, (10.0, 10.0))));
        assert!(!detector.feed(raw(false, (10.0, 10.0))));

        assert!(!detector.feed(raw(true, (50.0, 50.0))));
        assert!(
            !detector.feed(raw(false, (56.0, 55.0))),
            "jitter below threshold"
        );
    }

    #[test]
    fn state_resets_after_each_gesture() {
        let mut detector = GestureDetector::default();
        assert!(!detector.feed(raw(true, (0.0, 0.0))));
        assert!(detector.feed(raw(false, (200.0, 0.0))));
        // 第二次拖拽照常工作。
        assert!(!detector.feed(raw(true, (500.0, 500.0))));
        assert!(detector.feed(raw(false, (700.0, 500.0))));
    }

    #[test]
    fn stray_events_are_ignored() {
        let mut detector = GestureDetector::default();
        // 无按下记录的释放、以及双重按下（重置起点而非 panic）。
        assert!(!detector.feed(raw(false, (1.0, 1.0))));
        assert!(!detector.feed(raw(true, (0.0, 0.0))));
        assert!(!detector.feed(raw(true, (100.0, 100.0))));
        assert!(detector.feed(raw(false, (200.0, 100.0))));
    }

    /// 状态机经通道驱动（事件线程的实际用法）。
    #[test]
    fn poll_drains_channel_through_state_machine() {
        let (tx, rx) = unbounded::<RawButtonEvent>();
        let mut detector = GestureDetector::default();
        tx.send(raw(true, (0.0, 0.0))).unwrap();
        tx.send(raw(false, (100.0, 0.0))).unwrap();
        assert_eq!(detector.poll(&rx), 1);
        assert_eq!(detector.poll(&rx), 0, "drained queue stays empty");
    }
}

/// L4 opt-in 真机注入测试（OS 事件边界，分层测试说明见 AGENTS.md）：
/// 只在授权真机以 `cargo test -- --ignored` 运行，不进 CI。
#[cfg(all(any(target_os = "macos", target_os = "windows"), test))]
mod injected_gesture_live_tests {
    use std::time::{Duration, Instant};

    use rdev::{Button, EventType};

    use super::tap::MouseSource;
    use crate::events::EventSource;

    /// 验收：辅助功能授权下，注入的拖拽序列（按下→移动→释放）经真实
    /// 系统 tap 被监听并判定为划词手势；监听器未降级。
    #[test]
    #[ignore = "注入真实全局鼠标事件：先把运行测试的终端 App 加入 系统设                置→隐私与安全性→辅助功能（未授权时由 live_test_support 快速失败）"]
    fn injected_drag_yields_selection_gesture() {
        crate::live_test_support::require_accessibility("mouse_drag_gesture");
        let (source, degraded) = MouseSource::spawn();
        let mut source = source.expect("tap must start under accessibility grant");

        // 拖拽：按下 (100,100)，分五段移动到 (400,300)，释放。
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
