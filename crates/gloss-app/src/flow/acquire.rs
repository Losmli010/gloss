//! 编排·取材段：平台事件线程上取材侧的业务逻辑——端口事件到通道一事件
//! 的映射（授权观察、划词手势、剪贴板哨兵）、监听降级的一次性提示、取材
//! 命令到读取器的分派与产物回传。回传的时序不变量：先发通道四事件、后
//! 执行还原收尾（收尾不挡在弹窗链路上）。发送面以发送闭包承载、事件源
//! 以普通闭包返回（装箱与事件线程骨架归平台层），本层不认识平台类型。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gloss_core::log::{debug, info, thread, warn};
use gloss_core::model::GlossError;
use gloss_core::ports::{
    ClipboardImageReader, ClipboardWatcher, SelectionGestureSource, TextAcquirer,
};
use gloss_core::task::TaskInput;

use crate::channel::{AcquireCommand, Event, PlatformEvent, Traced};

pub fn event_sources(
    mut watch: Box<dyn FnMut() -> bool + Send>,
    gesture: Option<Box<dyn SelectionGestureSource>>,
    mut clipboard_watcher: Box<dyn ClipboardWatcher>,
    gesture_degraded: Arc<AtomicBool>,
) -> Vec<Box<dyn FnMut() -> Vec<PlatformEvent> + Send>> {
    let mut sources: Vec<Box<dyn FnMut() -> Vec<PlatformEvent> + Send>> = Vec::new();
    sources.push(Box::new(move || {
        watch()
            .then_some(PlatformEvent::AccessibilityGranted)
            .into_iter()
            .collect()
    }));
    if let Some(mut gesture) = gesture {
        sources.push(Box::new(move || {
            gesture
                .poll()
                .into_iter()
                .map(|pos| PlatformEvent::SelectionGesture { pos })
                .collect()
        }));
    }
    sources.push(Box::new(move || {
        clipboard_watcher
            .poll()
            .into_iter()
            .map(|_| PlatformEvent::PasteboardImageObserved)
            .collect()
    }));
    let mut hinted = false;
    sources.push(Box::new(move || {
        if !hinted && gesture_degraded.load(Ordering::Relaxed) {
            hinted = true;
            warn!(
                thread = thread::EVENT,
                "mouse listener degraded mid-run, selection gesture disabled"
            );
        }
        Vec::new()
    }));
    sources
}

pub fn acquire_command_handler(
    mut text_acquirer: Box<dyn TextAcquirer>,
    mut image_reader: Box<dyn ClipboardImageReader>,
) -> impl FnMut(Traced<AcquireCommand>, &mut dyn FnMut(Event)) + Send {
    move |job, send| {
        let _entered = job.span.enter();
        match job.payload {
            AcquireCommand::AcquireText { generation } => {
                acquire_text(&mut *text_acquirer, generation, send);
            }
            AcquireCommand::AcquireClipboardImage { generation } => {
                acquire_clipboard_image(&mut *image_reader, generation, send);
            }
        }
    }
}

fn acquire_text(
    text_acquirer: &mut dyn TextAcquirer,
    generation: u64,
    send: &mut dyn FnMut(Event),
) {
    info!(thread = thread::EVENT, "acquiring text");
    let (outcome, settle) = text_acquirer.read().into_parts();
    match outcome {
        Ok(text) => {
            debug!(
                thread = thread::EVENT,
                bytes = text.len(),
                chars = text.chars().count(),
                "text input acquired"
            );
            send(Event::InputReady {
                generation,
                input: TaskInput::Text { text },
            });
        }
        Err(err) => {
            if err == GlossError::AccessibilityDenied {
                warn!(
                    thread = thread::EVENT,
                    "accessibility permission missing, text acquisition denied"
                );
            } else {
                debug!(
                    thread = thread::EVENT,
                    error = %err,
                    "text acquisition failed"
                );
            }
            send(Event::TaskFailed {
                generation,
                error: err,
            });
        }
    }
    settle.finish();
}

fn acquire_clipboard_image(
    image_reader: &mut dyn ClipboardImageReader,
    generation: u64,
    send: &mut dyn FnMut(Event),
) {
    info!(thread = thread::EVENT, "acquiring clipboard image");
    match image_reader.read() {
        Ok(Some(png)) => {
            debug!(
                thread = thread::EVENT,
                bytes = png.len(),
                "clipboard image acquired"
            );
            send(Event::InputReady {
                generation,
                input: TaskInput::Image { png },
            });
        }
        Ok(None) => {}
        Err(err) => {
            debug!(
                thread = thread::EVENT,
                error = %err,
                "clipboard image acquisition failed"
            );
            send(Event::TaskFailed {
                generation,
                error: err,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::MutexGuard;
    use std::sync::PoisonError;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;

    use gloss_core::model::ScreenPoint;
    use gloss_core::ports::{ClipboardImageObserved, TextRead, TextSettle};

    use super::*;

    type EventLog = Arc<Mutex<Vec<Event>>>;
    type OrderLog = Arc<Mutex<Vec<&'static str>>>;
    type ReadOutcome = Result<Option<Arc<[u8]>>, GlossError>;
    type ReadQueue = VecDeque<ReadOutcome>;

    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(PoisonError::into_inner)
    }

    struct ScriptedTextAcquirer {
        outcome: Result<String, GlossError>,
        restored: OrderLog,
    }

    impl TextAcquirer for ScriptedTextAcquirer {
        fn read(&mut self) -> TextRead {
            let restored = Arc::clone(&self.restored);
            TextRead::new(
                self.outcome.clone(),
                TextSettle::deferred(move || lock(&restored).push("restore")),
            )
        }
    }

    struct QueuedImageReader {
        reads: Mutex<ReadQueue>,
    }

    impl QueuedImageReader {
        fn with_reads(reads: Vec<ReadOutcome>) -> Self {
            Self {
                reads: Mutex::new(reads.into()),
            }
        }
    }

    impl ClipboardImageReader for QueuedImageReader {
        fn read(&mut self) -> Result<Option<Arc<[u8]>>, GlossError> {
            lock(&self.reads).pop_front().unwrap_or(Ok(None))
        }
    }

    struct QueuedGestureSource {
        positions: Mutex<Vec<ScreenPoint>>,
    }

    impl SelectionGestureSource for QueuedGestureSource {
        fn poll(&mut self) -> Vec<ScreenPoint> {
            std::mem::take(&mut lock(&self.positions))
        }
    }

    struct ObservedClipboardWatcher;

    impl ClipboardWatcher for ObservedClipboardWatcher {
        fn poll(&mut self) -> Vec<ClipboardImageObserved> {
            vec![ClipboardImageObserved]
        }
    }

    fn send_log(log: &EventLog) -> impl FnMut(Event) {
        let log = Arc::clone(log);
        move |event: Event| lock(&log).push(event)
    }

    fn send_marker(log: &OrderLog, events: &EventLog) -> impl FnMut(Event) {
        let order = Arc::clone(log);
        let events = Arc::clone(events);
        move |event: Event| {
            lock(&order).push("event");
            lock(&events).push(event);
        }
    }

    fn granted_then_silent() -> Box<dyn FnMut() -> bool + Send> {
        let mut granted = true;
        Box::new(move || std::mem::take(&mut granted))
    }

    #[test]
    fn input_ready_event_precedes_the_clipboard_restore() {
        let order: OrderLog = Arc::default();
        let events: EventLog = Arc::default();
        let mut acquirer = ScriptedTextAcquirer {
            outcome: Ok("demo word".into()),
            restored: Arc::clone(&order),
        };
        let mut send = send_marker(&order, &events);
        acquire_text(&mut acquirer, 7, &mut send);
        assert_eq!(
            *lock(&order),
            ["event", "restore"],
            "the channel-4 event must be sent before the restore settles"
        );
        assert!(matches!(
            lock(&events)[0],
            Event::InputReady {
                generation: 7,
                input: TaskInput::Text { .. }
            }
        ));
    }

    #[test]
    fn accessibility_denied_warns_and_still_fails_the_task() {
        let order: OrderLog = Arc::default();
        let events: EventLog = Arc::default();
        let mut acquirer = ScriptedTextAcquirer {
            outcome: Err(GlossError::AccessibilityDenied),
            restored: Arc::clone(&order),
        };
        let mut send = send_marker(&order, &events);
        let logs = gloss_core::log::capture(|| acquire_text(&mut acquirer, 3, &mut send));
        assert_eq!(
            *lock(&order),
            ["event", "restore"],
            "the failure event must be sent before the restore settles"
        );
        assert!(matches!(
            lock(&events)[0],
            Event::TaskFailed {
                generation: 3,
                error: GlossError::AccessibilityDenied
            }
        ));
        assert!(
            logs.lines().any(|line| line.contains("\"level\":\"WARN\"")
                && line.contains("text acquisition denied")),
            "a denied acquisition must route to the warn level: {logs}"
        );
    }

    #[test]
    fn other_acquire_failures_stay_below_the_warn_level() {
        let events: EventLog = Arc::default();
        let mut acquirer = ScriptedTextAcquirer {
            outcome: Err(GlossError::SelectionUnavailable),
            restored: Arc::default(),
        };
        let mut send = send_log(&events);
        let logs = gloss_core::log::capture(|| acquire_text(&mut acquirer, 4, &mut send));
        assert!(matches!(
            lock(&events)[0],
            Event::TaskFailed {
                generation: 4,
                error: GlossError::SelectionUnavailable
            }
        ));
        assert!(
            !logs.contains("\"level\":\"WARN\""),
            "a daily-path acquisition failure must not escalate to warn: {logs}"
        );
    }

    #[test]
    fn acquire_commands_dispatch_to_the_matching_reader() {
        let events: EventLog = Arc::default();
        let mut handler = acquire_command_handler(
            Box::new(ScriptedTextAcquirer {
                outcome: Ok("demo word".into()),
                restored: Arc::default(),
            }),
            Box::new(QueuedImageReader::with_reads(vec![Ok(Some(Arc::from(
                &b"png4"[..],
            )))])),
        );
        let mut send = send_log(&events);
        handler(
            Traced::untraced(AcquireCommand::AcquireText { generation: 5 }),
            &mut send,
        );
        handler(
            Traced::untraced(AcquireCommand::AcquireClipboardImage { generation: 6 }),
            &mut send,
        );
        let events = lock(&events);
        assert!(matches!(
            events[0],
            Event::InputReady {
                generation: 5,
                input: TaskInput::Text { .. }
            }
        ));
        assert!(matches!(
            events[1],
            Event::InputReady {
                generation: 6,
                input: TaskInput::Image { .. }
            }
        ));
    }

    #[test]
    fn a_clipboard_image_without_data_sends_nothing() {
        let events: EventLog = Arc::default();
        let mut reader = QueuedImageReader::with_reads(vec![Ok(None)]);
        let mut send = send_log(&events);
        acquire_clipboard_image(&mut reader, 8, &mut send);
        assert!(
            lock(&events).is_empty(),
            "no data on the pasteboard: no channel-4 event"
        );
    }

    #[test]
    fn the_grant_watch_answers_one_granted_event_per_true_poll() {
        let mut sources = event_sources(
            granted_then_silent(),
            None,
            Box::new(ObservedClipboardWatcher),
            Arc::default(),
        );
        assert_eq!(sources.len(), 3);
        let mut watch = sources.remove(0);
        assert_eq!(
            watch(),
            vec![PlatformEvent::AccessibilityGranted],
            "a true watch answer maps into one granted event"
        );
        assert_eq!(watch(), vec![], "a false answer produces no event");
    }

    #[test]
    fn gesture_polls_map_into_selection_gesture_events() {
        let sources = event_sources(
            Box::new(|| false),
            Some(Box::new(QueuedGestureSource {
                positions: Mutex::new(vec![ScreenPoint::new(30, 40), ScreenPoint::new(50, 60)]),
            })),
            Box::new(ObservedClipboardWatcher),
            Arc::default(),
        );
        assert_eq!(sources.len(), 4);
        let mut sources = sources.into_iter();
        let mut watch = sources.next().unwrap();
        let mut gesture = sources.next().unwrap();
        assert_eq!(watch(), vec![]);
        assert_eq!(
            gesture(),
            vec![
                PlatformEvent::SelectionGesture {
                    pos: ScreenPoint::new(30, 40)
                },
                PlatformEvent::SelectionGesture {
                    pos: ScreenPoint::new(50, 60)
                },
            ],
            "gesture positions map one-to-one into channel-1 events"
        );
        assert_eq!(gesture(), vec![], "a drained gesture source polls empty");
    }

    #[test]
    fn clipboard_watch_polls_map_into_pasteboard_observations() {
        let sources = event_sources(
            Box::new(|| false),
            None,
            Box::new(ObservedClipboardWatcher),
            Arc::default(),
        );
        let mut sources = sources.into_iter();
        let mut watch = sources.next().unwrap();
        let mut clipboard = sources.next().unwrap();
        assert_eq!(watch(), vec![]);
        assert_eq!(
            clipboard(),
            vec![PlatformEvent::PasteboardImageObserved],
            "a pasteboard observation maps into its channel-1 event"
        );
    }

    #[test]
    fn a_session_without_a_gesture_source_arms_no_gesture_source() {
        let with_gesture = event_sources(
            Box::new(|| false),
            Some(Box::new(QueuedGestureSource {
                positions: Mutex::new(Vec::new()),
            })),
            Box::new(ObservedClipboardWatcher),
            Arc::default(),
        );
        let without = event_sources(
            Box::new(|| false),
            None,
            Box::new(ObservedClipboardWatcher),
            Arc::default(),
        );
        assert_eq!(with_gesture.len(), 4);
        assert_eq!(without.len(), 3);
    }

    #[test]
    fn the_degraded_hint_warns_once_per_flag_rise() {
        let flag = Arc::new(AtomicBool::new(false));
        let sources = event_sources(
            Box::new(|| false),
            None,
            Box::new(ObservedClipboardWatcher),
            Arc::clone(&flag),
        );
        let mut sources = sources.into_iter();
        let _watch = sources.next().unwrap();
        let _clipboard = sources.next().unwrap();
        let mut hint = sources.next().unwrap();
        let logs = gloss_core::log::capture(|| {
            assert!(hint().is_empty(), "the hint source never emits events");
            flag.store(true, Ordering::Relaxed);
            assert!(
                hint().is_empty(),
                "the flag rising arms the hint without emitting"
            );
            assert!(hint().is_empty());
        });
        let warns = logs
            .lines()
            .filter(|line| line.contains("mouse listener degraded"))
            .count();
        assert_eq!(
            warns, 1,
            "the degradation hint warns exactly once per flag rise: {logs}"
        );
    }
}
