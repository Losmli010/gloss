//! 整包原位替换（系统（Windows）占位）：与系统（macOS）实现同名镜像的桩。
//!
//! 系统（Windows）的替换规格（可执行文件（exe）加旁置动态库（dll）的
//! 压缩包（zip）结构校验、运行中可执行文件的重命名交换）按移植方案在
//! 实施前另立定案，本桩一律带非应用包（NotABundle）失败——错误形态走
//! “不在可替换的应用包内”，上层照常按失败卡降级。

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
