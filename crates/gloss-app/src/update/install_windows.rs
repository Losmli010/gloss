//! 整包原位替换（系统（Windows）实现）：压缩包结构校验、重命名交换与重启拉起。
//!
//! 替换规格（仅原位替换当前可执行文件，不做安装器迁移）：提取走进程内压缩包（zip）
//! 库，替换编排、可写性门禁与错误契约住共享核心（install_shared）。运行中的可执行
//! 文件不可原位覆盖（文件锁），改名不受锁限制，腾位因此可行；上一次替换遗留的旧件
//! 在本次腾位前清场。重启经直接拉起新可执行文件完成，当前进程随后退出——可执行文件
//! 能在改名后的原路径上存活，正是腾位策略的前提。平台承载：锚点为当前可执行文件，
//! 载荷锚点即其文件名。

use std::path::Path;
use std::process::Command;

pub use super::install_shared::InstallError;
use super::install_shared::{PayloadSpec, replace};

pub fn install(zip: &Path) -> Result<(), InstallError> {
    let exe = std::env::current_exe().map_err(|_| InstallError::NotABundle)?;
    let spec = PayloadSpec {
        exe_name: exe.file_name().map(|name| name.to_os_string()),
    };
    replace(zip, &exe, &spec, extract_zip)?;
    relaunch(&exe)
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

    fn spec_of(exe: &Path) -> PayloadSpec {
        PayloadSpec {
            exe_name: exe.file_name().map(|name| name.to_os_string()),
        }
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

        replace(&zip, &exe, &spec_of(&exe), extract_zip).expect("replace must succeed");
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

        let error = replace(&zip, &exe, &spec_of(&exe), extract_zip)
            .expect_err("zip without the exe must be rejected");
        assert_eq!(error, InstallError::InvalidStructure);
        assert_eq!(
            std::fs::read(&exe).expect("old exe"),
            b"old-exe",
            "a failed structure check must not touch the installed exe"
        );
        assert!(
            std::fs::read_dir(&dir).expect("dir").all(|entry| !entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".gloss-update")),
            "a rejected structure must not leave the unpack dir behind"
        );
    }
}
