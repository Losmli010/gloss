//! Gloss 唯一入口：按七步顺序组装依赖并启动，不含业务逻辑。

use std::error::Error;

use gloss_core::log::{info, thread};

type StartupResult = Result<(), Box<dyn Error>>;

fn main() -> StartupResult {
    run()
}

fn run() -> StartupResult {
    init_logging();
    load_config()?;
    create_channels()?;
    start_platform_event_thread()?;
    start_tokio_runtime()?;
    assemble_adapters()?;
    run_event_loop()
}

fn init_logging() {
    gloss_core::log::init();
    info!(thread = thread::UI, "gloss starting");
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
    // TODO(M1-T3): winit 事件循环，不返回
    Ok(())
}
