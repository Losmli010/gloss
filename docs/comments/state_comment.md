# state.rs 决策记录

> 自 `crates/gloss-app/src/update/state.rs` 迁出的代码注释。文件头 `//!` 已保留状态迁移表外三条语义与 `Failed` 重试语义，此处记录符号级契约。

## UpdatePhase

子状态机七相位（另加初始相位 `Idle`）：

- `Idle`：尚未发起过检查（仅首次静默检查之前存在）。
- `Checking`：清单拉取与校验进行中。
- `UpdateAvailable`：发现新版，待用户确认下载。
- `Downloading`：整包下载（含校验）进行中。
- `UpdateReady`：下载校验通过，待用户确认重启替换。
- `ReadyToRestart`：用户已确认，替换执行中（终结前最后一个相位）。
- `UpToDate`：已是最新。
- `Failed(FailStep)`：失败：记录失败步骤，重试回到该步。

## FailStep

失败步骤（[`UpdatePhase::Failed`] 的载荷）：

- `Manifest`：清单拉取/校验失败。
- `Download`：下载或 sha256/size 校验失败。
- `Install`：替换或重启失败。

## UpdateState

watch 频道广播的最新状态快照（UI 每帧读，不引入锁）。

- `phase`：当前相位。
- `version`：已锁定的更新目标版本（`UpdateAvailable` 及其后的失败态有值）。

## UpdateTarget

检查通过后锁定的更新目标：目标版本与本机架构的产物快照（下载与替换命令的载荷，从清单复制——清单本体用完即弃）。

- `version`：清单声明的目标版本。
- `artifact`：本机架构条目。

## UpdateCommand

状态机交给壳执行的动作：壳据此发起网络任务或替换流程。

- `CheckManifest`：拉取清单（启动静默检查与手动检查同一命令）；`cancel` 是本次检查的取消令牌（重复检查由状态机先取消旧令牌）。
- `Download`：下载目标产物；`resume` 为真时基于 `.partial` 续传（重试路径），为假时先清除 `.partial` 全量重下（首次确认路径）。`target` 是检查通过时锁定的更新目标；`cancel` 是本次下载的取消令牌（重复检查由状态机先取消旧令牌）。
- `Install`：执行替换并重启（调用前已完成两道确认的第二道）；`target` 是检查通过时锁定的更新目标。

## UpdateMachine

更新子状态机：纯状态 + 决策，无 IO。

- `update`：检查通过后锁定的更新目标；“无更新”“清单失败”后的重试会重新检查并覆盖，下载/替换失败的重试依赖它续跑，故随 Failed 保留。
- `cancel`：在途任务的取消令牌（检查与下载各一任；替换不可取消）。

## UpdateMachine::new

初始相位 `Idle`、无在途任务。

## UpdateMachine::phase / update / current_cancel

测试观察口：当前相位、当前锁定的更新目标、在途任务的取消令牌。

## UpdateMachine::snapshot

当前状态快照（watch 频道广播的载荷）。

## UpdateMachine::check

发起一次检查（启动静默检查 / 设置页检查共用入口）：`ReadyToRestart` 之外任意相位可发起，先取消在途任务（下载中断保留 `.partial`），落 `Checking` 并交出清单拉取命令。清单步重试同走此处。

## UpdateMachine::confirm_download

确认下载（两道确认的第一道）：仅 `UpdateAvailable` 可发起，落 `Downloading` 并交出全量下载命令（`.partial` 先清除）。

## UpdateMachine::confirm_restart

确认重启替换（两道确认的第二道）：仅 `UpdateReady` 可发起，落 `ReadyToRestart` 并交出替换命令；替换不可取消。

## UpdateMachine::retry

重试失败卡：回到记录的失败步骤（见模块注释）。

## UpdateMachine::cancel

取消在途下载：中断保留 `.partial`，回 `UpdateAvailable`。仅下载中有效；其余相位无事可做，返回 `false`。

## UpdateMachine::accept_manifest

采纳清单回包：守卫 `Checking`（迟到回包一律拒绝）。版本高于本地才置 `UpdateAvailable` 并锁定目标；相等或更低一律 `UpToDate`。

## UpdateMachine::accept_manifest_unavailable

采纳“清单不可用”：守卫 `Checking`，落清单步失败。

## UpdateMachine::accept_download_verified

采纳下载完成：守卫 `Downloading`，落 `UpdateReady`。

## UpdateMachine::accept_download_failed

采纳下载失败：守卫 `Downloading`，落下载步失败。

## UpdateMachine::accept_install_failed

采纳替换失败：守卫 `ReadyToRestart`，落替换步失败。

## UpdateMachine::fresh_token

换一枚新取消令牌并记为在途。
