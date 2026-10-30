//! 桌面应用（Gloss）唯一入口：组装依赖、分发通道端点并启动，不含
//! 业务逻辑。组装段助手（通道分发、预检、事件源与后台装配）见组装
//! 模块。

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
use gloss_core::ports::{AiEngine, ConfigStore, SecretStore};
use gloss_platform::scene::SystemSceneProbe;
use gloss_platform::storage::CompositeConfigStore;
use mimalloc::MiMalloc;

type StartupResult = Result<(), Box<dyn Error>>;

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

fn log_dir() -> Option<PathBuf> {
    resolve_log_dir(env::var_os("GLOSS_LOG_DIR"), env::var_os("HOME"))
}

fn resolve_log_dir(override_dir: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    if let Some(dir) = override_dir.filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let home = home?;
    Some(PathBuf::from(home).join(".gloss").join("logs"))
}

type ConfigWiring = (Arc<ConfigHandle>, Arc<dyn SecretStore>);

fn load_config() -> Result<ConfigWiring, Box<dyn Error>> {
    let composite = Arc::new(CompositeConfigStore::new()?);
    let handle = Arc::new(ConfigHandle::load_or_default(
        Arc::clone(&composite) as Arc<dyn ConfigStore>
    ));
    Ok((handle, Arc::clone(&composite) as Arc<dyn SecretStore>))
}

fn build_service(
    config: &Arc<ConfigHandle>,
    store: &Arc<dyn SecretStore>,
) -> Result<Arc<AiTaskService>, Box<dyn Error>> {
    let engine = LlmClient::new(Arc::clone(config), Arc::clone(store))?;
    Ok(Arc::new(AiTaskService::new(
        Arc::new(engine) as Arc<dyn AiEngine>
    )))
}

fn run_event_loop(
    config: Arc<ConfigHandle>,
    store: Arc<dyn SecretStore>,
    service: Arc<AiTaskService>,
) -> StartupResult {
    let (endpoints, halves) = wiring::distribute_channels();
    let system_locale = wiring::resolve_system_locale();
    let scene = Arc::new(SystemSceneProbe);
    let update_handle = gloss_app::update::start_once();
    let update = gloss_app::update::UpdateWiring::from_handle(update_handle);
    let (accessibility_ready, mouse_source, mouse_degraded) = wiring::preflight_and_tap();
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
