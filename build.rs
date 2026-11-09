//! 构建脚本：系统（Windows）目标把应用图标（Gloss.ico）编译进可执行文件
//! 的资源段——任务栏、桌面快捷方式与文件管理器因此显示品牌图标；其余目标
//! 无可执行文件资源面（系统（macOS）的图标随应用包资源走），构建脚本为
//! 无操作。脚本以构建目标的系统为门，宿主差异不进入判据。
//!
//! 资源编译的调用面还须随宿主门控：目标专属构建依赖按构建目标选择，宿主
//! 为其他系统时编译面没有该依赖，引用必须随宿主系统参与编译（非系统
//! （Windows）宿主上的脚本编译因此跳过调用面）。宿主为系统（Windows）而
//! 构建目标为其他系统时不走本门（脚本以目标为门先退出），组合门下仅剩的
//! 不可达区（系统（Windows）宿主跨构建其他目标）不在本仓库任何流水线上。

#[allow(
    clippy::print_stderr,
    reason = "构建脚本的失败消息只能经标准错误呈现：此刻应用进程尚不存在，日志出口未装配"
)]
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    #[cfg(target_os = "windows")]
    embed_icon();
}

#[cfg(target_os = "windows")]
fn embed_icon() {
    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("assets/icons/Gloss.ico");
    if let Err(err) = resource.compile() {
        eprintln!("compile windows resources (app icon): {err}");
        std::process::exit(1);
    }
}
