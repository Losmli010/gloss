//! L3 真实二进制冒烟：spawn 编译产物 `gloss --overlay-selftest`，断言
//! 100 轮显隐自检以退出码 0 收尾（见 crates/gloss-app/src/
//! overlay_selftest.rs）。需要窗口服务与 GPU（macOS runner 具备；Linux
//! 无头环境不适用——分层说明见 AGENT.md 测试节）。

#![cfg(target_os = "macos")]

use std::process::Command;

/// 真实二进制 + 真实窗口栈：自检跑满 100 轮并以退出码 0 收尾。
#[test]
fn overlay_selftest_passes_on_the_real_binary() {
    let bin = env!("CARGO_BIN_EXE_gloss");
    let output = Command::new(bin)
        .arg("--overlay-selftest")
        .output()
        .expect("gloss binary should be spawnable");

    assert!(
        output.status.success(),
        "overlay self-test must exit 0 (stderr: {})",
        String::from_utf8_lossy(&output.stderr)
    );
}
