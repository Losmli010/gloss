//! 整包原位替换（未适配目标占位）：与两平台实现同名镜像的桩。
//!
//! 系统（macOS）与系统（Windows）各有实现；其余目标一律带平台未支持
//! （PlatformUnsupported）失败——失败缘由如实表达为平台未适配，上层照常
//! 按失败卡降级。

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    PlatformUnsupported,
    Unwritable { dir: PathBuf },
    Unzip(String),
    InvalidStructure,
    Replace(String),
    Relaunch(String),
}

pub fn install(_zip: &Path) -> Result<(), InstallError> {
    Err(InstallError::PlatformUnsupported)
}
