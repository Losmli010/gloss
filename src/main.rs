//! Gloss 唯一入口：组装依赖、分发通道端点并启动，不含业务逻辑。

use std::env;
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;

use gloss_app::channel::{AcquireCommand, AppEndpoints, Channels, Event, PlatformEvent};
use gloss_core::cache::MokaCache;
use gloss_core::config_handle::ConfigHandle;
use gloss_core::engine::AiTaskService;
use gloss_core::log::{self, debug, error, info, thread};
use gloss_core::ports::{AiEngine, ConfigStore};
use gloss_platform::engine::llm::LlmClient;
use gloss_platform::events::hotkey::HotkeyRegistrar;
use gloss_platform::events::{EventSink, EventSources};
use gloss_platform::storage::CompositeConfigStore;

#[cfg(any(target_os = "macos", target_os = "windows"))]
use gloss_core::log::warn;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use gloss_core::model::GlossError;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use gloss_core::task::TaskInput;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use gloss_platform::events::EventSource;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use gloss_platform::events::mouse::MouseSource;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use gloss_platform::selection::composite::CompositeReader;

type StartupResult = Result<(), Box<dyn Error>>;

fn main() -> StartupResult {
    run()
}

fn run() -> StartupResult {
    init_logging();
    let (config, store) = load_config()?;
    let service = build_service(&config, &store)?;
    run_event_loop(config, service)
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

/// 日志目录：三平台统一 `~/.gloss/logs`（Windows 下 `HOME` 通常缺失，退回 `USERPROFILE`）。
fn log_dir() -> Option<PathBuf> {
    let home = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".gloss").join("logs"))
}

/// 启动期装配产物：配置句柄（给 App 与引擎）与配置存储（给引擎直查密钥）。
type ConfigWiring = (Arc<ConfigHandle>, Arc<dyn ConfigStore>);

/// 装配配置句柄（启动骨架第 2 步，06 §3.3）：配置文件走标准配置目录的
/// `config.toml`，密钥走系统安全存储（M4-T2）。
///
/// 唯一必须成功的失败是「拿不到配置目录」——那时无处读写配置，属启动硬
/// 错误；文档本身损坏由 `ConfigHandle::load_or_default` 降级为出厂默认
/// 并记日志，应用照常起得来（用户还能进设置页改回来）。
///
/// 返回句柄与存储两份：句柄给 App（任务选项）与引擎（端点），存储给引擎
/// 直查密钥——存储不下沉进句柄，因为密钥不经快照（06 §6.3）。
fn load_config() -> Result<ConfigWiring, Box<dyn Error>> {
    let store: Arc<dyn ConfigStore> = Arc::new(CompositeConfigStore::new()?);
    let handle = Arc::new(ConfigHandle::load_or_default(Arc::clone(&store)));
    Ok((handle, store))
}

/// 装配推理服务（启动骨架第 6 步）：真实引擎 + moka 缓存。
///
/// 引擎构造失败（HTTP/TLS 栈起不来）是启动硬错误——不装配服务就进事件
/// 循环的话，通道③没有消费者，用户触发的任务会静默石沉大海（06 §3.3
/// 「报错退出」而非带病运行）。
fn build_service(
    config: &Arc<ConfigHandle>,
    store: &Arc<dyn ConfigStore>,
) -> Result<Arc<AiTaskService>, Box<dyn Error>> {
    let engine = LlmClient::new(Arc::clone(config), Arc::clone(store))?;
    Ok(Arc::new(AiTaskService::new(
        Arc::new(engine) as Arc<dyn AiEngine>,
        Arc::new(MokaCache::new()),
    )))
}

/// 组装事件循环：拆分四通道端点、主线程创建热键 registrar、装配推理
/// 服务与消费运行时、启动应用，并在拿到唤醒句柄后启动平台事件线程。
///
/// 端点分发：App 持有 ① 收 / ② 发 / ③ 发 / ④ 收；事件线程持有 ① 发 /
/// ② 收 / ④ 发（组装进 sink）；tokio 消费循环持有 ③ 收 / ④ 发。
fn run_event_loop(config: Arc<ConfigHandle>, service: Arc<AiTaskService>) -> StartupResult {
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

    // 热键 registrar 必须创建在主线程（Windows 后端的 WM_HOTKEY 投递与
    // Drop 清理亲和创建线程，见 hotkey.rs 模块注释），并存活至进程退出。
    let registrar = HotkeyRegistrar::with_defaults();

    let mut command_runtime = None;
    let mut event_thread = None;
    let result = gloss_app::app::run(endpoints, config, |waker| {
        // tokio 消费桥在拿到唤醒句柄后再启动：回传事件入队时要靠它唤醒
        // 睡在事件循环里的主线程。运行时存活至 run_event_loop 结束——
        // App drop 关闭通道③后，消费循环自行退出。
        let runtime_waker = waker.clone();
        match gloss_app::pipeline::start_command_runtime(
            service,
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
    });

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
    // 通道创建与端点分发都在组装点完成（08 §4.3）。
    Channels::new()
}

/// 事件源集合：热键泵常驻（Linux 上 registrar 自身已降级，源无害），
/// 划词手势与监听降级提示仅目标平台挂载。
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

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let (mouse_source, degraded) = MouseSource::spawn();
        if let Some(mut source) = mouse_source {
            sources.push(Box::new(move || {
                source
                    .poll()
                    .into_iter()
                    .map(|_| PlatformEvent::SelectionGesture)
                    .collect()
            }));
        }
        // 监听降级的一次性提示：标志由 tap 线程异步置位（如 macOS 未授权
        // 辅助功能），事件线程轮询到即告警一次。
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
    }
    sources
}

/// 通道②消费处理器：取材命令 → 组合读取 → ④ 回传，运行在事件线程上
/// 顺序执行。读取器提升进闭包复用（当前无状态，为将来缓存留位）。
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn acquire_command_handler() -> impl FnMut(AcquireCommand, &EventSink<Event, PlatformEvent>) + Send
{
    let mut reader = CompositeReader::new();
    move |command, sink| {
        let AcquireCommand::AcquireText { generation, kind } = command else {
            debug!(
                thread = thread::EVENT,
                "capture region command not wired yet, dropped"
            );
            return;
        };
        info!(
            thread = thread::EVENT,
            generation = generation,
            kind = ?kind,
            "acquiring text"
        );
        match reader.read() {
            Ok(text) => {
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

/// Linux 只跑 CI：无取材实现，命令只留诊断痕迹。
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn acquire_command_handler() -> impl FnMut(AcquireCommand, &EventSink<Event, PlatformEvent>) + Send
{
    move |command, _sink| {
        let AcquireCommand::AcquireText { kind, .. } = command else {
            return;
        };
        debug!(
            thread = thread::EVENT,
            kind = ?kind,
            "selection reader unavailable on this platform, command dropped"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 事件循环一旦进入就不返回，所以冒烟测试只覆盖日志初始化与通道创建。
    #[test]
    fn channels_bundle_is_created() {
        let channels = create_channels();
        // 发送端立即可用（接收端在同一结构里），不 panic 即可通过。
        channels
            .acquire_commands
            .tx
            .send(AcquireCommand::AcquireText {
                generation: 1,
                kind: gloss_core::task::TaskKind::TranslateWord,
            })
            .expect("acquire channel must accept commands");
    }
}
