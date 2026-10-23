//! Gloss 唯一入口：组装依赖、分发通道端点并启动，不含业务逻辑。
//! 组装段助手（通道分发、预检、事件源与后台装配）见 `wiring`。

mod wiring;

use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use gloss_core::config_handle::ConfigHandle;
use gloss_core::engine::AiTaskService;
use gloss_core::engine::llm::LlmClient;
use gloss_core::log::{self, info, thread, warn};
use gloss_core::ports::{AiEngine, ConfigStore};
use gloss_platform::scene::SystemSceneProbe;
use gloss_platform::storage::CompositeConfigStore;
use mimalloc::MiMalloc;

type StartupResult = Result<(), Box<dyn Error>>;

/// 全局分配器：长驻进程的常驻内存贴着活跃字节走（取舍见 Cargo.toml 依赖注）。
#[global_allocator]
static GLOBAL_ALLOCATOR: MiMalloc = MiMalloc;

fn main() -> StartupResult {
    run()
}

fn run() -> StartupResult {
    log::milestone("01_entry");
    init_logging();
    log::milestone("02_logging");
    let (config, store) = load_config()?;
    log::milestone("03_config");
    let service = build_service(&config, &store)?;
    log::milestone("04_engine");
    run_event_loop(config, store, service)
}

fn init_logging() {
    let dir = log_dir();
    let active = log::init(dir.as_deref());
    let file = active.as_deref().map_or_else(
        || "<stderr only>".to_owned(),
        |dir| dir.display().to_string(),
    );
    if active.is_some() {
        info!(thread = thread::UI, log_dir = %file, "gloss starting");
    } else {
        warn!(
            thread = thread::UI,
            log_dir = %file,
            "gloss starting without file logging"
        );
    }
}

/// 日志目录：`GLOSS_LOG_DIR` 优先（量化脚本用它把测量日志隔离出日常日志），
/// 缺省 `~/.gloss/logs`（与 justfile 的 logs 配方保持一致）。
fn log_dir() -> Option<PathBuf> {
    resolve_log_dir(env::var_os("GLOSS_LOG_DIR"), env::var_os("HOME"))
}

/// 日志目录解析：覆盖值非空即用，否则回落 home 下的 `.gloss/logs`。
fn resolve_log_dir(override_dir: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    if let Some(dir) = override_dir.filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let home = home?;
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

/// 组装事件循环：拆分四通道端点（见 `wiring::distribute_channels`）、做
/// 启动期预检，随后启动应用——EventLoop 建好后的后台装配（tokio 消费桥
/// 与事件线程）在 `wiring::Assembly` 里进行，存活句柄记回组装态，循环
/// 结束后在这里统一收尾。
fn run_event_loop(
    config: Arc<ConfigHandle>,
    store: Arc<dyn ConfigStore>,
    service: Arc<AiTaskService>,
) -> StartupResult {
    let (endpoints, halves) = wiring::distribute_channels();
    let system_locale = wiring::resolve_system_locale();
    // 触发前场景探针：安全输入态与前台应用都由系统查询回答（纯查询、
    // 不索取新权限）。与系统语言同类——平台适配器的事，壳只消费。
    let scene = Arc::new(SystemSceneProbe);
    // 启动钩子：更新子系统的进程内幂等入口，只拉起模块任务——静默检查
    // 在首帧就绪后由壳补发，发现新版仅置状态由设置页提示，失败落 Failed
    // 供被动渲染，不打扰划词。
    let update_handle = gloss_app::update::start_once();
    let update = gloss_app::update::UpdateWiring::from_handle(update_handle);
    let (accessibility_ready, mouse_source, mouse_degraded) = wiring::preflight_and_tap();
    // 密钥预热据此门控：未就绪即辅助功能引导对话框在途，keychain 授权框
    // 不得抢在它之前（引导对话框无落定回调，就绪与否只能取启动时刻的
    // 静态事实），预热推迟到首次任务自然重读。
    let permissions_ready = accessibility_ready;
    let mut assembly = wiring::Assembly {
        service,
        cache_config: Arc::clone(&config),
        runtime_store: Arc::clone(&store),
        update_handle,
        accessibility_ready,
        mouse_source,
        mouse_degraded,
        clipboard_watch_enabled: Arc::new(AtomicBool::new(
            config.snapshot().watch_clipboard_images,
        )),
        command_runtime: None,
        event_thread: None,
    };
    let result = gloss_app::app::run(
        endpoints,
        config,
        store,
        scene,
        system_locale,
        permissions_ready,
        update,
        Arc::clone(&assembly.clipboard_watch_enabled),
        |waker| assembly.on_waker(waker, halves),
    );

    // App 已随事件循环结束 drop：通道②仅剩的 Sender（endpoints 内）归还
    // 后事件线程看到 Disconnected 自行退出；通道③关闭后消费循环退出，
    // CommandRuntime drop 在超时内收尾 tokio。
    if let Some(event_thread) = assembly.event_thread {
        event_thread.join();
    }
    drop(assembly.command_runtime);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    use gloss_app::channel::{AcquireCommand, Channels, Traced};

    #[test]
    fn channels_bundle_is_created() {
        let channels = Channels::new();
        channels
            .acquire_commands
            .tx
            .send(Traced::untraced(AcquireCommand::AcquireText {
                generation: 1,
            }))
            .expect("acquire channel must accept commands");
    }

    #[test]
    fn log_dir_override_wins_over_home() {
        let dir = resolve_log_dir(Some("/tmp/metrics-logs".into()), Some("/Users/dev".into()));
        assert_eq!(dir, Some(PathBuf::from("/tmp/metrics-logs")));
    }

    #[test]
    fn log_dir_falls_back_to_home_dot_gloss() {
        let dir = resolve_log_dir(None, Some("/Users/dev".into()));
        assert_eq!(dir, Some(PathBuf::from("/Users/dev/.gloss/logs")));
    }

    #[test]
    fn log_dir_is_none_without_override_and_home() {
        assert_eq!(resolve_log_dir(None, None), None);
    }

    #[test]
    fn log_dir_empty_override_falls_back_to_home() {
        let dir = resolve_log_dir(Some("".into()), Some("/Users/dev".into()));
        assert_eq!(dir, Some(PathBuf::from("/Users/dev/.gloss/logs")));
    }
}
