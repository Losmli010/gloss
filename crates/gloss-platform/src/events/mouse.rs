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
//! 权限，未授权时监听失败 → 手势功能整体降级，热键路径不受影响。
//! rdev 仅在目标平台（macOS/Windows）编译：Linux 只跑 CI，其 x11 后端是
//! 构建期链接，不能带进 CI 依赖树；手势状态机无平台依赖，照常全平台单测。

use crossbeam_channel::Receiver;

/// 划词手势产物（platform 本地类型；组装点映射为 ① 的平台事件）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseGesture {
    /// 用户完成了一次典型的文本选择动作（拖拽释放）。
    Selection,
}

/// 判定为「拖拽选择」所需的最小位移（屏幕逻辑点）。普通点击的抖动远小于此。
const DRAG_THRESHOLD_PX: f64 = 12.0;

/// tap 回调转发给手势状态机的最小事件集：仅左键按下/释放，坐标取事件
/// 时刻的最新位置。
#[derive(Debug, Clone, Copy, PartialEq)]
struct RawButtonEvent {
    pressed: bool,
    pos: (f64, f64),
}

/// 划词手势状态机：Idle →（左键按下）→ Pressed →（位移超阈值后释放）
/// → 产出 Selection。模块私有——组装点只经 [`MouseSource`] 使用它。
#[derive(Default)]
struct GestureDetector {
    pressed_at: Option<(f64, f64)>,
}

impl GestureDetector {
    /// 抽干原始按键事件流并驱动状态机。
    fn poll(&mut self, raw: &Receiver<RawButtonEvent>) -> Vec<MouseGesture> {
        let mut gestures = Vec::new();
        while let Ok(event) = raw.try_recv() {
            if let Some(gesture) = self.feed(event) {
                gestures.push(gesture);
            }
        }
        gestures
    }

    /// 喂入单个原始事件，返回本轮产出的手势。
    fn feed(&mut self, event: RawButtonEvent) -> Option<MouseGesture> {
        match (event.pressed, self.pressed_at) {
            (true, _) => {
                self.pressed_at = Some(event.pos);
                None
            }
            (false, Some(start)) => {
                self.pressed_at = None;
                let dx = event.pos.0 - start.0;
                let dy = event.pos.1 - start.1;
                (dx * dx + dy * dy > DRAG_THRESHOLD_PX * DRAG_THRESHOLD_PX)
                    .then_some(MouseGesture::Selection)
            }
            // 无按下记录的释放（如监听启动前就按下的拖拽），静默忽略。
            (false, None) => None,
        }
    }
}

/// 事件线程侧的鼠标手势源：tap 原始事件通道 + 状态机。
pub struct MouseSource {
    raw_rx: Receiver<RawButtonEvent>,
    detector: GestureDetector,
}

impl MouseSource {
    /// 抽干一轮原始事件并驱动状态机，返回产出的手势。
    pub fn poll(&mut self) -> Vec<MouseGesture> {
        self.detector.poll(&self.raw_rx)
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod tap {
    use std::thread::JoinHandle;

    use crossbeam_channel::{Receiver, Sender, bounded};
    use rdev::{Button, Event, EventType, listen};

    use gloss_core::log::{debug, info, thread, warn};

    use super::{GestureDetector, MouseSource, RawButtonEvent};

    impl MouseSource {
        /// 启动全局鼠标监听并返回事件线程侧源；监听启动失败返回 `None`
        /// （手势功能降级，见模块注释）。
        pub fn spawn() -> Option<Self> {
            let raw_rx = spawn_tap()?;
            Some(Self {
                raw_rx,
                detector: GestureDetector::default(),
            })
        }
    }

    /// 启动 rdev 全局鼠标监听线程，返回原始按键事件通道；启动失败返回 `None`。
    fn spawn_tap() -> Option<Receiver<RawButtonEvent>> {
        let (tx, rx) = bounded(256);
        let spawned: Result<JoinHandle<()>, _> = std::thread::Builder::new()
            .name("gloss-mouse-tap".into())
            .spawn(move || {
                let mut last_pos: Option<(f64, f64)> = None;
                match listen(move |event: Event| {
                    forward_rdev_event(&event, &mut last_pos, &tx);
                }) {
                    // listen 正常返回只发生在系统层面停止投递时（如 tap 失效），
                    // 留 info 便于诊断线程为何提前结束。
                    Ok(()) => info!(thread = thread::EVENT, "mouse listener stopped"),
                    Err(err) => warn!(
                        thread = thread::EVENT,
                        error = ?err,
                        "mouse listener failed, selection gesture disabled"
                    ),
                }
            });
        match spawned {
            Ok(_join) => Some(rx),
            Err(err) => {
                warn!(
                    thread = thread::EVENT,
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
                thread = thread::EVENT,
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
            assert_eq!(
                detector.poll(&rx),
                vec![super::super::MouseGesture::Selection]
            );
            assert!(detector.poll(&rx).is_empty(), "drained queue stays empty");
        }
    }
}

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
        assert_eq!(detector.feed(raw(true, (10.0, 10.0))), None);
        assert_eq!(
            detector.feed(raw(false, (140.0, 30.0))),
            Some(MouseGesture::Selection)
        );
    }

    /// 验收标准「普通点击不误触发」：原地点击与阈值内抖动都不产出手势。
    #[test]
    fn plain_click_and_jitter_do_not_trigger() {
        let mut detector = GestureDetector::default();
        assert_eq!(detector.feed(raw(true, (10.0, 10.0))), None);
        assert_eq!(detector.feed(raw(false, (10.0, 10.0))), None);

        assert_eq!(detector.feed(raw(true, (50.0, 50.0))), None);
        assert_eq!(
            detector.feed(raw(false, (56.0, 55.0))),
            None,
            "jitter below threshold"
        );
    }

    #[test]
    fn state_resets_after_each_gesture() {
        let mut detector = GestureDetector::default();
        assert_eq!(detector.feed(raw(true, (0.0, 0.0))), None);
        assert_eq!(
            detector.feed(raw(false, (200.0, 0.0))),
            Some(MouseGesture::Selection)
        );
        // 第二次拖拽照常工作。
        assert_eq!(detector.feed(raw(true, (500.0, 500.0))), None);
        assert_eq!(
            detector.feed(raw(false, (700.0, 500.0))),
            Some(MouseGesture::Selection)
        );
    }

    #[test]
    fn stray_events_are_ignored() {
        let mut detector = GestureDetector::default();
        // 无按下记录的释放、以及双重按下（重置起点而非 panic）。
        assert_eq!(detector.feed(raw(false, (1.0, 1.0))), None);
        assert_eq!(detector.feed(raw(true, (0.0, 0.0))), None);
        assert_eq!(detector.feed(raw(true, (100.0, 100.0))), None);
        assert_eq!(
            detector.feed(raw(false, (200.0, 100.0))),
            Some(MouseGesture::Selection)
        );
    }

    /// 状态机经通道驱动（事件线程的实际用法）。
    #[test]
    fn poll_drains_channel_through_state_machine() {
        let (tx, rx) = unbounded::<RawButtonEvent>();
        let mut detector = GestureDetector::default();
        tx.send(raw(true, (0.0, 0.0))).unwrap();
        tx.send(raw(false, (100.0, 0.0))).unwrap();
        assert_eq!(detector.poll(&rx), vec![MouseGesture::Selection]);
        assert!(detector.poll(&rx).is_empty(), "drained queue stays empty");
    }
}
