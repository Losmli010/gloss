//! gloss-platform 测试桩库：本包测试所需的端口替身副本。
//!
//! 按被替身的对象分文件：端口桩。桩按包自持、跨包
//! 刻意不共享（一份桩的行为变化不得静默改写另一个包测试套件的
//! 语义）：本文件是 gloss-core 的同名端口桩的子集（只留
//! 本包用到的），同名桩逐字一致；改注入语义时两边同步。当前仅
//! 源码内联单测使用（以测试属性并入为桩模块）。本模块按生产代码对待
//! （注释规则同源码目录）。

#![allow(dead_code)]

use std::sync::{Mutex, MutexGuard, PoisonError};

pub mod ports;

fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
