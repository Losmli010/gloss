//! 整包原位替换（未适配目标占位）：与两平台实现同名镜像的桩。
//!
//! 系统（macOS）与系统（Windows）各有实现；其余目标一律带非应用包
//! （NotABundle）失败——错误形态走“不在可替换的应用包内”，上层照常
//! 按失败卡降级。

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    NotABundle,
    Unwritable { dir: PathBuf },
    Unzip(String),
    InvalidStructure,
    Replace(String),
    Relaunch(String),
}

pub fn install(_zip: &Path) -> Result<(), InstallError> {
    Err(InstallError::NotABundle)
}
