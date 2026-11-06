//! 校验通过的整包 → 原位替换当前应用包并重启。
//!
//! 替换规格（仅原位替换当前应用包，不做安装器迁移）：解压压缩包到目标目录
//! 同卷的临时目录并检查包内应用结构 → 换装（腾位、就位与回滚走共享核心，
//! 平铺载荷的系统（Windows）实现共用同一套编排）→ 删除回滚副本。重启经
//! 脱离父进程的辅助进程拉起新应用包后当前进程退出。
//!
//! 应用包位于只读卷——直接从磁盘映像（DMG）运行——或目录无写权限时不尝试替换，
//! 带指引失败；当前进程不在应用包内（开发期直接运行）同样带指引
//! 失败——自更新只服务正式安装的应用包。

use std::path::{Path, PathBuf};
use std::process::Command;

use super::install_shared::{self, PayloadSpec, SwapFailure};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    NotABundle,
    Unwritable { dir: PathBuf },
    Unzip(String),
    InvalidStructure,
    Replace(String),
    Relaunch(String),
}

pub fn install(zip: &Path) -> Result<(), InstallError> {
    let bundle = current_bundle().ok_or(InstallError::NotABundle)?;
    let dir = bundle
        .parent()
        .ok_or_else(|| InstallError::Unwritable {
            dir: PathBuf::from("/"),
        })?
        .to_path_buf();
    if !install_shared::dir_writable(&dir) {
        return Err(InstallError::Unwritable { dir });
    }
    replace(zip, &dir, &bundle)?;
    relaunch(&bundle)
}

pub fn current_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .find(|dir| dir.extension() == Some("app".as_ref()))
        .map(Path::to_path_buf)
}

fn replace(zip: &Path, dir: &Path, bundle: &Path) -> Result<(), InstallError> {
    let unpack = install_shared::prepare_unpack_dir(dir)
        .map_err(|err| InstallError::Unzip(format!("create unpack dir: {err}")))?;
    let payload = match extract_app(zip, &unpack) {
        Ok(payload) => payload,
        Err(err) => {
            install_shared::remove_unpack_dir(&unpack);
            return Err(err);
        }
    };
    let entries =
        install_shared::swap_entries(&PayloadSpec { exe_name: None }, &payload, dir, bundle);
    if let Err(failure) = install_shared::swap_in_place(&entries) {
        install_shared::remove_unpack_dir(&unpack);
        return Err(replace_failure(failure));
    }
    install_shared::finish_replace(&entries, &unpack);
    Ok(())
}

fn replace_failure(failure: SwapFailure) -> InstallError {
    match failure {
        SwapFailure::Stash { source } => {
            InstallError::Replace(format!("stash old bundle: {source}"))
        }
        SwapFailure::Move { source } => InstallError::Replace(format!("move new bundle: {source}")),
    }
}

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

fn extract_app(zip: &Path, unpack: &Path) -> Result<PathBuf, InstallError> {
    ditto_extract(zip, unpack)?;
    install_shared::payload_root(unpack, &PayloadSpec { exe_name: None })
        .ok_or(InstallError::InvalidStructure)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::Command;

    use super::install_shared::dir_writable;
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
