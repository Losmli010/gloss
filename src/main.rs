//! 桌面应用（Gloss）唯一入口：组装依赖、分发通道端点并启动，不含
//! 业务逻辑。组装段助手（平台能力装配、配置装配、通道分发、预检、
//! 事件源与后台装配）见组装模块。

mod wiring;

use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use gloss_core::config_handle::ConfigHandle;
use gloss_core::engine::AiTaskService;
use gloss_core::engine::llm::LlmClient;
use gloss_core::log::{self, info, thread, warn};
use gloss_core::ports::{AiEngine, SecretStore};
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
    let platform = wiring::connect()?;
    let (config, store) = wiring::load_config(&platform)?;
    log::milestone("03_config");
    let service = build_service(&config, &store)?;
    log::milestone("04_engine");
    wiring::run_event_loop(platform, config, store, service)
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
    resolve_log_dir(env::var_os("GLOSS_LOG_DIR"), default_log_root())
}

fn resolve_log_dir(
    override_dir: Option<OsString>,
    default_root: Option<PathBuf>,
) -> Option<PathBuf> {
    if let Some(dir) = override_dir.filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let root = default_root?;
    Some(root.join("logs"))
}

#[cfg(target_os = "macos")]
fn default_log_root() -> Option<PathBuf> {
    env::var_os("HOME").map(|home| PathBuf::from(home).join(".gloss"))
}

#[cfg(target_os = "windows")]
fn default_log_root() -> Option<PathBuf> {
    env::var_os("LOCALAPPDATA").map(|base| PathBuf::from(base).join("gloss"))
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
        let dir = resolve_log_dir(
            Some("/tmp/metrics-logs".into()),
            Some("/Users/dev/.gloss".into()),
        );
        assert_eq!(dir, Some(PathBuf::from("/tmp/metrics-logs")));
    }

    #[test]
    fn log_dir_falls_back_to_home_dot_gloss() {
        let dir = resolve_log_dir(None, Some("/Users/dev/.gloss".into()));
        assert_eq!(dir, Some(PathBuf::from("/Users/dev/.gloss/logs")));
    }

    #[test]
    fn log_dir_is_none_without_override_and_home() {
        assert_eq!(resolve_log_dir(None, None), None);
    }

    #[test]
    fn log_dir_empty_override_falls_back_to_home() {
        let dir = resolve_log_dir(Some("".into()), Some("/Users/dev/.gloss".into()));
        assert_eq!(dir, Some(PathBuf::from("/Users/dev/.gloss/logs")));
    }
}
