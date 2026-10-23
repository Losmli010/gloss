//! 组装段助手：四通道端点分发、启动期预检与事件源装配、tokio 消费桥与
//! 事件线程的启动（[`Assembly`]）。`main` 只留启动骨架与配置/服务装配，
//! 「拿到唤醒句柄之后才能做的事」都收在这里。

use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use tokio::sync::mpsc::UnboundedReceiver;

use gloss_app::app::Waker;
use gloss_app::channel::{
    AcquireCommand, AppEndpoints, Channels, Command, Event, PlatformEvent, Traced,
};
use gloss_app::runtime::cache::TaskCache;
use gloss_app::runtime::pipeline::{CommandRuntime, TaskBridge, start_command_runtime};
use gloss_app::update::UpdateHandle;
use gloss_core::config::CACHE_TTL_MAX_SECS;
use gloss_core::config_handle::ConfigHandle;
use gloss_core::engine::AiTaskService;
use gloss_core::log::{debug, info, thread, warn};
use gloss_core::model::GlossError;
use gloss_core::ports::{AppIcon, ConfigStore};
use gloss_core::task::TaskInput;
use gloss_platform::appearance::MacAppIcon;
use gloss_platform::clipboard::{ClipboardImageReader, ClipboardWatchSource, WatchRoundFlag};
use gloss_platform::events::mouse::{MouseGesture, MouseSource};
use gloss_platform::events::{EventSink, EventSource, EventSources, EventThread};
use gloss_platform::permissions::{self, AccessibilityWatch};
use gloss_platform::selection::composite::CompositeReader;

type StartupResult = Result<(), Box<dyn Error>>;

/// 开发期 Dock 图标：与打包用的 `assets/icons/Gloss.icns` 同一份设计的 PNG
/// 版本（生成方式见 scripts/dev/build-app-icon.sh）。内嵌进二进制而非运行时
/// 读盘——非 bundle 运行时没有资源目录可信（约 9 KB，值这个体积）。
static DOCK_ICON_PNG: &[u8] = include_bytes!("../assets/icons/gloss-dock-icon.png");

/// 装开发期 Dock 图标（非 bundle 运行时才有实际效果）。失败只降级记日志：
/// 图标是外观，不该拦住启动——端口契约见 `gloss_core::ports::AppIcon`。
fn install_app_icon() {
    let icon: Box<dyn AppIcon> = Box::new(MacAppIcon::new());
    if !icon.install(DOCK_ICON_PNG) {
        warn!(
            thread = thread::UI,
            "dock icon not applied, keeping the system default"
        );
    }
}

/// 启动期权限预检（启动骨架第 1.5 步）：缺失即弹系统授权引导对话框（内
/// 含「打开系统设置」入口）。必须在建 tap 之前调用——授权对话框非阻塞，
/// 先弹引导再装配事件源，避免 tap 建立期的日志与事件被引导动作穿插。
fn preflight_accessibility() -> bool {
    if permissions::preflight_accessibility() {
        info!(
            thread = thread::UI,
            "accessibility permission granted for selection reading"
        );
        true
    } else {
        warn!(
            thread = thread::UI,
            "accessibility permission missing, triggering the system guidance dialog"
        );
        permissions::request_accessibility();
        false
    }
}

/// 启动期预检与事件 tap 建立：先做辅助功能预检与引导（唯一的取材授权项），
/// 再建 tap——tap 的建立结果同步可知（等待是通道事件且有界，无用户交互），
/// tap 在这里先建好，事件线程稍后才消费其产物；等待期间的手势事件在通道
/// 里有界排队。返回启动时刻的授权状态（密钥预热门控的依据）与 tap 产物。
pub(super) fn preflight_and_tap() -> (bool, Option<MouseSource>, Arc<AtomicBool>) {
    let accessibility_ready = preflight_accessibility();
    let (mouse_source, mouse_degraded) = MouseSource::spawn();
    if mouse_source.is_some() {
        info!(
            thread = thread::UI,
            "event tap established for the selection gesture"
        );
    }
    (accessibility_ready, mouse_source, mouse_degraded)
}

/// 系统语言只在启动期读一次（改系统语言要重启）：配置里的
/// `Language::System` 要拿它落定成具体的 `Locale`（prompt 模板语言与界面
/// 文案表共用）。适配器对「拿不到偏好语言」按英文兜底，因此没有失败面。
pub(super) fn resolve_system_locale() -> gloss_core::model::Locale {
    let locale = gloss_platform::locale::system_locale();
    info!(
        thread = thread::UI,
        locale = ?locale,
        "system language resolved"
    );
    locale
}

/// 四通道的线程对端：App 持端点（随 `distribute_channels` 一并返回），
/// 事件线程与 tokio 消费桥持这些原始句柄——组装回调内经 [`Assembly`]
/// 启动时消费。
pub(super) struct ChannelHalves {
    pub(super) platform_tx: Sender<PlatformEvent>,
    pub(super) acquire_rx: Receiver<Traced<AcquireCommand>>,
    pub(super) events_tx: Sender<Event>,
    pub(super) commands_rx: UnboundedReceiver<Traced<Command>>,
}

/// 拆分四通道端点：App 持 ① 收 / ② 发 / ③ 发 / ④ 收；事件线程持 ① 发 /
/// ② 收 / ④ 发（组装进 sink）；tokio 消费循环持 ③ 收 / ④ 发。
///
/// 退出协议不变量：通道②的 Sender 只允许 App 经 endpoints 持有。原始
/// 句柄若存活到 join（Rust 的 drop 发生在作用域结束而非最后使用点），
/// 事件线程永远看不到 Disconnected，run_app 返回后会卡死在 join。
pub(super) fn distribute_channels() -> (AppEndpoints, ChannelHalves) {
    let gloss_app::channel::Channels {
        platform_events,
        acquire_commands,
        commands,
        events,
    } = Channels::new();
    let gloss_app::channel::CrossbeamPair {
        tx: platform_tx,
        rx: platform_rx,
    } = platform_events;
    let gloss_app::channel::CrossbeamPair {
        tx: acquire_tx,
        rx: acquire_rx,
    } = acquire_commands;
    let gloss_app::channel::CrossbeamPair {
        tx: events_tx,
        rx: events_rx,
    } = events;
    let gloss_app::channel::CommandChannel {
        tx: commands_tx,
        rx: commands_rx,
    } = commands;
    let endpoints = AppEndpoints {
        platform_events: platform_rx,
        acquire_commands: acquire_tx.clone(),
        commands: commands_tx,
        events: events_rx,
    };
    drop(acquire_tx);
    (
        endpoints,
        ChannelHalves {
            platform_tx,
            acquire_rx,
            events_tx,
            commands_rx,
        },
    )
}

/// 组装段的在途资源与后台存活句柄：[`Assembly::on_waker`] 在 EventLoop
/// 建好后启动 tokio 消费桥与事件线程，把存活句柄记回，事件循环结束后由
/// 调用方统一收尾。
pub(super) struct Assembly {
    pub(super) service: Arc<AiTaskService>,
    /// 缓存 TTL 的快照来源（App 按值收走主句柄，这里先拆一份）。
    pub(super) cache_config: Arc<ConfigHandle>,
    /// 与 App 共享同一份存储实例：预热读密命中引擎的同一份进程内缓存
    /// （密钥不经快照，见 gloss-app 的 Env）。
    pub(super) runtime_store: Arc<dyn ConfigStore>,
    pub(super) update_handle: &'static UpdateHandle,
    pub(super) accessibility_ready: bool,
    pub(super) mouse_source: Option<MouseSource>,
    pub(super) mouse_degraded: Arc<AtomicBool>,
    /// 剪贴板图片哨兵的共享开关位：按启动快照初始化，一份交给 App（设置页
    /// 保存成功时置位，热切换不重启），一份随组装回调与哨兵源同造。
    pub(super) clipboard_watch_enabled: Arc<AtomicBool>,
    pub(super) command_runtime: Option<CommandRuntime>,
    pub(super) event_thread: Option<EventThread>,
}

impl Assembly {
    /// EventLoop 建好后的后台装配：Dock 图标、更新唤醒出口、tokio 消费桥
    /// 与事件线程。后两者的存活句柄记回 self，事件循环结束后由调用方
    /// 统一收尾。
    pub(super) fn on_waker(&mut self, waker: Waker, halves: ChannelHalves) -> StartupResult {
        let ChannelHalves {
            acquire_rx,
            platform_tx,
            events_tx,
            commands_rx,
        } = halves;
        // Dock 图标在这里装：macOS 的 NSApplication 单例只允许在 EventLoop
        // 建好之后访问，而本回调是主线程上第一个满足该时机的点（app::run
        // 建完 EventLoop 就回调它，早于任何窗口创建）。
        install_app_icon();
        // 更新子系统的唤醒出口：相位迁移时请求设置窗重绘。
        let update_waker = waker.clone();
        self.update_handle.install_wake(move || {
            update_waker.wake_settings();
        });
        self.start_bridge(&events_tx, commands_rx, &waker)?;
        self.start_event_thread(acquire_rx, platform_tx, events_tx, &waker);
        gloss_core::log::milestone("05_assembly");
        Ok(())
    }

    /// tokio 消费桥：任务服务、缓存（缓存站点归桥，TTL 取配置快照、手改
    /// 超出上限的值按上限钳制）与共享密钥存储接进消费循环。回传事件入队
    /// 时靠 waker 唤醒睡在事件循环里的主线程。
    fn start_bridge(
        &mut self,
        events_tx: &Sender<Event>,
        commands_rx: UnboundedReceiver<Traced<Command>>,
        waker: &Waker,
    ) -> Result<(), std::io::Error> {
        let ttl = Duration::from_secs(
            self.cache_config
                .snapshot()
                .cache_ttl_secs
                .min(CACHE_TTL_MAX_SECS),
        );
        let bridge = TaskBridge::new(
            Arc::clone(&self.service),
            Arc::new(TaskCache::with_ttl(ttl)),
            Arc::clone(&self.runtime_store),
            events_tx.clone(),
            {
                let waker = waker.clone();
                move || {
                    waker.wake();
                }
            },
        );
        // 消费桥装配失败是启动硬错误：commands_rx 随失败被 drop，通道③
        // 没有消费者，用户触发的任务会静默石沉大海。与 build_service 同
        // 一失败出口——`?` 上抛，报错退出而非带病运行。
        self.command_runtime = Some(start_command_runtime(bridge, commands_rx)?);
        Ok(())
    }

    /// 事件线程：sink 发送产物时靠 waker 唤醒主线程。剪贴板图片哨兵源与
    /// 取材读取器在此同造：① 观察记录经共享句柄交给读取器比对竞态；取材
    /// 轮次抑制闩锁经共享句柄交给文本取材处理器置位（见 clipboard 模块）。
    fn start_event_thread(
        &mut self,
        acquire_rx: Receiver<Traced<AcquireCommand>>,
        platform_tx: Sender<PlatformEvent>,
        events_tx: Sender<Event>,
        waker: &Waker,
    ) {
        let sink = EventSink::new(events_tx, platform_tx, {
            let waker = waker.clone();
            move || {
                waker.wake();
            }
        });
        let clipboard_watch = ClipboardWatchSource::new(Arc::clone(&self.clipboard_watch_enabled));
        let round_flag = clipboard_watch.round_flag();
        let image_reader = ClipboardImageReader::new(clipboard_watch.observer());
        self.event_thread = Some(gloss_platform::events::spawn(
            acquire_rx,
            sink,
            acquire_command_handler(image_reader, round_flag),
            event_sources(
                self.mouse_source.take(),
                Arc::clone(&self.mouse_degraded),
                self.accessibility_ready,
                clipboard_watch,
            ),
        ));
    }
}

/// 事件源集合：划词手势、剪贴板图片哨兵、监听降级提示与运行中授权观察。
/// tap 的源与失效标志由组装点传入（spawn 在组装段同步完成，启动期失败已
/// 由 [`MouseSource::spawn`] 留 warn 日志并整体降级，这里只覆盖运行中失效）；
/// 授权观察仅在启动预检未就绪的会话武装——授权落定经 ① 通知 App 解除预
/// 热门控并补发被推迟的密钥预热，落定只产出一次。剪贴板哨兵随组装点传入
/// （开关位与观察记录句柄在组装段与取材读取器共享）。
fn event_sources(
    mouse_source: Option<MouseSource>,
    mouse_degraded: Arc<AtomicBool>,
    accessibility_ready: bool,
    clipboard_watch: ClipboardWatchSource,
) -> EventSources<PlatformEvent> {
    let mut sources: EventSources<PlatformEvent> = Vec::new();

    let mut watch = AccessibilityWatch::armed_if(!accessibility_ready);
    sources.push(Box::new(move || {
        watch
            .poll()
            .then_some(PlatformEvent::AccessibilityGranted)
            .into_iter()
            .collect()
    }));

    if let Some(mut source) = mouse_source {
        sources.push(Box::new(move || {
            source
                .poll()
                .into_iter()
                .map(|gesture| match gesture {
                    MouseGesture::Selection { pos } => PlatformEvent::SelectionGesture { pos },
                })
                .collect()
        }));
    }
    // 剪贴板图片哨兵：源内自节流与开关位见 clipboard 模块；观察无载荷，
    // 内容由状态机下发的取材命令②到读取器再取。
    let mut clipboard_watch = clipboard_watch;
    sources.push(Box::new(move || {
        clipboard_watch
            .poll()
            .into_iter()
            .map(|_| PlatformEvent::PasteboardImageObserved)
            .collect()
    }));
    // 监听降级的一次性提示：标志由 tap 线程在**运行中**失效时置位（启动
    // 期失败不经它表达），事件线程轮询到即告警一次。
    let mut hinted = false;
    sources.push(Box::new(move || {
        if !hinted && mouse_degraded.load(Ordering::Relaxed) {
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

/// 通道②消费处理器：取材命令 → 组合读取 → ④ 回传，运行在事件线程上
/// 顺序执行。读取器提升进闭包复用：选区组合读取器（激活器按 pid 记忆
/// 会话内已评估的前台应用）与剪贴板图片读取器（观察句柄与哨兵源在组装
/// 段共享）；取材轮次闩锁句柄进组合读取器——剪贴板兜底写盘的轮次由
/// 读取器自带计数归因置位，哨兵消费时据此区分自身写盘与用户复制。
fn acquire_command_handler(
    mut image_reader: ClipboardImageReader,
    round_flag: WatchRoundFlag,
) -> impl FnMut(Traced<AcquireCommand>, &EventSink<Event, PlatformEvent>) + Send {
    let mut reader = CompositeReader::new(round_flag);
    move |job, sink| {
        // 进入触发点建好的任务 span：本处理器（含取材读选区、剪贴板兜底
        // 与读图）的日志自动带上 `generation`。
        let _entered = job.span.enter();
        match job.payload {
            AcquireCommand::AcquireText { generation } => {
                acquire_text(&mut reader, generation, sink);
            }
            AcquireCommand::AcquireClipboardImage { generation } => {
                acquire_clipboard_image(&mut image_reader, generation, sink);
            }
        }
    }
}

/// 划词取材：组合读取（AX → 激活补读 → 剪贴板兜底）→ ④ 回传。只记形态
/// 不记原文：选区是用户敏感内容，不落进日志文件。失败也回传（TaskFailed），
/// 主线程与用户不至无感；日志分级：权限缺失值得引导授权（warn），其余是
/// 日常路径（debug）。先记日志再回传：错误所有权随事件 move，不再复制
/// 一份。收口兜底轮（恢复写盘与归因）在产物上抛之后执行：恢复不挡在弹窗
/// 链路上。
fn acquire_text(
    reader: &mut CompositeReader,
    generation: u64,
    sink: &EventSink<Event, PlatformEvent>,
) {
    info!(thread = thread::EVENT, "acquiring text");
    let (outcome, settle) = reader.read().into_parts();
    match outcome {
        Ok(text) => {
            debug!(
                thread = thread::EVENT,
                bytes = text.len(),
                chars = text.chars().count(),
                "text input acquired"
            );
            sink.send_event(Event::InputReady {
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
            sink.send_event(Event::TaskFailed {
                generation,
                error: err,
            });
        }
    }
    settle.finish();
}

/// 剪贴板图片取材：读板 → ④ 回传。只记字节数不记内容：图像字节是用户
/// 敏感数据，不落进日志文件（像素尺寸由读取器在各自路径上留痕）。静默
/// 丢弃不弹卡的两种落点——①与②之间剪贴板被覆盖（竞态）、板声明图像却
/// 交付不出数据（粘贴授权被拒等）——分支留痕在读取器内（竞态 debug、
/// 无数据 warn），这里不再复述成因。
fn acquire_clipboard_image(
    image_reader: &mut ClipboardImageReader,
    generation: u64,
    sink: &EventSink<Event, PlatformEvent>,
) {
    info!(thread = thread::EVENT, "acquiring clipboard image");
    match image_reader.read() {
        Ok(Some(png)) => {
            debug!(
                thread = thread::EVENT,
                bytes = png.len(),
                "clipboard image acquired"
            );
            sink.send_event(Event::InputReady {
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
            sink.send_event(Event::TaskFailed {
                generation,
                error: err,
            });
        }
    }
}
