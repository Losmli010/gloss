//! gloss-app 测试桩库：本包测试所需的端口替身副本。
//!
//! 按被替身的对象分文件：脚本引擎桩、端口桩。
//! 桩按包自持、跨包刻意不共享（一份桩的行为变化不得静默改写
//! 另一个包测试套件的语义）：引擎桩与 gloss-core 的测试桩目录
//! 同名文件逐字一致；端口桩是其同名桩的子集（只留本包用到的），
//! 同名桩逐字一致。改注入语义时两边同步。文件供两类编译上下文共享同一
//! 份源：集成测试目标（各测试文件引入桩模块）与源码内联
//! 单测（以测试属性并入为桩模块）。
//! 本模块按生产代码对待（注释规则同源码目录，见 AGENTS.md）。

#![allow(dead_code)]

use std::sync::{Mutex, MutexGuard, PoisonError};

pub mod engine;
pub mod ports;

fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
