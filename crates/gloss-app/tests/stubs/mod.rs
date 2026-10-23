//! gloss-app 测试桩库：本 crate 测试所需的端口替身副本。
//!
//! 按被替身的对象分文件：`engine`（脚本引擎桩）、`ports`（端口桩）。
//! 桩按 crate 自持、跨 crate 刻意不共享（一份桩的行为变化不得静默改写
//! 另一个 crate 测试套件的语义）：`engine.rs` 与 gloss-core 的 tests/stubs/
//! 同名文件逐字一致；`ports.rs` 是其同名桩的子集（只留本 crate 用到的），
//! 同名桩逐字一致。改注入语义时两边同步。文件供两类编译上下文共享同一
//! 份源：集成测试目标（`tests/*.rs` 经 `mod stubs;` 引入）与 `src/` 内联
//! 单测（lib.rs 以 `#[cfg(test)] #[path]` 包含为 `crate::stubs`）。
//! 本模块按生产代码对待（注释规则同 `src/`，见 AGENTS.md）。

#![allow(dead_code)]

use std::sync::{Mutex, MutexGuard, PoisonError};

pub mod engine;
pub mod ports;

fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
