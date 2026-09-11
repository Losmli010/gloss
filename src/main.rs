//! Gloss 唯一入口：按七步顺序组装依赖并启动，不含业务逻辑。

use std::env;
use std::error::Error;
use std::path::PathBuf;

use gloss_core::log::{self, info, thread};

type StartupResult = Result<(), Box<dyn Error>>;

fn main() -> StartupResult {
    run()
}

fn run() -> StartupResult {
    startup()?;
    run_event_loop()
}

/// 进入事件循环之前的六步启动：任何一步失败都在主循环之前退出，
/// 不允许带病进入（06 §3.3）。
fn startup() -> StartupResult {
    init_logging();
    load_config()?;
    create_channels()?;
    start_platform_event_thread()?;
    start_tokio_runtime()?;
    assemble_adapters()
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

fn load_config() -> StartupResult {
    // TODO(M4-T1): ConfigStore::load → ArcSwap<Config> 首份快照
    Ok(())
}

fn create_channels() -> StartupResult {
    // TODO(M2-T1): 四通道创建 + Sender 分发
    Ok(())
}

fn start_platform_event_thread() -> StartupResult {
    // TODO(M2-T2): 事件线程（RunLoop + 事件源注册 + 通道②消费）
    Ok(())
}

fn start_tokio_runtime() -> StartupResult {
    // TODO(M3-T6): tokio runtime + Command 消费 → AiTaskService
    Ok(())
}

fn assemble_adapters() -> StartupResult {
    // TODO(M3-T9): 适配器注入端口（全进程唯一组装点）
    Ok(())
}

fn run_event_loop() -> StartupResult {
    // 显隐自检：--overlay-selftest 反复显隐 100 次后退出（09 M1-T6 验收入口）
    let self_test = env::args().any(|arg| arg == "--overlay-selftest");
    // 唤醒句柄交给组装点，再由它分发给平台事件线程与 tokio（08 §7.3）
    gloss_app::app::run(self_test, |_waker| {})
}

#[cfg(test)]
mod tests {
    use super::startup;

    /// 事件循环一旦进入就不返回，所以冒烟测试只覆盖它之前的启动步骤。
    #[test]
    fn startup_skeleton_returns_ok() {
        assert!(startup().is_ok());
    }
}
