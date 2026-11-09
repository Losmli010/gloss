//! 构建脚本：系统（Windows）目标把应用图标（Gloss.ico）编译进可执行文件
//! 的资源段——任务栏、桌面快捷方式与文件管理器因此显示品牌图标；其余目标
//! 无可执行文件资源面（系统（macOS）的图标随应用包资源走），构建脚本为
//! 无操作。脚本以构建目标的系统为门，宿主差异不进入判据。

#[allow(
    clippy::print_stderr,
    reason = "构建脚本的失败消息只能经标准错误呈现：此刻应用进程尚不存在，日志出口未装配"
)]
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("assets/icons/Gloss.ico");
    if let Err(err) = resource.compile() {
        eprintln!("compile windows resources (app icon): {err}");
        std::process::exit(1);
    }
}
