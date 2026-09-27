//! 应用内更新子系统：清单检查 → 整包下载校验 → 原位替换重启。
//!
//! 与主流程完全隔离：自持专用通道与自己的 tokio 任务（独立线程上的
//! current-thread 运行时），不占用四通道（`Command`/`Event` 等）、不引入
//! 代数（generation）。UI → 模块走 mpsc（[`UpdateMsg`]），模块 → UI 走
//! `watch` 广播最新 [`UpdateState`]——设置页每帧从 receiver 读最新状态
//! 渲染，不引入 `Arc<Mutex<…>>`。
//!
//! 触发入口只有两个：[`start_once`]（启动钩子，进程内幂等，首次调用拉起
//! 模块任务并发起一次静默检查，失败落 `Failed` 供设置页被动渲染、不弹层）
//! 与 [`check_now`]（设置页手动检查）。两道确认（确认下载 / 确认重启替换）
//! 由设置页经句柄发对应消息；重试是设计 §4.1 迁移表的「用户点重试」事件，
//! 比消息草案多出的 [`UpdateMsg::Retry`] 为它服务。
//!
//! 取消由模块内部 `CancellationToken` 自管：重复检查先取消在途任务；已
//! 取消任务的迟到回包经令牌核对一律丢弃，不回写状态（与状态机相位守卫
//! 互为备份）。

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use futures::StreamExt;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use gloss_core::log::{debug, error, info, thread, warn};

pub mod download;
pub mod install;
pub mod manifest;
pub mod state;

use self::download::DownloadError;
use self::install::InstallError;
use self::manifest::UpdateManifest;
use self::state::{UpdateCommand, UpdateMachine, UpdateState, UpdateTarget};

/// 清单地址（GitHub Pages，设计 §4.1 的常量）。
pub const MANIFEST_URL: &str = "https://losmli010.github.io/gloss/manifest.json";

/// 清单 body 上限：清单是几百字节级的小文档，1 MiB 已远超合理尺寸，
/// 超限按清单失败处理（无界内存禁入）。
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

/// UI → 模块的命令（mpsc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateMsg {
    /// 发起一次检查（启动静默检查与手动检查同型）。
    Check,
    /// 重试失败卡（回到记录的失败步骤）。
    Retry,
    /// 确认下载（两道确认的第一道）。
    ConfirmDownload,
    /// 确认重启替换（两道确认的第二道）。
    ConfirmRestart,
    /// 取消在途下载（在途进度留在 `.partial`，下次全量下载时清除；
    /// 仅失败重试路径会基于它续传）。
    Cancel,
}

/// 模块的 UI 侧句柄：设置页经它发消息、每帧读最新状态。
pub struct UpdateHandle {
    sender: mpsc::UnboundedSender<UpdateMsg>,
    receiver: watch::Receiver<UpdateState>,
}

impl UpdateHandle {
    /// 投递一条命令；模块已死（线程起来失败等）时记日志丢弃，不崩 UI。
    pub fn send(&self, msg: UpdateMsg) {
        if let Err(err) = self.sender.send(msg) {
            warn!(
                thread = thread::UI,
                error = %err,
                "update: message dropped, module is gone"
            );
        }
    }

    /// 订阅状态广播（每帧从 receiver 读最新 [`UpdateState`]）。
    pub fn subscribe(&self) -> watch::Receiver<UpdateState> {
        self.receiver.clone()
    }
}

/// 壳侧的更新接线：状态订阅 + 命令出口。生产经 [`UpdateWiring::from_handle`]
/// 接到全局模块；测试注桩（本地 watch + 空发送），不触碰真实网络。
pub struct UpdateWiring {
    /// watch 广播的最新 [`UpdateState`]（UI 每帧读）。
    pub receiver: watch::Receiver<UpdateState>,
    /// 命令出口（模块已死时发送侧自行记日志丢弃）。
    pub send: Arc<dyn Fn(UpdateMsg) + Send + Sync>,
}

impl UpdateWiring {
    /// 从全局句柄接线（组装点专用）。
    pub fn from_handle(handle: &'static UpdateHandle) -> Self {
        Self {
            receiver: handle.subscribe(),
            send: Arc::new(move |msg| handle.send(msg)),
        }
    }
}

/// 启动钩子入口：进程内幂等。首次调用拉起模块线程并发起一次静默检查，
/// 后续调用只返回既有句柄。
pub fn start_once() -> &'static UpdateHandle {
    static HANDLE: OnceLock<UpdateHandle> = OnceLock::new();
    HANDLE.get_or_init(|| {
        let handle = spawn_module();
        handle.send(UpdateMsg::Check);
        handle
    })
}

/// 设置页手动检查：除 `ReadyToRestart`（替换执行中）外任意态可发起，
/// 先取消在途任务（状态机负责）。
pub fn check_now() {
    start_once().send(UpdateMsg::Check);
}

/// 模块线程：专用通道 + 独立 current-thread 运行时。线程/运行时起来
/// 失败属降级（日志有痕，更新功能不可用，主流程不受影响）。
fn spawn_module() -> UpdateHandle {
    let (sender, receiver) = mpsc::unbounded_channel();
    let (state_tx, state_rx) = watch::channel(UpdateState::default());
    let spawn = std::thread::Builder::new()
        .name("gloss-update".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(err) => {
                    error!(
                        thread = thread::TOKIO,
                        error = %err,
                        "update: runtime build failed, update disabled"
                    );
                    return;
                }
            };
            if runtime.block_on(run(receiver, state_tx, real_hooks())) {
                info!(
                    thread = thread::TOKIO,
                    "update: installed, restarting process"
                );
                std::process::exit(0);
            }
        });
    if let Err(err) = spawn {
        error!(
            thread = thread::UI,
            error = %err,
            "update: worker thread spawn failed, update disabled"
        );
    }
    UpdateHandle {
        sender,
        receiver: state_rx,
    }
}

/// 在途任务的结果：检查或下载，二者必有其一。
pub(crate) enum PendingResult {
    /// 清单拉取结果（网络/解析失败收敛成 `Err`，细节只进日志）。
    Checked(Result<UpdateManifest, ()>),
    /// 下载落位结果（成功带最终 zip 路径）。
    Downloaded(Result<PathBuf, DownloadError>),
}

/// 模块任务对外执行的动作集合：真实实现走网络与文件系统，L1 时序
/// 测试注入桩（本地 oneshot 触发，不碰网络，同步点全走 watch）。
pub(crate) struct Hooks {
    /// 发起一次清单拉取，回传结果。
    pub(crate) check: CheckHook,
    /// 发起一次整包下载，回传结果。
    pub(crate) download: DownloadHook,
    /// 执行替换（不含重启接力；成功后由调用方退出进程）。
    pub(crate) install: InstallHook,
}

pub(crate) type CheckHook =
    Arc<dyn Fn(CancellationToken) -> tokio::task::JoinHandle<PendingResult> + Send + Sync>;
pub(crate) type DownloadHook = Arc<
    dyn Fn(UpdateTarget, bool, CancellationToken) -> tokio::task::JoinHandle<PendingResult>
        + Send
        + Sync,
>;
pub(crate) type InstallHook = Arc<dyn Fn(&Path) -> Result<(), InstallError> + Send + Sync>;

/// 在途任务：结果回包、取消令牌与任务类别一起携带——迟到回包按令牌
/// 核对丢弃；任务自身 panic（JoinError）按类别降级成对应失败（有痕迹）。
struct Pending {
    token: CancellationToken,
    kind: PendingKind,
    join: tokio::task::JoinHandle<PendingResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingKind {
    Check,
    Download,
}

/// 模块任务主循环：用户消息与在途回包在此收敛；每一步状态机采纳后都
/// 广播最新快照。返回真表示替换已成功（调用方随即退出进程）。
pub(crate) async fn run(
    mut msg_rx: mpsc::UnboundedReceiver<UpdateMsg>,
    state_tx: watch::Sender<UpdateState>,
    hooks: Hooks,
) -> bool {
    let mut machine = UpdateMachine::new();
    let mut pending: Option<Pending> = None;
    let mut zip: Option<PathBuf> = None;
    state_tx.send_replace(machine.snapshot());

    loop {
        tokio::select! {
            msg = msg_rx.recv() => {
                match msg {
                    None => return false,
                    Some(msg) => {
                        if handle_msg(msg, &mut machine, &mut pending, &mut zip, &hooks, &state_tx) {
                            return true;
                        }
                    }
                }
            }
            (result, token) = pending_outcome(&mut pending) => {
                // 迟到回包（令牌已被新检查/取消作废）一律丢弃，不回写状态。
                if token.is_cancelled() {
                    debug!(thread = thread::TOKIO, "update: stale task reply dropped");
                } else {
                    apply_result(result, &mut machine, &mut zip, &state_tx);
                }
                pending = None;
            }
        }
    }
}

/// 一条用户消息的去向；确认重启替换成功替换时返回真（调用方随即退出
/// 进程）。
fn handle_msg(
    msg: UpdateMsg,
    machine: &mut UpdateMachine,
    pending: &mut Option<Pending>,
    zip: &mut Option<PathBuf>,
    hooks: &Hooks,
    state_tx: &watch::Sender<UpdateState>,
) -> bool {
    match msg {
        UpdateMsg::Check => {
            if let Some(UpdateCommand::CheckManifest { cancel }) = machine.check() {
                *pending = Some(Pending {
                    join: (hooks.check)(cancel.clone()),
                    token: cancel,
                    kind: PendingKind::Check,
                });
                broadcast(machine, state_tx);
            }
        }
        UpdateMsg::Retry => match machine.retry() {
            Some(UpdateCommand::CheckManifest { cancel }) => {
                *pending = Some(Pending {
                    join: (hooks.check)(cancel.clone()),
                    token: cancel,
                    kind: PendingKind::Check,
                });
                broadcast(machine, state_tx);
            }
            Some(UpdateCommand::Download {
                target,
                resume,
                cancel,
            }) => {
                *zip = None;
                *pending = Some(Pending {
                    join: (hooks.download)(target, resume, cancel.clone()),
                    token: cancel,
                    kind: PendingKind::Download,
                });
                broadcast(machine, state_tx);
            }
            Some(UpdateCommand::Install { .. }) => {
                // 进入 ReadyToRestart 先广播（替换执行中），再跑替换。
                broadcast(machine, state_tx);
                return restart(machine, zip, hooks, state_tx);
            }
            None => {}
        },
        UpdateMsg::ConfirmDownload => {
            if let Some(UpdateCommand::Download {
                target,
                resume,
                cancel,
            }) = machine.confirm_download()
            {
                *zip = None;
                *pending = Some(Pending {
                    join: (hooks.download)(target, resume, cancel.clone()),
                    token: cancel,
                    kind: PendingKind::Download,
                });
                broadcast(machine, state_tx);
            }
        }
        UpdateMsg::ConfirmRestart => {
            if let Some(UpdateCommand::Install { .. }) = machine.confirm_restart() {
                // 进入 ReadyToRestart 先广播（替换执行中），再跑替换。
                broadcast(machine, state_tx);
                return restart(machine, zip, hooks, state_tx);
            }
        }
        UpdateMsg::Cancel => {
            if machine.cancel() {
                broadcast(machine, state_tx);
            }
        }
    }
    false
}

/// 执行替换并收尾：zip 缺失（不应发生，防御分支）或替换失败都落
/// `Failed(Install)` 广播；成功返回真。替换失败时 zip 放回槽位——
/// 重试回到替换步还靠它。
fn restart(
    machine: &mut UpdateMachine,
    zip_slot: &mut Option<PathBuf>,
    hooks: &Hooks,
    state_tx: &watch::Sender<UpdateState>,
) -> bool {
    let Some(zip) = zip_slot.take() else {
        machine.accept_install_failed();
        broadcast(machine, state_tx);
        return false;
    };
    match (hooks.install)(&zip) {
        Ok(()) => true,
        Err(err) => {
            warn!(
                thread = thread::TOKIO,
                error = ?err,
                "update: install failed"
            );
            machine.accept_install_failed();
            *zip_slot = Some(zip);
            broadcast(machine, state_tx);
            false
        }
    }
}

/// 在途回包的采纳：按结果类型进状态机，成功路径记下 zip 落位路径。
fn apply_result(
    result: PendingResult,
    machine: &mut UpdateMachine,
    zip: &mut Option<PathBuf>,
    state_tx: &watch::Sender<UpdateState>,
) {
    match result {
        PendingResult::Checked(Ok(manifest)) => {
            machine.accept_manifest(manifest, &manifest::current_version());
        }
        PendingResult::Checked(Err(())) => {
            machine.accept_manifest_unavailable();
        }
        PendingResult::Downloaded(Ok(path)) => {
            *zip = Some(path);
            machine.accept_download_verified();
        }
        PendingResult::Downloaded(Err(err)) => {
            warn!(
                thread = thread::TOKIO,
                error = ?err,
                "update: download failed"
            );
            machine.accept_download_failed();
        }
    }
    broadcast(machine, state_tx);
}

/// 在途回包流：无在途任务时永远挂起（select 的另一半继续等消息）。
/// 任务 panic 按 [`PendingKind`] 降级为对应的失败回包，主循环不崩。
async fn pending_outcome(pending: &mut Option<Pending>) -> (PendingResult, CancellationToken) {
    match pending {
        Some(pending) => {
            let kind = pending.kind;
            let token = pending.token.clone();
            let result = match (&mut pending.join).await {
                Ok(result) => result,
                Err(join_err) => match kind {
                    PendingKind::Check => PendingResult::Checked(Err(())),
                    PendingKind::Download => PendingResult::Downloaded(Err(
                        DownloadError::Network(format!("download task panicked: {join_err}")),
                    )),
                },
            };
            (result, token)
        }
        None => std::future::pending().await,
    }
}

/// 广播最新快照（watch 的 send_replace 每次都唤醒订阅者）。
fn broadcast(machine: &UpdateMachine, state_tx: &watch::Sender<UpdateState>) {
    state_tx.send_replace(machine.snapshot());
}

/// 真实 hooks：清单拉取与整包下载各自现建 HTTP 客户端（建失败按该次
/// 任务失败处理，重试自然重建），替换走 [`install::install`]。
fn real_hooks() -> Hooks {
    Hooks {
        check: Arc::new(|_cancel| {
            tokio::spawn(async move {
                let result = match build_client() {
                    Ok(client) => fetch_manifest(&client, MANIFEST_URL).await,
                    Err(err) => {
                        warn!(
                            thread = thread::TOKIO,
                            error = %err,
                            "update: http client build failed"
                        );
                        Err(())
                    }
                };
                PendingResult::Checked(result)
            })
        }),
        download: Arc::new(|target, resume, cancel| {
            tokio::spawn(async move {
                let result = match build_client() {
                    Ok(client) => {
                        download::download(
                            &client,
                            &target.artifact,
                            &download_work_dir(),
                            resume,
                            &cancel,
                        )
                        .await
                    }
                    Err(err) => Err(DownloadError::Network(format!("build http client: {err}"))),
                };
                PendingResult::Downloaded(result)
            })
        }),
        install: Arc::new(install::install),
    }
}

/// 下载工作目录（临时目录）：zip 的落位卷不必与 bundle 同卷——解压目标
/// 才必须同卷（ditto 跨卷复制无碍，改名对必须同卷）。
fn download_work_dir() -> PathBuf {
    std::env::temp_dir().join("gloss-update")
}

/// 清单拉取：非 200、超限 body、UTF-8 不符、校验矩阵不符都收敛为失败
/// （细节只进日志，状态机只认失败）。
async fn fetch_manifest(client: &reqwest::Client, url: &str) -> Result<UpdateManifest, ()> {
    let response = client.get(url).send().await.map_err(|err| {
        warn!(thread = thread::TOKIO, error = %err, "update: manifest fetch failed");
    })?;
    let status = response.status();
    if status != reqwest::StatusCode::OK {
        warn!(thread = thread::TOKIO, status = %status, "update: manifest fetch not ok");
        return Err(());
    }
    // 分块累计、超限即断：content-length 缺省（HTTP/2 常见）时上限照样
    // 生效，不给无界缓冲留入口。
    let mut bytes: Vec<u8> = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|err| {
            warn!(thread = thread::TOKIO, error = %err, "update: manifest body read failed");
        })?;
        if bytes.len() + chunk.len() > MAX_MANIFEST_BYTES as usize {
            warn!(thread = thread::TOKIO, "update: manifest body too large");
            return Err(());
        }
        bytes.extend_from_slice(&chunk);
    }
    let text = std::str::from_utf8(&bytes).map_err(|err| {
        warn!(thread = thread::TOKIO, error = %err, "update: manifest body is not utf-8");
    })?;
    manifest::parse(text).map_err(|err| {
        warn!(thread = thread::TOKIO, error = ?err, "update: manifest rejected");
    })
}

/// 更新子系统的 HTTP 客户端：与 platform 的 LLM 客户端同一套取向——
/// 建连/读超时、明确 UA、不跟随重定向（重定向会改写信任根的宿主）。
fn build_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .user_agent(concat!("gloss/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::none())
        .build()
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use tokio::sync::{mpsc, oneshot, watch};

    use super::*;
    use crate::update::install::InstallError;

    const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn manifest_of(version: &str) -> UpdateManifest {
        let text = format!(
            r#"{{"schema":1,"version":"{version}","channels":{{"stable":{{
  "aarch64-apple-darwin":{{"url":"https://p/a.zip","size":1,"sha256":"{sha_a}"}},
  "x86_64-apple-darwin":{{"url":"https://p/x.zip","size":2,"sha256":"{sha_b}"}}}}}}}}"#,
            sha_a = SHA_A,
            sha_b = SHA_B,
        );
        manifest::parse(&text).expect("test manifest must validate")
    }

    struct Harness {
        tx: mpsc::UnboundedSender<UpdateMsg>,
        rx: watch::Receiver<UpdateState>,
        task: tokio::task::JoinHandle<bool>,
        check: Arc<TriggerQueue<PendingResult>>,
        download: Arc<DownloadStub>,
        install_results: Arc<Mutex<VecDeque<Result<(), InstallError>>>>,
        install_calls: Arc<Mutex<Vec<PathBuf>>>,
    }

    struct TriggerQueue<T> {
        senders: Mutex<VecDeque<oneshot::Sender<T>>>,
    }

    impl<T> Default for TriggerQueue<T> {
        fn default() -> Self {
            Self {
                senders: Mutex::new(VecDeque::new()),
            }
        }
    }

    impl<T> TriggerQueue<T> {
        fn register(&self, sender: oneshot::Sender<T>) {
            self.senders
                .lock()
                .expect("trigger queue")
                .push_back(sender);
        }

        fn complete(&self, value: T) {
            if let Some(sender) = self.senders.lock().expect("trigger queue").pop_front() {
                drop(sender.send(value));
            }
        }

        fn len(&self) -> usize {
            self.senders.lock().expect("trigger queue").len()
        }
    }

    #[derive(Default)]
    struct DownloadStub {
        triggers: TriggerQueue<PendingResult>,
        calls: Mutex<Vec<(String, bool)>>,
    }

    fn harness() -> Harness {
        let (tx, rx) = mpsc::unbounded_channel();
        let (state_tx, state_rx) = watch::channel(UpdateState::default());
        let check = Arc::new(TriggerQueue::default());
        let download = Arc::new(DownloadStub::default());
        let install_results = Arc::new(Mutex::new(VecDeque::new()));
        let install_calls = Arc::new(Mutex::new(Vec::new()));

        let check_stub = Arc::clone(&check);
        let download_stub = Arc::clone(&download);
        let install_results_stub = Arc::clone(&install_results);
        let install_calls_stub = Arc::clone(&install_calls);
        let hooks = Hooks {
            check: Arc::new(move |_cancel| {
                let stub = Arc::clone(&check_stub);
                let (trigger, reply) = oneshot::channel();
                stub.register(trigger);
                tokio::spawn(async move { reply.await.unwrap_or(PendingResult::Checked(Err(()))) })
            }),
            download: Arc::new(move |target, resume, _cancel| {
                let stub = Arc::clone(&download_stub);
                stub.calls
                    .lock()
                    .expect("download calls")
                    .push((target.artifact.url, resume));
                let (trigger, reply) = oneshot::channel();
                stub.triggers.register(trigger);
                tokio::spawn(async move {
                    reply
                        .await
                        .unwrap_or(PendingResult::Downloaded(Err(DownloadError::Cancelled)))
                })
            }),
            install: Arc::new(move |zip| {
                install_calls_stub
                    .lock()
                    .expect("install calls")
                    .push(zip.to_path_buf());
                install_results_stub
                    .lock()
                    .expect("install results")
                    .pop_front()
                    .unwrap_or(Ok(()))
            }),
        };
        let task = tokio::spawn(run(rx, state_tx, hooks));
        Harness {
            tx,
            rx: state_rx,
            task,
            check,
            download,
            install_results,
            install_calls,
        }
    }

    async fn state_after(harness: &mut Harness) -> UpdateState {
        harness.rx.changed().await.expect("watch alive");
        harness.rx.borrow().clone()
    }

    #[tokio::test]
    async fn initial_broadcast_is_the_idle_snapshot() {
        let mut harness = harness();
        assert_eq!(state_after(&mut harness).await, UpdateState::default());
    }

    #[tokio::test]
    async fn confirmed_flow_runs_check_download_install_to_completion() {
        let mut harness = harness();
        assert_eq!(state_after(&mut harness).await, UpdateState::default());

        harness.tx.send(UpdateMsg::Check).expect("send");
        assert_eq!(
            state_after(&mut harness).await.phase,
            state::UpdatePhase::Checking
        );

        harness
            .check
            .complete(PendingResult::Checked(Ok(manifest_of("99.0.0"))));
        let state = state_after(&mut harness).await;
        assert_eq!(state.phase, state::UpdatePhase::UpdateAvailable);
        assert_eq!(state.version.as_deref(), Some("99.0.0"));

        harness.tx.send(UpdateMsg::ConfirmDownload).expect("send");
        assert_eq!(
            state_after(&mut harness).await.phase,
            state::UpdatePhase::Downloading
        );

        harness
            .download
            .triggers
            .complete(PendingResult::Downloaded(Ok(PathBuf::from(
                "/tmp/gloss.zip",
            ))));
        assert_eq!(
            state_after(&mut harness).await.phase,
            state::UpdatePhase::UpdateReady
        );

        harness.tx.send(UpdateMsg::ConfirmRestart).expect("send");
        assert_eq!(
            state_after(&mut harness).await.phase,
            state::UpdatePhase::ReadyToRestart
        );

        let Harness {
            task,
            install_calls,
            ..
        } = harness;
        assert!(task.await.expect("task runs to completion"));
        let calls = install_calls.lock().expect("install calls");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], PathBuf::from("/tmp/gloss.zip"));
    }

    #[tokio::test]
    async fn not_newer_manifest_lands_in_up_to_date_without_a_target() {
        let mut harness = harness();
        assert_eq!(state_after(&mut harness).await, UpdateState::default());

        harness.tx.send(UpdateMsg::Check).expect("send");
        assert_eq!(
            state_after(&mut harness).await.phase,
            state::UpdatePhase::Checking
        );

        harness
            .check
            .complete(PendingResult::Checked(Ok(manifest_of(
                &manifest::current_version().to_string(),
            ))));
        let state = state_after(&mut harness).await;
        assert_eq!(state.phase, state::UpdatePhase::UpToDate);
        assert_eq!(state.version, None);
    }

    #[tokio::test]
    async fn manifest_failure_lands_in_failed_and_retry_rechecks() {
        let mut harness = harness();
        assert_eq!(state_after(&mut harness).await, UpdateState::default());

        harness.tx.send(UpdateMsg::Check).expect("send");
        assert_eq!(
            state_after(&mut harness).await.phase,
            state::UpdatePhase::Checking
        );

        harness.check.complete(PendingResult::Checked(Err(())));
        assert_eq!(
            state_after(&mut harness).await.phase,
            state::UpdatePhase::Failed(state::FailStep::Manifest)
        );

        harness.tx.send(UpdateMsg::Retry).expect("send");
        assert_eq!(
            state_after(&mut harness).await.phase,
            state::UpdatePhase::Checking
        );
        assert!(harness.check.len() >= 1, "retry must re-run the check hook");
    }

    #[tokio::test]
    async fn download_failure_lands_in_failed_and_retry_resumes() {
        let mut harness = harness();
        assert_eq!(state_after(&mut harness).await, UpdateState::default());

        harness.tx.send(UpdateMsg::Check).expect("send");
        assert_eq!(
            state_after(&mut harness).await.phase,
            state::UpdatePhase::Checking
        );
        harness
            .check
            .complete(PendingResult::Checked(Ok(manifest_of("99.0.0"))));
        assert_eq!(
            state_after(&mut harness).await.phase,
            state::UpdatePhase::UpdateAvailable
        );

        harness.tx.send(UpdateMsg::ConfirmDownload).expect("send");
        assert_eq!(
            state_after(&mut harness).await.phase,
            state::UpdatePhase::Downloading
        );
        harness
            .download
            .triggers
            .complete(PendingResult::Downloaded(Err(DownloadError::Network(
                "boom".into(),
            ))));
        assert_eq!(
            state_after(&mut harness).await.phase,
            state::UpdatePhase::Failed(state::FailStep::Download)
        );

        harness.tx.send(UpdateMsg::Retry).expect("send");
        assert_eq!(
            state_after(&mut harness).await.phase,
            state::UpdatePhase::Downloading
        );
        let calls = harness.download.calls.lock().expect("calls");
        assert_eq!(calls.len(), 2);
        assert!(!calls[0].1, "the first attempt downloads from scratch");
        assert!(calls[1].1, "the retry must carry resume=true");
    }

    #[tokio::test]
    async fn install_failure_lands_in_failed_install_and_retry_installs() {
        let mut harness = harness();
        assert_eq!(state_after(&mut harness).await, UpdateState::default());

        harness.tx.send(UpdateMsg::Check).expect("send");
        assert_eq!(
            state_after(&mut harness).await.phase,
            state::UpdatePhase::Checking
        );
        harness
            .check
            .complete(PendingResult::Checked(Ok(manifest_of("99.0.0"))));
        assert_eq!(
            state_after(&mut harness).await.phase,
            state::UpdatePhase::UpdateAvailable
        );
        harness.tx.send(UpdateMsg::ConfirmDownload).expect("send");
        assert_eq!(
            state_after(&mut harness).await.phase,
            state::UpdatePhase::Downloading
        );
        harness
            .download
            .triggers
            .complete(PendingResult::Downloaded(Ok(PathBuf::from(
                "/tmp/gloss.zip",
            ))));
        assert_eq!(
            state_after(&mut harness).await.phase,
            state::UpdatePhase::UpdateReady
        );

        harness
            .install_results
            .lock()
            .expect("results")
            .push_back(Err(InstallError::InvalidStructure));
        harness.tx.send(UpdateMsg::ConfirmRestart).expect("send");
        assert_eq!(
            state_after(&mut harness).await.phase,
            state::UpdatePhase::Failed(state::FailStep::Install)
        );

        harness.tx.send(UpdateMsg::Retry).expect("send");
        let Harness {
            task,
            install_calls,
            ..
        } = harness;
        assert!(task.await.expect("task runs to completion"));
        assert_eq!(
            install_calls.lock().expect("install calls").len(),
            2,
            "retry must re-run install"
        );
    }

    #[tokio::test]
    async fn cancel_during_download_returns_to_update_available() {
        let mut harness = harness();
        assert_eq!(state_after(&mut harness).await, UpdateState::default());

        harness.tx.send(UpdateMsg::Check).expect("send");
        assert_eq!(
            state_after(&mut harness).await.phase,
            state::UpdatePhase::Checking
        );
        harness
            .check
            .complete(PendingResult::Checked(Ok(manifest_of("99.0.0"))));
        assert_eq!(
            state_after(&mut harness).await.phase,
            state::UpdatePhase::UpdateAvailable
        );

        harness.tx.send(UpdateMsg::ConfirmDownload).expect("send");
        assert_eq!(
            state_after(&mut harness).await.phase,
            state::UpdatePhase::Downloading
        );

        harness.tx.send(UpdateMsg::Cancel).expect("send");
        assert_eq!(
            state_after(&mut harness).await.phase,
            state::UpdatePhase::UpdateAvailable
        );

        harness
            .download
            .triggers
            .complete(PendingResult::Downloaded(Ok(PathBuf::from(
                "/tmp/gloss.zip",
            ))));
        harness.tx.send(UpdateMsg::ConfirmDownload).expect("send");
        assert_eq!(
            state_after(&mut harness).await.phase,
            state::UpdatePhase::Downloading
        );
    }

    fn spawn_body_server(head: &str, body: &[u8]) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let head = head.to_owned();
        let body = body.to_vec();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                use std::io::{Read, Write};
                let mut buf = [0u8; 4096];
                drop(stream.read(&mut buf));
                drop(stream.write_all(head.as_bytes()));
                drop(stream.write_all(&body));
                drop(stream.flush());
                drop(stream.shutdown(std::net::Shutdown::Write));
            }
        });
        format!("http://{addr}/manifest.json")
    }

    fn manifest_body_wire(version: &str) -> String {
        format!(
            r#"{{"schema":1,"version":"{version}","channels":{{"stable":{{
  "aarch64-apple-darwin":{{"url":"https://p/a.zip","size":1,"sha256":"{sha_a}"}},
  "x86_64-apple-darwin":{{"url":"https://p/x.zip","size":2,"sha256":"{sha_b}"}}}}}}}}"#,
            sha_a = SHA_A,
            sha_b = SHA_B,
        )
    }

    #[tokio::test]
    async fn fetch_manifest_parses_a_valid_body_from_the_wire() {
        let body = manifest_body_wire("99.0.0");
        let url = spawn_body_server(
            &format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            ),
            body.as_bytes(),
        );
        let client = reqwest::Client::builder().build().expect("client");
        let manifest = fetch_manifest(&client, &url)
            .await
            .expect("valid wire manifest must parse");
        assert_eq!(manifest.version().to_string(), "99.0.0");
    }

    #[tokio::test]
    async fn fetch_manifest_rejects_oversize_body_without_content_length() {
        let url = spawn_body_server(
            "HTTP/1.1 200 OK
Connection: close

",
            &vec![b'a'; 24 * 64 * 1024],
        );
        let client = reqwest::Client::builder().build().expect("client");
        let result = fetch_manifest(&client, &url).await;
        assert_eq!(result, Err(()), "a 1.5 MiB body must be refused mid-stream");
    }
}
