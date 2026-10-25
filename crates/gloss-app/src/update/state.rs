//! 更新子状态机：纯状态转移，不做任何输入输出（IO）（与应用层状态机同构的
//! 函数式内核，同属纯状态机部分）。
//!
//! 触发入口（启动静默检查与设置页手动检查）、下载与替换都由壳执行——
//! 本模块只决策：用户意图（检查、确认下载、确认重启、重试、取消）与任务
//! 回包（各类回执）在此收敛成命令与状态，界面每帧读
//! 最新相位渲染。
//!
//! 状态迁移逐行对应迁移表；额外说明三条表外语义：
//! - 初始相位只存在于首次检查发起之前（启动静默
//!   检查在首帧就绪后补发，稳态不可达）；
//! - 取消只对下载中有效，回到待确认下载——清单已知且校验
//!   通过，该事实不因下载取消而消失；在途进度留在半成品文件（下次全量
//!   下载时清除，仅失败重试路径会续传）；检查中无处可回，不予取消
//!   （重复检查本就会取消在途任务）；
//! - 迟到回包由守卫拒绝（相位不符一律不采纳），已取消任务的回包在模块
//!   任务侧经取消令牌过滤后才进本机，两道防线互为备份。
//!
//! 失败相位记录失败步骤，重试回到该步骤：清单步 → 重新检查、下载步 →
//! 基于半成品文件续传、替换步 → 回到待确认重启替换（用户点重试即再次
//! 确认，安装直接重跑）。

use semver::Version;
use tokio_util::sync::CancellationToken;

use super::manifest::{self, Artifact, UpdateManifest};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdatePhase {
    #[default]
    Idle,
    Checking,
    UpdateAvailable,
    Downloading,
    UpdateReady,
    ReadyToRestart,
    UpToDate,
    Failed(FailStep),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailStep {
    Manifest,
    Download,
    Install,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpdateState {
    pub phase: UpdatePhase,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateTarget {
    pub version: Version,
    pub artifact: Artifact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCommand {
    CheckManifest {
        cancel: CancellationToken,
    },
    Download {
        target: UpdateTarget,
        resume: bool,
        cancel: CancellationToken,
    },
    Install {
        target: UpdateTarget,
    },
}

#[derive(Debug, Default)]
pub struct UpdateMachine {
    phase: UpdatePhase,
    update: Option<UpdateTarget>,
    cancel: Option<CancellationToken>,
}

impl UpdateMachine {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub fn phase(&self) -> UpdatePhase {
        self.phase
    }

    #[cfg(test)]
    pub fn update(&self) -> Option<&UpdateTarget> {
        self.update.as_ref()
    }

    #[cfg(test)]
    pub fn current_cancel(&self) -> Option<&CancellationToken> {
        self.cancel.as_ref()
    }

    pub fn snapshot(&self) -> UpdateState {
        UpdateState {
            phase: self.phase,
            version: self
                .update
                .as_ref()
                .map(|target| target.version.to_string()),
        }
    }

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

    pub fn confirm_restart(&mut self) -> Option<UpdateCommand> {
        let target = self.update.clone()?;
        if self.phase != UpdatePhase::UpdateReady {
            return None;
        }
        self.phase = UpdatePhase::ReadyToRestart;
        Some(UpdateCommand::Install { target })
    }

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
            version: manifest.into_version(),
            artifact,
        });
        self.phase = UpdatePhase::UpdateAvailable;
        true
    }

    pub fn accept_manifest_unavailable(&mut self) -> bool {
        if self.phase != UpdatePhase::Checking {
            return false;
        }
        self.cancel = None;
        self.phase = UpdatePhase::Failed(FailStep::Manifest);
        true
    }

    pub fn accept_download_verified(&mut self) -> bool {
        if self.phase != UpdatePhase::Downloading {
            return false;
        }
        self.cancel = None;
        self.phase = UpdatePhase::UpdateReady;
        true
    }

    pub fn accept_download_failed(&mut self) -> bool {
        if self.phase != UpdatePhase::Downloading {
            return false;
        }
        self.cancel = None;
        self.phase = UpdatePhase::Failed(FailStep::Download);
        true
    }

    pub fn accept_install_failed(&mut self) -> bool {
        if self.phase != UpdatePhase::ReadyToRestart {
            return false;
        }
        self.phase = UpdatePhase::Failed(FailStep::Install);
        true
    }

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
