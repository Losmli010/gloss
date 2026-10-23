//! gloss-platform 测试桩库：本 crate 测试所需的端口替身副本。
//!
//! 按被替身的对象分文件：`ports`（端口桩）。桩按 crate 自持、跨 crate
//! 刻意不共享（一份桩的行为变化不得静默改写另一个 crate 测试套件的
//! 语义）：本文件是 gloss-core 的 `tests/stubs/ports.rs` 同名桩的子集（只留
//! 本 crate 用到的），同名桩逐字一致；改注入语义时两边同步。当前仅
//! `src/` 内联单测使用（lib.rs 以 `#[cfg(test)] #[path]` 包含为
//! `crate::stubs`）。本模块按生产代码对待（注释规则同 `src/`，
//! 见 AGENTS.md）。

#![allow(dead_code)]

use std::sync::{Mutex, MutexGuard, PoisonError};

pub mod ports;

fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
