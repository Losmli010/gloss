//! gloss-core 测试桩库：端口替身与脚本引擎的唯一存放处。
//!
//! 按被替身的对象分文件：`engine`（[`AiEngine`] 的脚本引擎桩）、
//! `ports`（其余端口桩）。供两类编译上下文共享同一份源：
//! - 集成测试目标（`tests/*.rs` 经 `mod stubs;` 引入）；
//! - `src/` 内联单测（lib.rs 以 `#[cfg(test)] #[path]` 包含为 `crate::stubs`）。
//!
//! 因此本模块统一以 `gloss_core::` 绝对路径引用库条目（lib 侧靠
//! `extern crate self as gloss_core` 让自引用成立）。本模块按生产代码
//! 对待（lint 与注释规则同 `src/`，见 AGENTS.md「注释纪律」）。
//!
//! 桩只承担两件事：**预置返回值**与**可注入的失败**——注入点要能造出想测
//! 的那种时序，观测点要能证明它发生过（见 AGENTS.md「测试」一节）。需要
//! 新能力时扩展本模块，别在测试里手搓 fake。桩按 crate 自持：跨 crate
//! 刻意不共享（各 crate 的 tests/stubs/ 各留所需副本），一份桩的行为变化
//! 不得静默改写另一个 crate 测试套件的语义。

// 桩是按需取用的能力全集：每个编译目标只用到其中一部分，未用能力不算死代码。
#![allow(dead_code)]

use std::sync::{Mutex, MutexGuard, PoisonError};

pub mod engine;
pub mod ports;

/// 锁中毒恢复：测试基建不值得 panic，拿回守卫继续用（数据由测试自身
/// 单线程写入，中毒不可能源于本模块逻辑）。
fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
