//! Gloss 唯一入口：组装依赖、分发通道端点并启动，不含业务逻辑。

use std::env;
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;

use gloss_app::channel::{AcquireCommand, AppEndpoints, Channels, Event, PlatformEvent, Traced};
use gloss_core::cache::MokaCache;
use gloss_core::config_handle::ConfigHandle;
use gloss_core::engine::AiTaskService;
use gloss_core::log::{self, debug, error, info, thread, warn};
use gloss_core::model::GlossError;
use gloss_core::ports::{AiEngine, AppIcon, ConfigStore, HotkeyBinder};
use gloss_core::task::TaskInput;
use gloss_platform::appearance::MacAppIcon;
use gloss_platform::engine::llm::LlmClient;
use gloss_platform::events::hotkey::HotkeyRegistrar;
use gloss_platform::events::mouse::{MouseGesture, MouseSource};
use gloss_platform::events::{EventSink, EventSource, EventSources};
use gloss_platform::scene::SystemSceneProbe;
use gloss_platform::selection::composite::CompositeReader;
use gloss_platform::storage::CompositeConfigStore;

type StartupResult = Result<(), Box<dyn Error>>;

/// 开发期 Dock 图标：与打包用的 `assets/icons/Gloss.icns` 同一份设计的 PNG
/// 版本（生成方式见 scripts/dev/build-app-icon.sh）。内嵌进二进制而非运行时
/// 读盘——非 bundle 运行时没有资源目录可信（约 15 KB，值这个体积）。
static DOCK_ICON_PNG: &[u8] = include_bytes!("../assets/icons/gloss-dock-icon.png");

fn main() -> StartupResult {
    run()
}

fn run() -> StartupResult {
    init_logging();
    let (config, store) = load_config()?;
    let service = build_service(&config, &store)?;
    run_event_loop(config, store, service)
}

fn init_logging() {
    let dir = log_dir();
    let active = log::init(dir.as_deref());
    let file = active.as_deref().map_or_else(
        || "<stderr only>".to_owned(),
        |dir| dir.display().to_string(),
    );
    info!(thread = thread::UI, log_dir = %file, "gloss starting");
}

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

/// 日志目录：`~/.gloss/logs`（与 justfile 的 logs 配方保持一致）。
fn log_dir() -> Option<PathBuf> {
    let home = env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".gloss").join("logs"))
}

/// 启动期装配产物：配置句柄（给 App 与引擎）与配置存储（给引擎直查密钥）。
type ConfigWiring = (Arc<ConfigHandle>, Arc<dyn ConfigStore>);

/// 装配配置句柄（启动骨架第 2 步）：配置文件走标准配置目录的
/// `config.toml`，密钥走系统安全存储。
///
/// 唯一必须成功的失败是「拿不到配置目录」——那时无处读写配置，属启动硬
/// 错误；文档本身损坏由 `ConfigHandle::load_or_default` 降级为出厂默认
/// 并记日志，应用照常起得来（用户还能进设置页改回来）。
///
/// 返回句柄与存储两份：句柄给 App（任务选项）与引擎（端点），存储给引擎
/// 直查密钥——存储不下沉进句柄，因为密钥不经快照。
fn load_config() -> Result<ConfigWiring, Box<dyn Error>> {
    let store: Arc<dyn ConfigStore> = Arc::new(CompositeConfigStore::new()?);
    let handle = Arc::new(ConfigHandle::load_or_default(Arc::clone(&store)));
    Ok((handle, store))
}

/// 装配推理服务（启动骨架第 6 步）：真实引擎。主产物缓存不在这里——
/// 缓存编排归 gloss-app 的消费桥（`pipeline::start_command_runtime`），
/// 服务只做渲染与转发。
///
/// 引擎构造失败（HTTP/TLS 栈起不来）是启动硬错误——不装配服务就进事件
/// 循环的话，通道③没有消费者，用户触发的任务会静默石沉大海（
/// 「报错退出」而非带病运行）。
fn build_service(
    config: &Arc<ConfigHandle>,
    store: &Arc<dyn ConfigStore>,
) -> Result<Arc<AiTaskService>, Box<dyn Error>> {
    let engine = LlmClient::new(Arc::clone(config), Arc::clone(store))?;
    Ok(Arc::new(AiTaskService::new(
        Arc::new(engine) as Arc<dyn AiEngine>
    )))
}

/// 组装事件循环：拆分四通道端点、主线程按配置建热键 registrar、装配推理
/// 服务与消费运行时、启动应用，并在拿到唤醒句柄后启动平台事件线程。
///
/// 端点分发：App 持有 ① 收 / ② 发 / ③ 发 / ④ 收；事件线程持有 ① 发 /
/// ② 收 / ④ 发（组装进 sink）；tokio 消费循环持有 ③ 收 / ④ 发。配置侧：
/// 句柄给 App（任务选项）与引擎（端点），存储另路给 App（设置页写
/// keychain）与引擎（每请求直查密钥）。热键侧：同一个 registrar 分两路
/// ——pump 给事件线程抽干按键队列，`HotkeyBinder` 端口给 App 在设置页
/// 保存后重注册（注册的线程亲和约束见 hotkey.rs 模块注释）。
fn run_event_loop(
    config: Arc<ConfigHandle>,
    store: Arc<dyn ConfigStore>,
    service: Arc<AiTaskService>,
) -> StartupResult {
    let gloss_app::channel::Channels {
        platform_events,
        acquire_commands,
        commands,
        events,
    } = create_channels();
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
    // 退出协议不变量：通道②的 Sender 只允许 App 经 endpoints 持有。原始
    // 句柄若存活到 join（Rust 的 drop 发生在作用域结束而非最后使用点），
    // 事件线程永远看不到 Disconnected，run_app 返回后会卡死在 join。
    drop(acquire_tx);

    // 热键 registrar 必须创建在主线程（后端的事件注册与 Drop 清理亲和
    // 创建线程，见 hotkey.rs 模块注释），并存活至进程退出。
    // 绑定取自启动时那份配置快照：出厂默认与设置页改的是同一份
    // 表，本文件不再有第二份写死的默认。
    let registrar = Arc::new(HotkeyRegistrar::new(
        config.snapshot().hotkey_bindings.iter().cloned(),
    ));
    // 设置页保存后 App 要按新配置重注册，而重绑定同样只能在主线程做——
    // 把同一个 registrar 以端口形态再给 App 一份句柄（是同一个管理器，
    // 不是第二个；第二个会与它抢热键）。
    let hotkeys = Arc::clone(&registrar) as Arc<dyn HotkeyBinder>;

    // 系统语言只在启动期读一次（改系统语言要重启）：配置里的
    // `Language::System` 要拿它落定成具体的 `Locale`（prompt 模板语言与
    // 界面文案表共用）。适配器对「拿不到偏好语言」按英文兜底，因此这里
    // 没有失败面。
    let system_locale = gloss_platform::locale::system_locale();
    info!(
        thread = thread::UI,
        locale = ?system_locale,
        "system language resolved"
    );

    // 触发前场景探针：安全输入态与前台应用都由系统查询回答（纯查询、
    // 不索取新权限）。与系统语言同类——平台适配器的事，壳只消费。
    let scene = Arc::new(SystemSceneProbe);

    let mut command_runtime = None;
    let mut event_thread = None;
    // 桥的分类阶段也要读配置快照：这里先拆一份，主句柄照旧交给 App。
    let bridge_config = Arc::clone(&config);
    let result = gloss_app::app::run(
        endpoints,
        config,
        store,
        hotkeys,
        scene,
        system_locale,
        |waker| {
            // Dock 图标在这里装：macOS 的 NSApplication 单例只允许在 EventLoop
            // 建好之后访问，而本回调是主线程上第一个满足该时机的点（app::run
            // 建完 EventLoop 就回调它，早于任何窗口创建）。
            install_app_icon();
            // tokio 消费桥在拿到唤醒句柄后再启动：回传事件入队时要靠它唤醒
            // 睡在事件循环里的主线程。主产物缓存与配置句柄在这里交给桥
            // （编排见 gloss_app::pipeline）。运行时存活至 run_event_loop
            // 结束——App drop 关闭通道③后，消费循环自行退出。
            let runtime_waker = waker.clone();
            let cache: Arc<dyn gloss_core::ports::Cache> = Arc::new(MokaCache::new());
            match gloss_app::pipeline::start_command_runtime(
                service,
                cache,
                bridge_config,
                commands_rx,
                events_tx.clone(),
                move || {
                    runtime_waker.wake();
                },
            ) {
                Ok(runtime) => command_runtime = Some(runtime),
                Err(err) => error!(
                    thread = thread::UI,
                    error = %err,
                    "failed to start command runtime, inference disabled"
                ),
            }
            // 事件线程同样在拿到唤醒句柄后再启动：sink 发送产物时要靠它唤醒
            // 睡在事件循环里的主线程。
            let sink = EventSink::new(events_tx, platform_tx, move || {
                waker.wake();
            });
            event_thread = Some(gloss_platform::events::spawn(
                acquire_rx,
                sink,
                acquire_command_handler(),
                event_sources(&registrar),
            ));
        },
    );

    // App 已随事件循环结束 drop：通道②仅剩的 Sender（endpoints 内）归还
    // 后事件线程看到 Disconnected 自行退出；通道③关闭后消费循环退出，
    // CommandRuntime drop 在超时内收尾 tokio。
    if let Some(event_thread) = event_thread {
        event_thread.join();
    }
    drop(command_runtime);
    result
}

fn create_channels() -> Channels {
    // 通道创建与端点分发都在组装点完成。
    Channels::new()
}

/// 事件源集合：热键泵、划词手势与监听降级提示。
fn event_sources(registrar: &HotkeyRegistrar) -> EventSources<PlatformEvent> {
    let mut sources: EventSources<PlatformEvent> = Vec::new();

    // 热键：registrar 在主线程创建（亲和约束），只把 pump 下发事件线程。
    let pump = registrar.pump();
    sources.push(Box::new(move || {
        pump.poll()
            .into_iter()
            .map(|binding| PlatformEvent::HotkeyTriggered { binding })
            .collect()
    }));

    let (mouse_source, degraded) = MouseSource::spawn();
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
    // 监听降级的一次性提示：标志由 tap 线程异步置位（如未授权辅助功能），
    // 事件线程轮询到即告警一次。
    let mut hinted = false;
    sources.push(Box::new(move || {
        if !hinted && degraded.load(std::sync::atomic::Ordering::Relaxed) {
            hinted = true;
            warn!(
                thread = thread::EVENT,
                "mouse listener degraded, selection gesture disabled"
            );
        }
        Vec::new()
    }));
    sources
}

/// 通道②消费处理器：取材命令 → 组合读取 → ④ 回传，运行在事件线程上
/// 顺序执行。读取器提升进闭包复用（当前无状态，为将来缓存留位）。
fn acquire_command_handler()
-> impl FnMut(Traced<AcquireCommand>, &EventSink<Event, PlatformEvent>) + Send {
    let mut reader = CompositeReader::new();
    move |job, sink| {
        // 进入触发点建好的任务 span：本处理器（含取材读选区、剪贴板兜底）
        // 的日志自动带上 `generation`。
        let _entered = job.span.enter();
        let AcquireCommand::AcquireText { generation, kind } = job.payload else {
            debug!(
                thread = thread::EVENT,
                "capture region command not wired yet, dropped"
            );
            return;
        };
        info!(
            thread = thread::EVENT,
            kind = ?kind,
            "acquiring text"
        );
        match reader.read() {
            Ok(text) => {
                // 只记形态不记原文：选区是用户敏感内容，不落进日志文件。
                debug!(
                    thread = thread::EVENT,
                    kind = ?kind,
                    bytes = text.len(),
                    chars = text.chars().count(),
                    "text input acquired"
                );
                sink.send_event(Event::InputReady {
                    generation,
                    input: TaskInput::Text { text, hint: None },
                });
            }
            // 失败也回传（TaskFailed），主线程与用户不至无感；日志分级：
            // 权限缺失值得引导授权（warn），其余是日常路径（debug）。
            Err(err) => {
                sink.send_event(Event::TaskFailed {
                    generation,
                    error: err.clone(),
                });
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
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_bundle_is_created() {
        let channels = create_channels();
        channels
            .acquire_commands
            .tx
            .send(Traced::untraced(AcquireCommand::AcquireText {
                generation: 1,
                kind: gloss_core::task::TaskKind::TranslateWord,
            }))
            .expect("acquire channel must accept commands");
    }
}
