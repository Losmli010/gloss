//! 唯一入口：组装依赖、分发通道 Sender、启动线程与事件循环（06 §3.3）。
//!
//! 只做接线，不含业务逻辑；启动失败一律在进入事件循环前退出，不带病进主循环。

type StartupResult = Result<(), Box<dyn std::error::Error>>;

fn main() {
    if let Err(err) = run() {
        eprintln!("启动失败: {err}"); // TODO(M1-T7): 改用 gloss_core::log::error!
        std::process::exit(1);
    }
}

/// 06 §3.3 七步启动顺序。
fn run() -> StartupResult {
    init_logging()?;
    load_config()?;
    create_channels()?;
    start_platform_event_thread()?;
    start_tokio_runtime()?;
    assemble_port_adapters()?;
    run_event_loop()
}

/// 1. `gloss_core::log::init()`（全进程唯一一次）。
fn init_logging() -> StartupResult {
    // TODO(M1-T7): tracing subscriber + EnvFilter（默认 info，RUST_LOG 覆盖）
    Ok(())
}

/// 2. `ConfigStore::load` → `ArcSwap<Config>` 首份快照。
fn load_config() -> StartupResult {
    // TODO(M4-T1/M4-T3): 读配置（失败用默认值），构造 ArcSwap 快照
    Ok(())
}

/// 3. 创建四条通道并分发 Sender（①②④ crossbeam，③ tokio mpsc）。
fn create_channels() -> StartupResult {
    // TODO(M2-T1): PlatformEvent Sender → 事件源；Event Sender → 事件线程 + tokio
    Ok(())
}

/// 4. 平台事件线程：RunLoop → 注册热键/鼠标事件源 → 通道②消费循环。
fn start_platform_event_thread() -> StartupResult {
    // TODO(M2-T2): 专用线程 + NSRunLoop，热键/鼠标/AX/截图同线程顺序处理
    Ok(())
}

/// 5. tokio 后台 runtime：`Command` 消费 → `AiTaskService` → `AiEngine`。
fn start_tokio_runtime() -> StartupResult {
    // TODO(M3-T6/M4-T4): rt-multi-thread 启动后台任务
    Ok(())
}

/// 6. 组装适配器 → 端口注入（CompositeReader / ScreenCapturer / LlmClient / FileConfigStore）。
fn assemble_port_adapters() -> StartupResult {
    // TODO(M3-T9): 组装只发生在这里，core 不感知具体实现
    Ok(())
}

/// 7. winit 事件循环（主线程，不返回）。
fn run_event_loop() -> StartupResult {
    // TODO(M1-T3): ApplicationHandler + 浮层窗口（M1-T6 显隐闭环）
    Ok(())
}
