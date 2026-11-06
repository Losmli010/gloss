//! 整包原位替换（系统（Windows））：压缩包结构校验、重命名交换与重启拉起。
//!
//! 替换规格（仅原位替换当前可执行文件，不做安装器迁移）：解压压缩包到安装目录
//! 同卷的临时目录并校验载荷结构——与当前可执行文件同名的文件所在目录为载荷根，
//! 旁置文件随行；旧文件改名腾位、新文件就位、失败全量回滚（编排走共享核心）。
//! 运行中的可执行文件不可原位覆盖（文件锁），改名不受锁限制，腾位因此可行；
//! 上一次替换遗留的旧件在本次腾位前清场。重启经直接拉起新可执行文件完成，当前
//! 进程随后退出——可执行文件能在改名后的原路径上存活，正是腾位策略的前提。

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
    let exe = std::env::current_exe().map_err(|_| InstallError::NotABundle)?;
    let dir = exe
        .parent()
        .ok_or_else(|| InstallError::Unwritable {
            dir: PathBuf::from("/"),
        })?
        .to_path_buf();
    if !install_shared::dir_writable(&dir) {
        return Err(InstallError::Unwritable { dir });
    }
    replace(zip, &dir, &exe)?;
    relaunch(&exe)
}

fn replace(zip: &Path, dir: &Path, exe: &Path) -> Result<(), InstallError> {
    let spec = PayloadSpec {
        exe_name: exe.file_name().map(|name| name.to_os_string()),
    };
    let unpack = install_shared::prepare_unpack_dir(dir)
        .map_err(|err| InstallError::Unzip(format!("create unpack dir: {err}")))?;
    let payload = match extract_payload(zip, &unpack, &spec) {
        Ok(payload) => payload,
        Err(err) => {
            install_shared::remove_unpack_dir(&unpack);
            return Err(err);
        }
    };
    let entries = install_shared::swap_entries(&spec, &payload, dir, exe);
    if let Err(failure) = install_shared::swap_in_place(&entries) {
        install_shared::remove_unpack_dir(&unpack);
        return Err(replace_failure(failure));
    }
    install_shared::finish_replace(&entries, &unpack);
    Ok(())
}

fn extract_payload(zip: &Path, unpack: &Path, spec: &PayloadSpec) -> Result<PathBuf, InstallError> {
    extract_zip(zip, unpack)?;
    install_shared::payload_root(unpack, spec).ok_or(InstallError::InvalidStructure)
}

fn extract_zip(zip: &Path, unpack: &Path) -> Result<(), InstallError> {
    let file =
        std::fs::File::open(zip).map_err(|err| InstallError::Unzip(format!("open zip: {err}")))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|err| InstallError::Unzip(format!("read zip: {err}")))?;
    archive
        .extract(unpack)
        .map_err(|err| InstallError::Unzip(format!("extract zip: {err}")))
}

fn replace_failure(failure: SwapFailure) -> InstallError {
    match failure {
        SwapFailure::Stash { source } => InstallError::Replace(format!("stash old file: {source}")),
        SwapFailure::Move { source } => InstallError::Replace(format!("move new file: {source}")),
    }
}

fn relaunch(exe: &Path) -> Result<(), InstallError> {
    Command::new(exe)
        .spawn()
        .map(|_| ())
        .map_err(|err| InstallError::Relaunch(format!("spawn new exe: {err}")))
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::PathBuf;

    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gloss-install-win-test-{tag}-{}",
            std::process::id()
        ));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("work dir");
        dir
    }

    fn fixture_zip(tag: &str, entries: &[(&str, &[u8])]) -> PathBuf {
        let dir = temp_dir(tag);
        let zip_path = dir.join("gloss-fixture.zip");
        let file = std::fs::File::create(&zip_path).expect("zip file");
        let mut writer = zip::ZipWriter::new(file);
        for (name, body) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .expect("zip entry");
            writer.write_all(body).expect("zip body");
        }
        writer.finish().expect("zip finish");
        zip_path
    }

    #[test]
    fn windows_replace_swaps_the_executable_and_side_files() {
        let zip = fixture_zip(
            "swap-src",
            &[
                ("gloss.exe", b"new-exe".as_slice()),
                ("gloss.dll", b"new-dll"),
            ],
        );
        let dir = temp_dir("swap");
        let exe = dir.join("gloss.exe");
        std::fs::write(&exe, b"old-exe").expect("old exe");

        replace(&zip, &dir, &exe).expect("replace must succeed");
        assert_eq!(
            std::fs::read(&exe).expect("new exe"),
            b"new-exe",
            "the new exe must sit at the original path"
        );
        assert_eq!(
            std::fs::read(dir.join("gloss.dll")).expect("side dll"),
            b"new-dll"
        );
        assert!(!dir.join("gloss.exe.old").exists());
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
    fn windows_replace_rejects_a_zip_without_the_executable() {
        let zip = fixture_zip("reject-src", &[("readme.txt", b"junk".as_slice())]);
        let dir = temp_dir("reject");
        let exe = dir.join("gloss.exe");
        std::fs::write(&exe, b"old-exe").expect("old exe");

        let error = replace(&zip, &dir, &exe).expect_err("zip without the exe must be rejected");
        assert_eq!(error, InstallError::InvalidStructure);
        assert_eq!(
            std::fs::read(&exe).expect("old exe"),
            b"old-exe",
            "a failed structure check must not touch the installed exe"
        );
    }
}
