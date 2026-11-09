//! 桌面快捷方式（外壳链接）的写入封装：已知目录定位与链接文件落盘。
//!
//! 快捷方式是外壳（Shell）对象：组件对象模型（COM）套间内创建外壳链接
//! 对象，填目标路径与图标位置后经持久化文件接口（IPersistFile）落盘为
//! 链接文件（.lnk）。套间由调用线程的既有状态决定（界面主线程在窗口库装配后已是
//! 单线程套间），本模块只按需补初始化并成对卸载。桌面目录走已知目录
//! 接口（SHGetKnownFolderPath），系统分配的内存用后即还。

use std::path::{Path, PathBuf};

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree, IPersistFile,
};
use windows::Win32::UI::Shell::{FOLDERID_Desktop, IShellLinkW, SHGetKnownFolderPath, ShellLink};
use windows::core::{HSTRING, Interface};

use gloss_core::model::GlossError;

pub(crate) fn desktop_dir() -> Result<PathBuf, GlossError> {
    // SAFETY: the known-folder lookup passes the per-user desktop identifier
    // with no flags and no user token override, and the returned path is
    // released by `CoTaskMemFree` below after the string is copied out.
    let folder = unsafe { SHGetKnownFolderPath(&FOLDERID_Desktop, Default::default(), None) }
        .map_err(|err| GlossError::Config(format!("resolve desktop directory: {err}")))?;
    // SAFETY: the folder pointer points at a system-allocated null-terminated
    // UTF-16 string for the duration of the conversion.
    let path = unsafe { folder.to_string() }
        .map_err(|err| GlossError::Config(format!("decode desktop directory: {err}")));
    // SAFETY: `folder` was allocated by `SHGetKnownFolderPath` and is freed
    // exactly once here, after the path string has been copied out.
    unsafe { CoTaskMemFree(Some(folder.as_ptr().cast())) };
    path.map(PathBuf::from)
}

pub(crate) fn write_shortcut(link: &Path, target_exe: &Path) -> Result<(), GlossError> {
    let _apartment = super::com::ComApartment::init_sta_unlogged();
    let link_wide = HSTRING::from(link.to_string_lossy().as_ref());
    let exe_wide = HSTRING::from(target_exe.to_string_lossy().as_ref());
    // SAFETY: the class id is the system shell-link class and the call creates
    // an in-proc instance with no outer aggregate; the returned interface
    // reference is handed to the caller and released by its Drop.
    let entry: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
        .map_err(|err| GlossError::Config(format!("create shell link object: {err}")))?;
    // SAFETY: `entry` is a live in-proc interface reference on this thread's
    // apartment; each setter only reads the wide strings that live through
    // the statement, and no pointer escapes the chain.
    let persist: IPersistFile = unsafe {
        entry
            .SetPath(&exe_wide)
            .map_err(|err| GlossError::Config(format!("set shortcut target: {err}")))?;
        entry
            .SetIconLocation(&exe_wide, 0)
            .map_err(|err| GlossError::Config(format!("set shortcut icon: {err}")))?;
        entry
            .cast()
            .map_err(|err| GlossError::Config(format!("query file persistence: {err}")))?
    };
    // SAFETY: `link_wide` lives through the call; persistence writes the
    // shortcut file at that path and takes no pointer past the call.
    let outcome = unsafe { persist.Save(&link_wide, true) };
    outcome.map_err(|err| GlossError::Config(format!("write shortcut {}: {err}", link.display())))
}
