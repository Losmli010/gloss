//! 应用内更新子系统：清单检查 → 整包下载校验 → 原位替换并重启。
//!
//! 与主流程隔离：界面经命令通道发指令、经状态通道读进度，模块自持专用线程与
//! 单线程运行时，不占四通道、不引入代数。
//! 启动静默检查在首帧就绪后补发；失败只落失败相位供设置页被动渲染。
//! 取消由模块内的取消令牌自管，迟到回包按令牌一律丢弃。

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use gloss_core::log::{debug, error, info, thread, warn};

pub mod download;
pub mod install;
pub mod manifest;
mod net;
pub mod state;

use self::download::DownloadError;
use self::install::InstallError;
use self::manifest::UpdateManifest;
use self::net::{fetch_manifest, shared_client};
use self::state::{UpdateCommand, UpdateMachine, UpdateState, UpdateTarget};

pub const MANIFEST_URL: &str = "https://losmli010.github.io/gloss/manifest.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateMsg {
    Check,
    Retry,
    ConfirmDownload,
    ConfirmRestart,
    Cancel,
}

type WakeSlot = Arc<OnceLock<Arc<dyn Fn() + Send + Sync>>>;

pub struct UpdateHandle {
    sender: mpsc::UnboundedSender<UpdateMsg>,
    receiver: watch::Receiver<UpdateState>,
    wake: WakeSlot,
}

impl UpdateHandle {
    pub fn send(&self, msg: UpdateMsg) {
        if let Err(err) = self.sender.send(msg) {
            warn!(
                thread = thread::UI,
                error = %err,
                "update: message dropped, module is gone"
            );
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<UpdateState> {
        self.receiver.clone()
    }

    pub fn install_wake(&self, wake: impl Fn() + Send + Sync + 'static) {
        self.wake.get_or_init(|| Arc::new(wake));
    }
}

pub struct UpdateWiring {
    pub receiver: watch::Receiver<UpdateState>,
    pub send: Arc<dyn Fn(UpdateMsg) + Send + Sync>,
}

impl UpdateWiring {
    pub fn from_handle(handle: &'static UpdateHandle) -> Self {
        Self {
            receiver: handle.subscribe(),
            send: Arc::new(move |msg| handle.send(msg)),
        }
    }
}

pub fn start_once() -> &'static UpdateHandle {
    static HANDLE: OnceLock<UpdateHandle> = OnceLock::new();
    HANDLE.get_or_init(spawn_module)
}

pub(crate) fn start_silent_check() {
    start_once().send(UpdateMsg::Check);
}

fn spawn_module() -> UpdateHandle {
    let (sender, receiver) = mpsc::unbounded_channel();
    let (state_tx, state_rx) = watch::channel(UpdateState::default());
    let wake: WakeSlot = Arc::default();
    let task_wake = Arc::clone(&wake);
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
            if runtime.block_on(run(
                receiver,
                Broadcaster {
                    tx: state_tx,
                    wake: task_wake,
                },
                real_hooks(),
            )) {
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
        wake,
    }
}

pub(crate) enum PendingResult {
    Checked(Result<UpdateManifest, ()>),
    Downloaded(Result<PathBuf, DownloadError>),
}

pub(crate) struct Hooks {
    pub(crate) check: CheckHook,
    pub(crate) download: DownloadHook,
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

impl Pending {
    fn check(hooks: &Hooks, cancel: CancellationToken) -> Self {
        Self {
            join: (hooks.check)(cancel.clone()),
            token: cancel,
            kind: PendingKind::Check,
        }
    }

    fn download(
        hooks: &Hooks,
        target: UpdateTarget,
        resume: bool,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            join: (hooks.download)(target, resume, cancel.clone()),
            token: cancel,
            kind: PendingKind::Download,
        }
    }
}

pub(crate) struct Broadcaster {
    tx: watch::Sender<UpdateState>,
    wake: WakeSlot,
}

impl Broadcaster {
    fn send(&self, state: UpdateState) {
        self.tx.send_replace(state);
        if let Some(wake) = self.wake.get() {
            wake();
        }
    }
}

pub(crate) async fn run(
    mut msg_rx: mpsc::UnboundedReceiver<UpdateMsg>,
    out: Broadcaster,
    hooks: Hooks,
) -> bool {
    let mut state = RunState {
        machine: UpdateMachine::new(),
        pending: None,
        zip: None,
    };
    out.send(state.machine.snapshot());

    loop {
        tokio::select! {
            msg = msg_rx.recv() => {
                match msg {
                    None => return false,
                    Some(msg) => {
                        if state.handle_msg(msg, &hooks, &out) {
                            return true;
                        }
                    }
                }
            }
            (result, token) = pending_outcome(&mut state.pending) => {
                if token.is_cancelled() {
                    debug!(thread = thread::TOKIO, "update: stale task reply dropped");
                } else {
                    apply_result(result, &mut state.machine, &mut state.zip, &out);
                }
                state.pending = None;
            }
        }
    }
}

struct RunState {
    machine: UpdateMachine,
    pending: Option<Pending>,
    zip: Option<PathBuf>,
}

impl RunState {
    fn handle_msg(&mut self, msg: UpdateMsg, hooks: &Hooks, out: &Broadcaster) -> bool {
        let command = match msg {
            UpdateMsg::Check => self.machine.check(),
            UpdateMsg::Retry => self.machine.retry(),
            UpdateMsg::ConfirmDownload => self.machine.confirm_download(),
            UpdateMsg::ConfirmRestart => self.machine.confirm_restart(),
            UpdateMsg::Cancel => {
                if self.machine.cancel() {
                    broadcast(&self.machine, out);
                }
                return false;
            }
        };
        self.apply_command(command, hooks, out)
    }

    fn apply_command(
        &mut self,
        command: Option<UpdateCommand>,
        hooks: &Hooks,
        out: &Broadcaster,
    ) -> bool {
        match command {
            Some(UpdateCommand::CheckManifest { cancel }) => {
                self.pending = Some(Pending::check(hooks, cancel));
                broadcast(&self.machine, out);
                false
            }
            Some(UpdateCommand::Download {
                target,
                resume,
                cancel,
            }) => {
                self.zip = None;
                self.pending = Some(Pending::download(hooks, target, resume, cancel));
                broadcast(&self.machine, out);
                false
            }
            Some(UpdateCommand::Install { .. }) => {
                broadcast(&self.machine, out);
                restart(&mut self.machine, &mut self.zip, hooks, out)
            }
            None => false,
        }
    }
}

fn restart(
    machine: &mut UpdateMachine,
    zip_slot: &mut Option<PathBuf>,
    hooks: &Hooks,
    out: &Broadcaster,
) -> bool {
    let Some(zip) = zip_slot.take() else {
        machine.accept_install_failed();
        broadcast(machine, out);
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
            broadcast(machine, out);
            false
        }
    }
}

fn apply_result(
    result: PendingResult,
    machine: &mut UpdateMachine,
    zip: &mut Option<PathBuf>,
    out: &Broadcaster,
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
    broadcast(machine, out);
}

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

fn broadcast(machine: &UpdateMachine, out: &Broadcaster) {
    out.send(machine.snapshot());
}

fn real_hooks() -> Hooks {
    Hooks {
        check: Arc::new(|_cancel| {
            tokio::spawn(async move {
                let result = match shared_client() {
                    Ok(client) => fetch_manifest(client, MANIFEST_URL).await,
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
                let result = match shared_client() {
                    Ok(client) => {
                        download::download(
                            client,
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

fn download_work_dir() -> PathBuf {
    std::env::temp_dir().join("gloss-update")
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

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
        wake_count: Arc<AtomicUsize>,
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

    fn stub_hooks(
        check: Arc<TriggerQueue<PendingResult>>,
        download: Arc<DownloadStub>,
        install_results: Arc<Mutex<VecDeque<Result<(), InstallError>>>>,
        install_calls: Arc<Mutex<Vec<PathBuf>>>,
    ) -> Hooks {
        Hooks {
            check: Arc::new(move |_cancel| {
                let stub = Arc::clone(&check);
                let (trigger, reply) = oneshot::channel();
                stub.register(trigger);
                tokio::spawn(async move { reply.await.unwrap_or(PendingResult::Checked(Err(()))) })
            }),
            download: Arc::new(move |target, resume, _cancel| {
                let stub = Arc::clone(&download);
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
                install_calls
                    .lock()
                    .expect("install calls")
                    .push(zip.to_path_buf());
                install_results
                    .lock()
                    .expect("install results")
                    .pop_front()
                    .unwrap_or(Ok(()))
            }),
        }
    }

    fn harness() -> Harness {
        let (tx, rx) = mpsc::unbounded_channel();
        let (state_tx, state_rx) = watch::channel(UpdateState::default());
        let wake_slot: WakeSlot = Arc::default();
        let wake_count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&wake_count);
        assert!(
            wake_slot
                .set(Arc::new(move || {
                    counter.fetch_add(1, Ordering::Relaxed);
                }))
                .is_ok(),
            "wake slot must be fresh"
        );
        let check = Arc::new(TriggerQueue::default());
        let download = Arc::new(DownloadStub::default());
        let install_results = Arc::new(Mutex::new(VecDeque::new()));
        let install_calls = Arc::new(Mutex::new(Vec::new()));
        let hooks = stub_hooks(
            Arc::clone(&check),
            Arc::clone(&download),
            Arc::clone(&install_results),
            Arc::clone(&install_calls),
        );
        let task = tokio::spawn(run(
            rx,
            Broadcaster {
                tx: state_tx,
                wake: wake_slot,
            },
            hooks,
        ));
        Harness {
            tx,
            rx: state_rx,
            task,
            check,
            download,
            install_results,
            install_calls,
            wake_count,
        }
    }

    async fn state_after(harness: &mut Harness) -> UpdateState {
        harness.rx.changed().await.expect("watch alive");
        harness.rx.borrow().clone()
    }

    async fn reach_update_ready(harness: &mut Harness) {
        assert_eq!(state_after(harness).await, UpdateState::default());
        harness.tx.send(UpdateMsg::Check).expect("send");
        assert_eq!(
            state_after(harness).await.phase,
            state::UpdatePhase::Checking
        );
        harness
            .check
            .complete(PendingResult::Checked(Ok(manifest_of("99.0.0"))));
        assert_eq!(
            state_after(harness).await.phase,
            state::UpdatePhase::UpdateAvailable
        );
        harness.tx.send(UpdateMsg::ConfirmDownload).expect("send");
        assert_eq!(
            state_after(harness).await.phase,
            state::UpdatePhase::Downloading
        );
        harness
            .download
            .triggers
            .complete(PendingResult::Downloaded(Ok(PathBuf::from(
                "/tmp/gloss.zip",
            ))));
        assert_eq!(
            state_after(harness).await.phase,
            state::UpdatePhase::UpdateReady
        );
    }

    #[tokio::test]
    async fn initial_broadcast_is_the_idle_snapshot() {
        let mut harness = harness();
        assert_eq!(state_after(&mut harness).await, UpdateState::default());
    }

    #[test]
    fn shared_client_hands_out_one_process_wide_instance() {
        let first = shared_client().expect("client builds without network");
        let second = shared_client().expect("client builds without network");
        assert!(std::ptr::eq(first, second));
    }

    #[tokio::test]
    async fn state_transition_wakes_the_ui_without_input() {
        let mut harness = harness();
        assert_eq!(state_after(&mut harness).await, UpdateState::default());
        let after_initial = harness.wake_count.load(Ordering::Relaxed);

        harness.tx.send(UpdateMsg::Check).expect("send");
        assert_eq!(
            state_after(&mut harness).await.phase,
            state::UpdatePhase::Checking
        );
        assert_eq!(
            harness.wake_count.load(Ordering::Relaxed),
            after_initial + 1,
            "an accepted transition must wake the UI"
        );

        harness
            .check
            .complete(PendingResult::Checked(Ok(manifest_of("99.0.0"))));
        assert_eq!(
            state_after(&mut harness).await.phase,
            state::UpdatePhase::UpdateAvailable
        );
        assert_eq!(
            harness.wake_count.load(Ordering::Relaxed),
            after_initial + 2,
            "a background reply adoption must wake the UI too, no input involved"
        );
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
        reach_update_ready(&mut harness).await;

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
