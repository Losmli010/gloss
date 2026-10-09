//! 更新子状态机：纯状态转移，不做任何 IO（与 [`crate::machine`] 同构的
//! functional core）。
//!
//! 触发入口（启动静默检查 / 设置页手动检查）、下载与替换都由壳执行——
//! 本模块只决策：用户意图（[`Self::check`] / [`Self::confirm_download`] /
//! [`Self::confirm_restart`] / [`Self::retry`] / [`Self::cancel`]）与任务
//! 回包（[`UpdateOutcome`]）在此收敛成命令与状态，UI 每帧读最新相位渲染。
//!
//! 状态迁移逐行对应分发设计 §4.1 的迁移表；额外说明三条表外语义：
//! - 初始相位 [`UpdatePhase::Idle`] 只存在于首次检查发起之前（启动静默
//!   检查在首帧就绪后补发，稳态不可达）；
//! - [`Self::cancel`] 只对下载中有效，回到待确认下载——清单已知且校验
//!   通过，该事实不因下载取消而消失；在途进度留在 `.partial`（下次全量
//!   下载时清除，仅失败重试路径会续传）；检查中无处可回，不予取消
//!   （重复检查本就会取消在途任务）；
//! - 迟到回包由守卫拒绝（相位不符一律不采纳），已取消任务的回包在模块
//!   任务侧经取消令牌过滤后才进本机，两道防线互为备份。
//!
//! `Failed` 记录失败步骤，重试回到该步骤：清单步 → 重新检查、下载步 →
//! 基于 `.partial` 续传、替换步 → 回到待确认重启替换（用户点重试即再次
//! 确认，安装直接重跑）。

use semver::Version;
use tokio_util::sync::CancellationToken;

use super::manifest::{self, Artifact, UpdateManifest};

/// 子状态机七相位（另加初始相位 `Idle`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdatePhase {
    /// 尚未发起过检查（仅首次静默检查之前存在）。
    #[default]
    Idle,
    /// 清单拉取与校验进行中。
    Checking,
    /// 发现新版，待用户确认下载。
    UpdateAvailable,
    /// 整包下载（含校验）进行中。
    Downloading,
    /// 下载校验通过，待用户确认重启替换。
    UpdateReady,
    /// 用户已确认，替换执行中（终结前最后一个相位）。
    ReadyToRestart,
    /// 已是最新。
    UpToDate,
    /// 失败：记录失败步骤，重试回到该步。
    Failed(FailStep),
}

/// 失败步骤（[`UpdatePhase::Failed`] 的载荷）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailStep {
    /// 清单拉取/校验失败。
    Manifest,
    /// 下载或 sha256/size 校验失败。
    Download,
    /// 替换或重启失败。
    Install,
}

/// watch 频道广播的最新状态快照（UI 每帧读，不引入锁）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpdateState {
    /// 当前相位。
    pub phase: UpdatePhase,
    /// 已锁定的更新目标版本（`UpdateAvailable` 及其后的失败态有值）。
    pub version: Option<String>,
}

/// 检查通过后锁定的更新目标：目标版本与本机架构的产物快照（下载与
/// 替换命令的载荷，从清单复制——清单本体用完即弃）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateTarget {
    /// 清单声明的目标版本。
    pub version: Version,
    /// 本机架构条目。
    pub artifact: Artifact,
}

/// 用户意图之外的任务回包：模块任务执行完毕后交回状态机。
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateOutcome {
    /// 清单拉取且校验通过（体已在 [`manifest::parse`] 走完矩阵）。
    ManifestReady(UpdateManifest),
    /// 清单拉取失败/超时/非 200，或校验矩阵不符（[`ManifestError`] 只进日志）。
    ManifestUnavailable,
    /// 下载完成且 sha256/size 校验通过。
    DownloadVerified,
    /// 下载或校验失败。
    DownloadFailed,
    /// 替换或重启失败（错误指引由壳按步骤措辞，状态机只记步骤）。
    InstallFailed,
}

/// 状态机交给壳执行的动作：壳据此发起网络任务或替换流程。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCommand {
    /// 拉取清单（启动静默检查与手动检查同一命令）。
    CheckManifest {
        /// 本次检查的取消令牌（重复检查由状态机先取消旧令牌）。
        cancel: CancellationToken,
    },
    /// 下载目标产物；`resume` 为真时基于 `.partial` 续传（重试路径），
    /// 为假时先清除 `.partial` 全量重下（首次确认路径）。
    Download {
        /// 检查通过时锁定的更新目标。
        target: UpdateTarget,
        /// 真为基于 `.partial` 续传（重试路径），假为先清除 `.partial` 全量重下。
        resume: bool,
        /// 本次下载的取消令牌（重复检查由状态机先取消旧令牌）。
        cancel: CancellationToken,
    },
    /// 执行替换并重启（调用前已完成两道确认的第二道）。
    Install {
        /// 检查通过时锁定的更新目标。
        target: UpdateTarget,
    },
}

/// 更新子状态机：纯状态 + 决策，无 IO。
#[derive(Debug, Default)]
pub struct UpdateMachine {
    phase: UpdatePhase,
    /// 检查通过后锁定的更新目标；「无更新」「清单失败」后的重试会重新
    /// 检查并覆盖，下载/替换失败的重试依赖它续跑，故随 Failed 保留。
    update: Option<UpdateTarget>,
    /// 在途任务的取消令牌（检查与下载各一任；替换不可取消）。
    cancel: Option<CancellationToken>,
}

impl UpdateMachine {
    /// 初始相位 `Idle`、无在途任务。
    pub fn new() -> Self {
        Self::default()
    }

    /// 当前相位。
    pub fn phase(&self) -> UpdatePhase {
        self.phase
    }

    /// 当前锁定的更新目标（`UpdateAvailable` 及其后的失败态都有值）。
    pub fn update(&self) -> Option<&UpdateTarget> {
        self.update.as_ref()
    }

    /// 在途任务的取消令牌（壳据此观察取消是否生效）。
    pub fn current_cancel(&self) -> Option<&CancellationToken> {
        self.cancel.as_ref()
    }

    /// 当前状态快照（watch 频道广播的载荷）。
    pub fn snapshot(&self) -> UpdateState {
        UpdateState {
            phase: self.phase,
            version: self
                .update
                .as_ref()
                .map(|target| target.version.to_string()),
        }
    }

    /// 发起一次检查（启动静默检查 / 设置页检查共用入口）：`ReadyToRestart`
    /// 之外任意相位可发起，先取消在途任务（下载中断保留 `.partial`），
    /// 落 `Checking` 并交出清单拉取命令。清单步重试同走此处。
    pub fn check(&mut self) -> Option<UpdateCommand> {
        if self.phase == UpdatePhase::ReadyToRestart {
            return None;
        }
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        self.phase = UpdatePhase::Checking;
        Some(UpdateCommand::CheckManifest {
            cancel: self.fresh_token(),
        })
    }

    /// 确认下载（两道确认的第一道）：仅 `UpdateAvailable` 可发起，
    /// 落 `Downloading` 并交出全量下载命令（`.partial` 先清除）。
    pub fn confirm_download(&mut self) -> Option<UpdateCommand> {
        let target = self.update.clone()?;
        if self.phase != UpdatePhase::UpdateAvailable {
            return None;
        }
        self.phase = UpdatePhase::Downloading;
        Some(UpdateCommand::Download {
            target,
            resume: false,
            cancel: self.fresh_token(),
        })
    }

    /// 确认重启替换（两道确认的第二道）：仅 `UpdateReady` 可发起，
    /// 落 `ReadyToRestart` 并交出替换命令；替换不可取消。
    pub fn confirm_restart(&mut self) -> Option<UpdateCommand> {
        let target = self.update.clone()?;
        if self.phase != UpdatePhase::UpdateReady {
            return None;
        }
        self.phase = UpdatePhase::ReadyToRestart;
        Some(UpdateCommand::Install { target })
    }

    /// 重试失败卡：回到记录的失败步骤（见模块注释）。
    pub fn retry(&mut self) -> Option<UpdateCommand> {
        let step = match self.phase {
            UpdatePhase::Failed(step) => step,
            _ => return None,
        };
        match step {
            FailStep::Manifest => self.check(),
            FailStep::Download => {
                let target = self.update.clone()?;
                self.phase = UpdatePhase::Downloading;
                Some(UpdateCommand::Download {
                    target,
                    resume: true,
                    cancel: self.fresh_token(),
                })
            }
            FailStep::Install => {
                let target = self.update.clone()?;
                self.phase = UpdatePhase::ReadyToRestart;
                Some(UpdateCommand::Install { target })
            }
        }
    }

    /// 取消在途下载：中断保留 `.partial`，回 `UpdateAvailable`。
    /// 仅下载中有效；其余相位无事可做，返回 `false`。
    pub fn cancel(&mut self) -> bool {
        if self.phase != UpdatePhase::Downloading {
            return false;
        }
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        self.phase = UpdatePhase::UpdateAvailable;
        true
    }

    /// 采纳清单回包：守卫 `Checking`（迟到回包一律拒绝）。版本高于本地
    /// 才置 `UpdateAvailable` 并锁定目标；相等或更低一律 `UpToDate`。
    pub fn accept_manifest(&mut self, manifest: UpdateManifest, current: &Version) -> bool {
        if self.phase != UpdatePhase::Checking {
            return false;
        }
        self.cancel = None;
        let artifact = manifest.artifact(manifest::current_arch()).cloned();
        if !manifest::is_newer(manifest.version(), current) {
            self.update = None;
            self.phase = UpdatePhase::UpToDate;
            return true;
        }
        let Some(artifact) = artifact else {
            self.update = None;
            self.phase = UpdatePhase::Failed(FailStep::Manifest);
            return true;
        };
        self.update = Some(UpdateTarget {
            version: manifest.version().clone(),
            artifact,
        });
        self.phase = UpdatePhase::UpdateAvailable;
        true
    }

    /// 采纳「清单不可用」：守卫 `Checking`，落清单步失败。
    pub fn accept_manifest_unavailable(&mut self) -> bool {
        if self.phase != UpdatePhase::Checking {
            return false;
        }
        self.cancel = None;
        self.phase = UpdatePhase::Failed(FailStep::Manifest);
        true
    }

    /// 采纳下载完成：守卫 `Downloading`，落 `UpdateReady`。
    pub fn accept_download_verified(&mut self) -> bool {
        if self.phase != UpdatePhase::Downloading {
            return false;
        }
        self.cancel = None;
        self.phase = UpdatePhase::UpdateReady;
        true
    }

    /// 采纳下载失败：守卫 `Downloading`，落下载步失败。
    pub fn accept_download_failed(&mut self) -> bool {
        if self.phase != UpdatePhase::Downloading {
            return false;
        }
        self.cancel = None;
        self.phase = UpdatePhase::Failed(FailStep::Download);
        true
    }

    /// 采纳替换失败：守卫 `ReadyToRestart`，落替换步失败。
    pub fn accept_install_failed(&mut self) -> bool {
        if self.phase != UpdatePhase::ReadyToRestart {
            return false;
        }
        self.phase = UpdatePhase::Failed(FailStep::Install);
        true
    }

    /// 换一枚新取消令牌并记为在途。
    fn fresh_token(&mut self) -> CancellationToken {
        let token = CancellationToken::new();
        self.cancel = Some(token.clone());
        token
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::update::manifest;

    fn manifest_of(version: &str) -> UpdateManifest {
        let text = format!(
            r#"{{"schema":1,"version":"{version}","channels":{{"stable":{{
  "aarch64-apple-darwin":{{"url":"https://p/a.zip","size":1,"sha256":"{sha_a}"}},
  "x86_64-apple-darwin":{{"url":"https://p/x.zip","size":2,"sha256":"{sha_b}"}}}}}}}}"#,
            sha_a = "a".repeat(64),
            sha_b = "b".repeat(64),
        );
        manifest::parse(&text).expect("test manifest must validate")
    }

    fn started(machine: &mut UpdateMachine) -> UpdateCommand {
        machine.check().expect("check must start from idle")
    }

    fn dispatched(command: Option<UpdateCommand>) -> UpdateCommand {
        command.expect("command expected")
    }

    #[test]
    fn starts_from_idle_into_checking_with_a_command() {
        let mut machine = UpdateMachine::new();
        assert_eq!(machine.phase(), UpdatePhase::Idle);

        let command = started(&mut machine);
        assert!(matches!(command, UpdateCommand::CheckManifest { .. }));
        assert_eq!(machine.phase(), UpdatePhase::Checking);
        assert!(machine.current_cancel().is_some());
        assert!(machine.update().is_none());
    }

    #[test]
    fn checking_manifest_newer_lands_in_update_available() {
        let mut machine = UpdateMachine::new();
        started(&mut machine);

        assert!(machine.accept_manifest(manifest_of("99.0.0"), &manifest::current_version()));
        assert_eq!(machine.phase(), UpdatePhase::UpdateAvailable);
        let target = machine.update().expect("target locked");
        assert_eq!(target.version.to_string(), "99.0.0");
        let expected_url = if manifest::current_arch() == "aarch64-apple-darwin" {
            "https://p/a.zip"
        } else {
            "https://p/x.zip"
        };
        assert_eq!(target.artifact.url, expected_url);
        assert!(
            machine.current_cancel().is_none(),
            "fetch finished, token dropped"
        );
    }

    #[test]
    fn checking_manifest_not_newer_lands_in_up_to_date() {
        let mut machine = UpdateMachine::new();
        started(&mut machine);

        assert!(machine.accept_manifest(manifest_of("0.0.1"), &manifest::current_version()));
        assert_eq!(machine.phase(), UpdatePhase::UpToDate);
        assert!(machine.update().is_none(), "stale target must be cleared");
    }

    #[test]
    fn checking_manifest_unavailable_lands_in_failed_manifest_step() {
        let mut machine = UpdateMachine::new();
        started(&mut machine);

        assert!(machine.accept_manifest_unavailable());
        assert_eq!(machine.phase(), UpdatePhase::Failed(FailStep::Manifest));
        assert!(machine.current_cancel().is_none());
    }

    #[test]
    fn recheck_from_any_active_phase_cancels_and_returns_to_checking() {
        let mut machine = UpdateMachine::new();
        started(&mut machine);
        machine.accept_manifest(manifest_of("99.0.0"), &manifest::current_version());
        let UpdateCommand::Download { cancel, .. } = dispatched(machine.confirm_download()) else {
            panic!("download expected")
        };
        let download_token = cancel;

        let command = dispatched(machine.check());
        assert!(matches!(command, UpdateCommand::CheckManifest { .. }));
        assert_eq!(machine.phase(), UpdatePhase::Checking);
        assert!(
            download_token.is_cancelled(),
            "recheck must cancel the in-flight download"
        );
    }

    #[test]
    fn ready_to_restart_refuses_a_new_check() {
        let mut machine = UpdateMachine::new();
        started(&mut machine);
        machine.accept_manifest(manifest_of("99.0.0"), &manifest::current_version());
        dispatched(machine.confirm_download());
        machine.accept_download_verified();
        dispatched(machine.confirm_restart());

        assert!(machine.check().is_none(), "replacement in flight is final");
        assert_eq!(machine.phase(), UpdatePhase::ReadyToRestart);
    }

    #[test]
    fn confirm_download_only_from_update_available() {
        let mut machine = UpdateMachine::new();
        assert!(machine.confirm_download().is_none(), "idle has no target");

        started(&mut machine);
        assert!(
            machine.confirm_download().is_none(),
            "checking has nothing confirmed yet"
        );

        machine.accept_manifest(manifest_of("99.0.0"), &manifest::current_version());
        let UpdateCommand::Download { target, resume, .. } = dispatched(machine.confirm_download())
        else {
            panic!("download expected")
        };
        assert!(!resume, "first confirmation downloads from scratch");
        assert_eq!(target.version.to_string(), "99.0.0");
        assert!(machine.current_cancel().is_some());

        assert!(
            machine.confirm_download().is_none(),
            "double confirm refused"
        );
    }

    #[test]
    fn download_verified_lands_in_update_ready_then_install_confirms() {
        let mut machine = UpdateMachine::new();
        started(&mut machine);
        machine.accept_manifest(manifest_of("99.0.0"), &manifest::current_version());
        dispatched(machine.confirm_download());

        assert!(machine.accept_download_verified());
        assert_eq!(machine.phase(), UpdatePhase::UpdateReady);

        let UpdateCommand::Install { target } = dispatched(machine.confirm_restart()) else {
            panic!("install expected")
        };
        assert_eq!(target.version.to_string(), "99.0.0");
        assert_eq!(machine.phase(), UpdatePhase::ReadyToRestart);
        assert!(
            machine.confirm_restart().is_none(),
            "double confirm refused"
        );
    }

    #[test]
    fn download_failed_lands_in_failed_download_step() {
        let mut machine = UpdateMachine::new();
        started(&mut machine);
        machine.accept_manifest(manifest_of("99.0.0"), &manifest::current_version());
        dispatched(machine.confirm_download());

        assert!(machine.accept_download_failed());
        assert_eq!(machine.phase(), UpdatePhase::Failed(FailStep::Download));
        assert!(
            machine.update().is_some(),
            "the verified target must survive for the retry"
        );
    }

    #[test]
    fn retry_returns_to_the_recorded_step() {
        let mut machine = UpdateMachine::new();
        started(&mut machine);
        machine.accept_manifest_unavailable();
        let command = dispatched(machine.retry());
        assert!(matches!(command, UpdateCommand::CheckManifest { .. }));
        assert_eq!(machine.phase(), UpdatePhase::Checking);

        let mut machine = UpdateMachine::new();
        started(&mut machine);
        machine.accept_manifest(manifest_of("99.0.0"), &manifest::current_version());
        let UpdateCommand::Download { cancel, .. } = dispatched(machine.confirm_download()) else {
            panic!("download expected")
        };
        machine.accept_download_failed();

        let UpdateCommand::Download {
            target,
            resume,
            cancel: retry_cancel,
        } = dispatched(machine.retry())
        else {
            panic!("download retry expected")
        };
        assert!(resume, "download retry resumes from the .partial");
        assert_eq!(target.version.to_string(), "99.0.0");
        assert_ne!(cancel, retry_cancel, "retry carries a fresh token");
        assert_eq!(machine.phase(), UpdatePhase::Downloading);

        machine.accept_download_verified();
        dispatched(machine.confirm_restart());
        machine.accept_install_failed();
        let UpdateCommand::Install { .. } = dispatched(machine.retry()) else {
            panic!("install retry expected")
        };
        assert_eq!(machine.phase(), UpdatePhase::ReadyToRestart);
    }

    #[test]
    fn install_failed_lands_in_failed_install_step() {
        let mut machine = UpdateMachine::new();
        started(&mut machine);
        machine.accept_manifest(manifest_of("99.0.0"), &manifest::current_version());
        dispatched(machine.confirm_download());
        machine.accept_download_verified();
        dispatched(machine.confirm_restart());

        assert!(machine.accept_install_failed());
        assert_eq!(machine.phase(), UpdatePhase::Failed(FailStep::Install));
    }

    #[test]
    fn cancel_only_interrupts_a_download() {
        let mut machine = UpdateMachine::new();
        assert!(!machine.cancel(), "idle has nothing to cancel");

        started(&mut machine);
        assert!(!machine.cancel(), "checking has no resumable middle state");

        machine.accept_manifest(manifest_of("99.0.0"), &manifest::current_version());
        let UpdateCommand::Download { cancel, .. } = dispatched(machine.confirm_download()) else {
            panic!("download expected")
        };

        assert!(machine.cancel());
        assert!(cancel.is_cancelled(), "cancel must stop the in-flight task");
        assert_eq!(machine.phase(), UpdatePhase::UpdateAvailable);
        assert!(
            machine.update().is_some(),
            "the verified update stays offered"
        );
        assert!(machine.current_cancel().is_none());

        assert!(!machine.cancel(), "second cancel is a no-op");
    }

    #[test]
    fn late_outcomes_are_rejected_by_phase_guards() {
        let mut machine = UpdateMachine::new();
        started(&mut machine);
        machine.accept_manifest(manifest_of("99.0.0"), &manifest::current_version());

        assert!(
            !machine.accept_manifest(manifest_of("98.0.0"), &manifest::current_version()),
            "a late manifest reply must not rewrite an accepted state"
        );
        assert!(
            !machine.accept_manifest_unavailable(),
            "a late manifest failure must not raise a card"
        );
        assert!(!machine.accept_download_verified());
        assert!(!machine.accept_download_failed());
        assert!(!machine.accept_install_failed());
        assert_eq!(
            machine.phase(),
            UpdatePhase::UpdateAvailable,
            "guards must leave the state untouched"
        );

        assert!(machine.retry().is_none(), "retry needs a recorded failure");
        assert!(
            !machine.accept_manifest_unavailable(),
            "retry path is guarded too"
        );
    }

    #[test]
    fn update_available_clears_when_a_new_check_finds_nothing_newer() {
        let mut machine = UpdateMachine::new();
        started(&mut machine);
        machine.accept_manifest(manifest_of("99.0.0"), &manifest::current_version());

        started(&mut machine);
        machine.accept_manifest(manifest_of("0.0.1"), &manifest::current_version());
        assert_eq!(machine.phase(), UpdatePhase::UpToDate);
        assert!(
            machine.update().is_none(),
            "the offered update is retracted"
        );
    }
}
