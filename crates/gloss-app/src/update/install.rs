//! 校验通过的整包 → 原位替换当前 bundle 并重启。
//!
//! 替换规格（分发设计 §4.1，仅原位替换当前 bundle，不做安装器迁移）：
//! 解压 zip 到目标目录同卷的临时目录并检查包内 `.app` 结构 → 当前
//! bundle 同卷改名保留为 `.app.old` → 新 `.app` 移入原位 → 删除
//! `.app.old`，中途失败回滚改名。重启经 detached 辅助进程拉起新
//! bundle 后当前进程退出。
//!
//! bundle 位于只读卷（直接从 DMG 运行）或目录无写权限时不尝试替换，
//! 带指引失败；当前进程不在 `.app` 内（开发期 `cargo run`）同样带指引
//! 失败——自更新只服务正式安装的 bundle。

use std::path::{Path, PathBuf};
use std::process::Command;

use gloss_core::log::{debug, thread, warn};

/// 替换或重启失败的形态：错误卡按变体给针对性指引。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    /// 当前进程不在 `.app` 内（开发期裸二进制）：无 bundle 可替换。
    NotABundle,
    /// bundle 所在卷只读或目录无写权限：替换无从下手。
    Unwritable {
        /// bundle 所在目录（指引里点名它）。
        dir: PathBuf,
    },
    /// 解压失败（ditto 非零退出或未产出内容）。
    Unzip(String),
    /// 解压产物里找不到结构完整的 `.app`（`Contents/MacOS` 缺失）。
    InvalidStructure,
    /// 改名/移入失败；已尽力回滚，错误串带两步的现场。
    Replace(String),
    /// 重启接力失败（`open` 不可用或报错）：新 bundle 已就位，指引手动重启。
    Relaunch(String),
}

/// 替换当前 bundle 并经 `open -n` 拉起新 bundle：成功返回后调用方进程
/// 退出（模块任务负责）。
pub fn install(zip: &Path) -> Result<(), InstallError> {
    let bundle = current_bundle().ok_or(InstallError::NotABundle)?;
    let dir = bundle
        .parent()
        .ok_or_else(|| InstallError::Unwritable {
            dir: PathBuf::from("/"),
        })?
        .to_path_buf();
    if !dir_writable(&dir) {
        return Err(InstallError::Unwritable { dir });
    }
    replace(zip, &dir, &bundle)?;
    relaunch(&bundle)
}

/// 当前进程所属的 bundle：从可执行路径向上找第一个 `.app` 目录。
/// 开发期裸二进制不在任何 `.app` 内，返回 `None`。
pub fn current_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .find(|dir| dir.extension() == Some("app".as_ref()))
        .map(Path::to_path_buf)
}

/// 替换的纯机械部分（重启除外），可全量驱动：`bundle` 必须位于
/// `dir` 下且 `dir` 可写。
fn replace(zip: &Path, dir: &Path, bundle: &Path) -> Result<(), InstallError> {
    // 上一次替换中断留下的旧 bundle：本轮成功替换会覆盖它，先清场。
    let old = dir.join(format!(
        "{}.old",
        bundle.file_name().unwrap_or_default().to_string_lossy()
    ));
    if let Err(err) = std::fs::remove_dir_all(&old) {
        debug!(
            thread = thread::TOKIO,
            error = %err,
            "update: stale old bundle cleanup failed, replacing anyway"
        );
    }

    let unpack = dir.join(format!(".gloss-update-{}", std::process::id()));
    if let Err(err) = std::fs::remove_dir_all(&unpack) {
        debug!(thread = thread::TOKIO, error = %err, "update: stale unpack dir cleanup failed");
    }
    std::fs::create_dir_all(&unpack)
        .map_err(|err| InstallError::Unzip(format!("create unpack dir: {err}")))?;

    if let Err(err) = ditto_extract(zip, &unpack) {
        if let Err(cleanup) = std::fs::remove_dir_all(&unpack) {
            debug!(thread = thread::TOKIO, error = %cleanup, "update: unpack dir cleanup failed");
        }
        return Err(err);
    }
    let new_app = match extracted_app(&unpack) {
        Some(found) => found,
        None => {
            if let Err(cleanup) = std::fs::remove_dir_all(&unpack) {
                debug!(thread = thread::TOKIO, error = %cleanup, "update: unpack dir cleanup failed");
            }
            return Err(InstallError::InvalidStructure);
        }
    };

    // 中途失败的回滚只护「改名对」：旧 bundle 改名成功而新 bundle 移入
    // 失败时，把旧 bundle 改回原位，现场恢复到替换前。
    if let Err(err) = std::fs::rename(bundle, &old) {
        if let Err(cleanup) = std::fs::remove_dir_all(&unpack) {
            debug!(thread = thread::TOKIO, error = %cleanup, "update: unpack dir cleanup failed");
        }
        return Err(InstallError::Replace(format!("stash old bundle: {err}")));
    }
    if let Err(err) = std::fs::rename(&new_app, bundle) {
        match std::fs::rename(&old, bundle) {
            Ok(()) => warn!(
                thread = thread::TOKIO,
                error = %err,
                "update: install rolled back, old bundle restored"
            ),
            Err(rollback) => warn!(
                thread = thread::TOKIO,
                error = %err,
                rollback = %rollback,
                "update: install failed and rollback also failed, old bundle kept as .app.old"
            ),
        }
        if let Err(cleanup) = std::fs::remove_dir_all(&unpack) {
            debug!(thread = thread::TOKIO, error = %cleanup, "update: unpack dir cleanup failed");
        }
        return Err(InstallError::Replace(format!("move new bundle: {err}")));
    }

    // 旧 bundle 清理与解压现场清理都属收尾：失败不致命（旧 bundle 留
    // 在 .app.old 不影响新 bundle 运行，下次替换先清场），记日志即可。
    if let Err(err) = std::fs::remove_dir_all(&old) {
        warn!(
            thread = thread::TOKIO,
            path = %old.display(),
            error = %err,
            "update: old bundle cleanup failed, kept as .app.old"
        );
    }
    if let Err(err) = std::fs::remove_dir_all(&unpack) {
        debug!(thread = thread::TOKIO, error = %err, "update: unpack dir cleanup failed");
    }
    Ok(())
}

/// 经 detached 辅助进程拉起新 bundle：`open -n` 由 LaunchServices 接管，
/// 本进程随后退出。接力失败带指引返回（新 bundle 已就位，手动重启即可）。
fn relaunch(bundle: &Path) -> Result<(), InstallError> {
    let status = Command::new("/usr/bin/open")
        .arg("-n")
        .arg(bundle)
        .status()
        .map_err(|err| InstallError::Relaunch(format!("spawn open: {err}")))?;
    if !status.success() {
        return Err(InstallError::Relaunch(format!("open exited with {status}")));
    }
    Ok(())
}

/// `ditto -x -k`：macOS 自带的 zip 解压（打包链路 bundle-dmg.sh 同源工具）。
fn ditto_extract(zip: &Path, unpack: &Path) -> Result<(), InstallError> {
    let status = Command::new("/usr/bin/ditto")
        .arg("-x")
        .arg("-k")
        .arg(zip)
        .arg(unpack)
        .status()
        .map_err(|err| InstallError::Unzip(format!("spawn ditto: {err}")))?;
    if !status.success() {
        return Err(InstallError::Unzip(format!("ditto exited with {status}")));
    }
    Ok(())
}

/// 解压产物里结构完整的 `.app`：zip 由打包链路产出（单 bundle），取第一
/// 个带 `Contents/MacOS` 的；没有即结构不符。
fn extracted_app(unpack: &Path) -> Option<PathBuf> {
    std::fs::read_dir(unpack).ok()?.find_map(|entry| {
        let Ok(entry) = entry else { return None };
        let path = entry.path();
        if path.extension() == Some("app".as_ref()) && path.join("Contents").join("MacOS").is_dir()
        {
            return Some(path);
        }
        None
    })
}

/// 目录可写性探测：真实建删一个探针文件（只读卷上 create 直接失败）。
fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(".gloss-update-probe");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            if let Err(err) = std::fs::remove_file(&probe) {
                debug!(thread = thread::TOKIO, error = %err, "update: writability probe cleanup failed");
            }
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::Command;

    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gloss-install-test-{tag}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("work dir");
        dir
    }

    fn fixture_zip(tag: &str, marker: &str) -> PathBuf {
        let dir = temp_dir(tag);
        let app = dir.join("Gloss.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).expect("app skeleton");
        std::fs::write(app.join("Contents/MacOS/Gloss"), b"#!/bin/sh\n").expect("binary");
        std::fs::write(app.join("Contents/marker"), marker).expect("marker");
        let zip = dir.join("gloss-fixture.zip");
        let status = Command::new("/usr/bin/ditto")
            .arg("-c")
            .arg("-k")
            .arg("--keepParent")
            .arg(&app)
            .arg(&zip)
            .status()
            .expect("ditto spawn");
        assert!(status.success(), "ditto fixture build failed");
        zip
    }

    #[test]
    fn replace_swaps_bundle_and_leaves_no_litter() {
        let dir = temp_dir("swap");
        let zip = fixture_zip("swap-src", "new-bundle-marker");
        let bundle = dir.join("Gloss.app");
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).expect("old bundle");
        std::fs::write(bundle.join("Contents/marker"), "old-bundle-marker").expect("old marker");

        replace(&zip, &dir, &bundle).expect("replace must succeed");
        assert_eq!(
            std::fs::read_to_string(bundle.join("Contents/marker")).expect("new marker"),
            "new-bundle-marker",
            "the new bundle must sit at the original path"
        );
        assert!(
            !dir.join("Gloss.app.old").exists(),
            "old bundle must be removed"
        );
        assert!(
            std::fs::read_dir(&dir).expect("dir").all(|entry| !entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".gloss-update")),
            "unpack dir must be cleaned up"
        );
    }

    #[test]
    fn replace_over_a_stale_app_old_still_succeeds() {
        let dir = temp_dir("stale-old");
        let zip = fixture_zip("stale-src", "new-marker");
        let bundle = dir.join("Gloss.app");
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).expect("old bundle");
        std::fs::create_dir_all(dir.join("Gloss.app.old")).expect("stale old");

        replace(&zip, &dir, &bundle).expect("stale .app.old must not block a replacement");
        assert!(!dir.join("Gloss.app.old").exists());
    }

    #[test]
    fn zip_without_a_structured_app_leaves_the_bundle_intact() {
        let dir = temp_dir("bad-structure");
        let bundle = dir.join("Gloss.app");
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).expect("old bundle");
        std::fs::write(bundle.join("Contents/marker"), "old").expect("old marker");

        let junk = dir.join("junk.zip");
        std::fs::write(&junk, b"not a zip").expect("junk zip");
        let error = replace(&junk, &dir, &bundle).expect_err("junk zip must fail");
        assert!(matches!(error, InstallError::Unzip(_)), "got {error:?}");
        assert_eq!(
            std::fs::read_to_string(bundle.join("Contents/marker")).expect("old marker"),
            "old",
            "a failed extract must not touch the installed bundle"
        );
    }

    #[test]
    fn unwritable_dir_is_reported_before_anything_is_touched() {
        let dir = temp_dir("readonly");
        let bundle = dir.join("Gloss.app");
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).expect("old bundle");
        std::fs::write(dir.join("hurdle"), b"").expect("hurdle");
        let mut perms = std::fs::metadata(&dir).expect("meta").permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o555);
        std::fs::set_permissions(&dir, perms).expect("chmod");

        let error = install_probe(&bundle);
        let mut perms = std::fs::metadata(&dir).expect("meta").permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        std::fs::set_permissions(&dir, perms).expect("chmod back");

        assert!(
            matches!(error, Some(InstallError::Unwritable { .. })),
            "expected Unwritable, got {error:?}"
        );
        assert!(
            bundle.join("Contents/MacOS").is_dir(),
            "bundle must be untouched"
        );
    }

    fn install_probe(bundle: &Path) -> Option<InstallError> {
        let dir = bundle.parent().expect("bundle parent").to_path_buf();
        if !dir_writable(&dir) {
            return Some(InstallError::Unwritable { dir });
        }
        None
    }

    #[test]
    fn zip_with_an_app_missing_macos_dir_is_invalid_structure() {
        let dir = temp_dir("no-macos");
        let bundle = dir.join("Gloss.app");
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).expect("old bundle");

        let app = dir.join("hollow.app");
        std::fs::create_dir_all(app.join("Contents/Resources")).expect("hollow skeleton");
        let zip = dir.join("hollow.zip");
        let status = Command::new("/usr/bin/ditto")
            .arg("-c")
            .arg("-k")
            .arg("--keepParent")
            .arg(&app)
            .arg(&zip)
            .status()
            .expect("ditto spawn");
        assert!(status.success(), "ditto fixture build failed");

        let error = replace(&zip, &dir, &bundle).expect_err("hollow .app must be rejected");
        assert_eq!(error, InstallError::InvalidStructure);
        assert!(bundle.join("Contents/MacOS").is_dir());
    }
}
