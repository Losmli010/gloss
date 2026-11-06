//! 原位替换共享核心：载荷定位、换装条目推导与换装编排，两平台实现共用同一套机制。
//!
//! 载荷形态由载荷规格（PayloadSpec）承载：带可执行文件锚点即平铺载荷（系统（Windows））——
//! 解包现场里与锚点同名的文件所在目录为载荷根，其下每个直接子项一条目；不带锚点即目录
//! 载荷（系统（macOS））——解包现场顶层结构完整的应用包目录为单条目。换装编排逐条目腾位
//! （旧载荷改名加旧件后缀）与就位（新载荷移入原位），任一步失败全量回滚：已就位的新载荷
//! 撤下、已腾位的旧载荷复位，回滚本身失败带痕迹保留。新载荷缺席的旧旁置文件不回收，文件
//! 集稳定由发布打包保证。平台差异只住各实现文件——提取命令、拉起方式与锚点取值；分派在
//! 更新模块的声明处，本文件零平台分支。

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};

use gloss_core::log::{debug, thread, warn};

pub(crate) struct PayloadSpec {
    pub exe_name: Option<OsString>,
}

pub(crate) struct SwapEntry {
    pub src: PathBuf,
    pub dst: PathBuf,
    pub stash: PathBuf,
}

#[derive(Debug)]
pub(crate) enum SwapFailure {
    Stash { source: io::Error },
    Move { source: io::Error },
}

pub(crate) fn payload_root(unpack: &Path, spec: &PayloadSpec) -> Option<PathBuf> {
    match &spec.exe_name {
        Some(exe_name) => exe_root(unpack, exe_name),
        None => bundle_root(unpack),
    }
}

pub(crate) fn swap_entries(
    spec: &PayloadSpec,
    payload: &Path,
    install_dir: &Path,
    current: &Path,
) -> Vec<SwapEntry> {
    match &spec.exe_name {
        Some(_) => flat_entries(payload, install_dir),
        None => vec![bundle_entry(payload, current)],
    }
}

pub(crate) fn swap_in_place(entries: &[SwapEntry]) -> Result<(), SwapFailure> {
    let mut stashed: Vec<&SwapEntry> = Vec::new();
    let mut placed: Vec<&SwapEntry> = Vec::new();
    for entry in entries {
        clear_stale_stash(&entry.stash);
        if entry.dst.exists() {
            if let Err(source) = std::fs::rename(&entry.dst, &entry.stash) {
                rollback(&source, &stashed, &placed);
                return Err(SwapFailure::Stash { source });
            }
            stashed.push(entry);
        }
        if let Err(source) = std::fs::rename(&entry.src, &entry.dst) {
            rollback(&source, &stashed, &placed);
            return Err(SwapFailure::Move { source });
        }
        placed.push(entry);
    }
    Ok(())
}

pub(crate) fn prepare_unpack_dir(dir: &Path) -> Result<PathBuf, io::Error> {
    let unpack = dir.join(format!(".gloss-update-{}", std::process::id()));
    if let Err(err) = remove_path(&unpack) {
        debug!(thread = thread::TOKIO, error = %err, "update: stale unpack dir cleanup failed");
    }
    std::fs::create_dir_all(&unpack)?;
    Ok(unpack)
}

pub(crate) fn finish_replace(entries: &[SwapEntry], unpack: &Path) {
    for entry in entries {
        if let Err(err) = remove_path(&entry.stash) {
            warn!(
                thread = thread::TOKIO,
                path = %entry.stash.display(),
                error = %err,
                "update: old payload cleanup failed, kept as .old"
            );
        }
    }
    remove_unpack_dir(unpack);
}

pub(crate) fn remove_unpack_dir(unpack: &Path) {
    if let Err(err) = remove_path(unpack) {
        debug!(thread = thread::TOKIO, error = %err, "update: unpack dir cleanup failed");
    }
}

pub(crate) fn dir_writable(dir: &Path) -> bool {
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

fn bundle_root(unpack: &Path) -> Option<PathBuf> {
    std::fs::read_dir(unpack).ok()?.find_map(|entry| {
        let Ok(entry) = entry else { return None };
        let path = entry.path();
        (path.extension() == Some("app".as_ref()) && path.join("Contents").join("MacOS").is_dir())
            .then_some(path)
    })
}

fn exe_root(unpack: &Path, exe_name: &OsStr) -> Option<PathBuf> {
    let mut pending = vec![unpack.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file() && entry.file_name().as_os_str() == exe_name {
                return Some(dir);
            }
        }
    }
    None
}

fn bundle_entry(payload: &Path, current: &Path) -> SwapEntry {
    SwapEntry {
        src: payload.to_path_buf(),
        dst: current.to_path_buf(),
        stash: stash_of(current),
    }
}

fn flat_entries(payload: &Path, install_dir: &Path) -> Vec<SwapEntry> {
    let Ok(children) = std::fs::read_dir(payload) else {
        return Vec::new();
    };
    children
        .flatten()
        .map(|entry| {
            let dst = install_dir.join(entry.file_name());
            SwapEntry {
                src: entry.path(),
                stash: stash_of(&dst),
                dst,
            }
        })
        .collect()
}

fn stash_of(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    path.with_file_name(format!("{name}.old"))
}

fn clear_stale_stash(stash: &Path) {
    if let Err(err) = remove_path(stash) {
        debug!(
            thread = thread::TOKIO,
            error = %err,
            "update: stale old payload cleanup failed, replacing anyway"
        );
    }
}

fn rollback(trigger: &io::Error, stashed: &[&SwapEntry], placed: &[&SwapEntry]) {
    for entry in placed {
        if let Err(err) = remove_path(&entry.dst) {
            debug!(thread = thread::TOKIO, error = %err, "update: rollback placed payload cleanup failed");
        }
    }
    if stashed.is_empty() && placed.is_empty() {
        return;
    }
    let mut rollback_failure = None;
    for entry in stashed {
        if let Err(err) = std::fs::rename(&entry.stash, &entry.dst) {
            rollback_failure.get_or_insert(err);
        }
    }
    match rollback_failure {
        None => warn!(
            thread = thread::TOKIO,
            error = %trigger,
            "update: install rolled back, old payload restored"
        ),
        Some(rollback) => warn!(
            thread = thread::TOKIO,
            error = %trigger,
            rollback = %rollback,
            "update: install failed and rollback also failed, old payload kept as .old"
        ),
    }
}

fn remove_path(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => std::fs::remove_dir_all(path),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gloss-install-shared-test-{tag}-{}",
            std::process::id()
        ));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("work dir");
        dir
    }

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).expect("write fixture file");
    }

    fn flat_spec(exe: &str) -> PayloadSpec {
        PayloadSpec {
            exe_name: Some(exe.into()),
        }
    }

    #[test]
    fn bundle_payload_requires_a_structured_app_dir() {
        let dir = temp_dir("bundle-ok");
        let unpack = dir.join("unpack");
        let app = unpack.join("Gloss.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).expect("structured app");
        assert_eq!(
            payload_root(&unpack, &PayloadSpec { exe_name: None }),
            Some(app)
        );

        let hollow = dir.join("hollow");
        let hollow_app = hollow.join("Gloss.app");
        std::fs::create_dir_all(hollow_app.join("Contents/Resources")).expect("hollow app");
        assert_eq!(payload_root(&hollow, &PayloadSpec { exe_name: None }), None);
    }

    #[test]
    fn flat_payload_requires_the_current_executable_name() {
        let dir = temp_dir("flat-ok");
        let unpack = dir.join("unpack");
        std::fs::create_dir_all(&unpack).expect("unpack");
        write(&unpack, "gloss.exe", "exe");
        write(&unpack, "gloss.dll", "dll");
        assert_eq!(payload_root(&unpack, &flat_spec("gloss.exe")), Some(unpack));

        let bare = dir.join("bare");
        std::fs::create_dir_all(&bare).expect("bare");
        write(&bare, "gloss.dll", "dll");
        assert_eq!(payload_root(&bare, &flat_spec("gloss.exe")), None);
    }

    #[test]
    fn flat_payload_found_inside_a_nested_folder() {
        let dir = temp_dir("flat-nested");
        let payload = dir.join("unpack/gloss-portable");
        std::fs::create_dir_all(&payload).expect("payload folder");
        write(&payload, "gloss.exe", "exe");
        assert_eq!(
            payload_root(&dir.join("unpack"), &flat_spec("gloss.exe")),
            Some(payload)
        );
    }

    #[test]
    fn bundle_entries_swap_the_whole_app_dir() {
        let dir = temp_dir("bundle-entries");
        let payload = dir.join("unpack/Gloss.app");
        std::fs::create_dir_all(&payload).expect("payload");
        let install_dir = dir.join("install");
        std::fs::create_dir_all(&install_dir).expect("install dir");
        let current = install_dir.join("Gloss.app");

        let entries = swap_entries(
            &PayloadSpec { exe_name: None },
            &payload,
            &install_dir,
            &current,
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].src, payload);
        assert_eq!(entries[0].dst, current);
        assert_eq!(entries[0].stash, install_dir.join("Gloss.app.old"));
    }

    #[test]
    fn flat_entries_cover_every_file_beside_the_executable() {
        let dir = temp_dir("flat-entries");
        let payload = dir.join("payload");
        std::fs::create_dir_all(&payload).expect("payload");
        write(&payload, "gloss.exe", "exe");
        write(&payload, "gloss.dll", "dll");
        write(&payload, "NOTICE.txt", "notice");
        let install_dir = dir.join("install");
        std::fs::create_dir_all(&install_dir).expect("install dir");

        let entries = swap_entries(
            &flat_spec("gloss.exe"),
            &payload,
            &install_dir,
            &install_dir,
        );
        assert_eq!(entries.len(), 3, "every payload file gets one entry");
        let dst_names: Vec<String> = entries
            .iter()
            .map(|entry| {
                entry
                    .dst
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert!(dst_names.contains(&"gloss.exe".to_string()));
        assert!(dst_names.contains(&"gloss.dll".to_string()));
        assert!(dst_names.contains(&"NOTICE.txt".to_string()));
        for entry in &entries {
            let stash_name = entry
                .stash
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let dst_name = entry
                .dst
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            assert_eq!(stash_name, format!("{dst_name}.old"));
        }
    }

    #[test]
    fn swap_moves_entries_into_place_without_litter() {
        let dir = temp_dir("swap-ok");
        let payload = dir.join("payload");
        std::fs::create_dir_all(&payload).expect("payload");
        write(&payload, "gloss.exe", "new-exe");
        write(&payload, "gloss.dll", "new-dll");
        let install_dir = dir.join("install");
        std::fs::create_dir_all(&install_dir).expect("install dir");
        write(&install_dir, "gloss.exe", "old-exe");

        let entries = swap_entries(
            &flat_spec("gloss.exe"),
            &payload,
            &install_dir,
            &install_dir,
        );
        swap_in_place(&entries).expect("swap must succeed");
        finish_replace(&entries, &dir.join(".gloss-update-0"));

        assert_eq!(
            std::fs::read_to_string(install_dir.join("gloss.exe")).expect("exe"),
            "new-exe"
        );
        assert_eq!(
            std::fs::read_to_string(install_dir.join("gloss.dll")).expect("dll"),
            "new-dll"
        );
        assert!(!install_dir.join("gloss.exe.old").exists());
        assert!(!install_dir.join("gloss.dll.old").exists());
    }

    #[test]
    fn swap_rolls_back_everything_when_a_move_fails() {
        let dir = temp_dir("swap-rollback");
        let payload = dir.join("payload");
        std::fs::create_dir_all(&payload).expect("payload");
        write(&payload, "gloss.exe", "new-exe");
        let install_dir = dir.join("install");
        std::fs::create_dir_all(&install_dir).expect("install dir");
        write(&install_dir, "gloss.exe", "old-exe");
        write(&install_dir, "gloss.dll", "old-dll");

        let entries = vec![
            SwapEntry {
                src: payload.join("gloss.exe"),
                dst: install_dir.join("gloss.exe"),
                stash: install_dir.join("gloss.exe.old"),
            },
            SwapEntry {
                src: payload.join("missing.dll"),
                dst: install_dir.join("gloss.dll"),
                stash: install_dir.join("gloss.dll.old"),
            },
        ];
        let error = swap_in_place(&entries).expect_err("missing source must fail the swap");
        assert!(matches!(error, SwapFailure::Move { .. }), "got {error:?}");

        assert_eq!(
            std::fs::read_to_string(install_dir.join("gloss.exe")).expect("exe"),
            "old-exe",
            "placed entry must roll back to the old content"
        );
        assert_eq!(
            std::fs::read_to_string(install_dir.join("gloss.dll")).expect("dll"),
            "old-dll",
            "stashed entry must be restored"
        );
        assert!(!install_dir.join("gloss.exe.old").exists());
        assert!(!install_dir.join("gloss.dll.old").exists());
    }

    #[test]
    fn swap_reports_a_stash_failure_with_the_dst_untouched() {
        let dir = temp_dir("swap-stash-fail");
        let payload = dir.join("payload");
        std::fs::create_dir_all(&payload).expect("payload");
        write(&payload, "gloss.exe", "new-exe");
        let install_dir = dir.join("install");
        std::fs::create_dir_all(&install_dir).expect("install dir");
        write(&install_dir, "gloss.exe", "old-exe");

        let entries = vec![SwapEntry {
            src: payload.join("gloss.exe"),
            dst: install_dir.join("gloss.exe"),
            stash: dir.join("missing-parent").join("gloss.exe.old"),
        }];
        let error = swap_in_place(&entries).expect_err("unreachable stash must fail");
        assert!(matches!(error, SwapFailure::Stash { .. }), "got {error:?}");
        assert_eq!(
            std::fs::read_to_string(install_dir.join("gloss.exe")).expect("exe"),
            "old-exe"
        );
    }

    #[test]
    fn swap_clears_a_stale_stash_before_swapping() {
        let dir = temp_dir("swap-stale");
        let payload = dir.join("payload");
        std::fs::create_dir_all(&payload).expect("payload");
        write(&payload, "gloss.exe", "new-exe");
        let install_dir = dir.join("install");
        std::fs::create_dir_all(&install_dir).expect("install dir");
        write(&install_dir, "gloss.exe", "old-exe");
        write(&install_dir, "gloss.exe.old", "leftover-from-last-update");

        let entries = swap_entries(
            &flat_spec("gloss.exe"),
            &payload,
            &install_dir,
            &install_dir,
        );
        swap_in_place(&entries).expect("stale stash must not block a replacement");
        finish_replace(&entries, &dir.join(".gloss-update-0"));

        assert_eq!(
            std::fs::read_to_string(install_dir.join("gloss.exe")).expect("exe"),
            "new-exe"
        );
        assert!(!install_dir.join("gloss.exe.old").exists());
    }
}
