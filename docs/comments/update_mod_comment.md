# update/mod.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），代码内不留指向本文件的指针。

## UpdateHandle::install_wake / Broadcaster（相位迁移的 UI 唤醒）

- 2026-09-27 为什么唤醒出口挂在模块的广播路径（Broadcaster::send），而不是设置页轮询或壳侧消费 watch：设置窗的帧只被窗口事件与 egui 截止时刻驱动，主线程空闲时睡在 `ControlFlow::Wait`，启动静默检查与下载完成恰恰发生在这种无输入等待期——只发 watch 的话界面停在陈旧相位（如已 `UpdateReady` 仍显示「正在下载」与失效的取消按钮），要等下一次鼠标移入才刷新。按「同步点用事件，不用时间」选事件唤醒（`UserEvent::RedrawSettings`），不引入周期 `request_repaint_after` 兜底轮询。
- 2026-09-27 为什么出口由组装点晚装（`OnceLock` 槽）而不是 `start_once` 时备好：模块线程起于组装点调用 `start_once` 之时，先于事件循环存在，而唤醒句柄（`Waker`）要到 `app::run` 的 `on_waker` 回调才有；槽让安装可以晚于模块启动。错过安装前的广播无碍——那时设置窗不可能可见，用户打开时 `draw_settings` 自然读到 watch 里的最新快照。
- 2026-09-27 为什么唤醒与 `send_replace` 同点发出（同一函数内先发后唤）：跨线程可见性靠这个先后顺序保证，主线程处理 `RedrawSettings` 时 watch 里必已是触发唤醒的那份快照；迟到回包被令牌作废的路径不采纳也不广播，自然也不唤醒。
- 2026-09-27 为什么出口是 `impl Fn + Send + Sync` 闭包而不是 `crate::app::Waker` 类型：app 依赖 update（壳消费接线），update 反向依赖会把分层倒过来；闭包让模块对「唤醒是什么」无感知，测试也只需要一个计数闭包。

## HTTP 客户端进程内唯一 + 静默检查延后（2026-10-09，PR #147）

- **为什么客户端只建一次**：macOS 上 `reqwest::Client` 构建要解析整套信任设置
  （SecTrustSettings，火焰图 82 样本 CPU），原先每次检查/下载各建一份；OnceLock 常驻后
  进程内至多一次。构建失败不污染缓存（先 `build_client()?` 再 `get_or_init`），下次任务
  重试自然重建——与旧「建失败按该次任务失败处理」语义一致。
- **为什么静默检查从组装点移到 `resumed` 末尾（06_ready 后）**：组装点首发会让信任链
  构建挤在字体装载与首帧预热的同一启动窗口里抢 CPU；06_ready 后补发把它移出首帧关键期。
  模块仍由组装点的 `start_once` 拉起（句柄要先交给 `UpdateWiring`），
  `start_silent_check` 只是延后那条 Check 消息。
- **为什么补发点安全**：`install_wake` 在 `app::run` 的 on_waker 回调（EventLoop 建成即
  调）安装，必然先于 `resumed` 的首个回调；`Broadcaster` 在出口未装时只更新 watch，
  兜底不丢状态。`resumed` 的重复投递由 `is_ready()` 早退守卫，`start_silent_check` 恰在
  守卫之后。
- **存量观察（本 PR 不动）**：`check_now` 在生产代码无调用点——设置页手动检查实际走
  `SettingsAction::Update` → `env.update.send`，`check_now` 的「设置页手动检查」描述
  不准确，待另行清理。

## Pending 的 panic 类 JoinError 降级与 release panic=abort（P2-1，2026-10-13）

- 为什么 release 上 `panic = "abort"` 后 `pending_outcome` 的 panic 类 JoinError
  分支保留而不删除：release 下 panic 的任务在事发点 abort 进程、该分支不可达
  （更新模块的后台 panic 同样从「降级成失败回包」变为「进程即崩」——行为
  变更须在 PR 描述显式记录，开 PR 时落实）；dev/test 构建仍是 unwind（test 目标由 cargo 强制 unwind），降级照常
  （Check→清单失败、Download→Network 失败，均带痕迹）。保留缘由与 pipeline 同：
  测试 profile 的行为不退化、回滚即删 profile 一行；`Pending` 与 `pending_outcome`
  的文档已旁注「生效边界」。

## MANIFEST_URL / UpdateMsg / WakeSlot

- `MANIFEST_URL`：清单地址：信任根是 GitHub Pages 的 TLS，故写死 https，且客户端不跟随重定向（重定向会把请求交给别的宿主）。
- `UpdateMsg`：UI → 模块的命令（mpsc）。`Cancel`：在途进度留在 `.partial`，只有失败重试路径会基于它续传。
- `WakeSlot`：相位迁移的 UI 唤醒出口槽：模块广播时读，组装点晚装（唤醒句柄在事件循环建好后才存在）；未安装时广播只更新 watch。

## UpdateHandle（模块的 UI 侧句柄）

- 设置页经它发消息、每帧读最新状态。
- `send`：模块已死（线程起来失败等）时记日志丢弃，不崩 UI。
- `install_wake`：首次安装生效，重复调用忽略；此后每次状态广播调用它一次，用来唤醒睡在 `ControlFlow::Wait` 的主线程重绘设置窗。

## UpdateWiring（壳侧的更新接线）

- 状态订阅 + 命令出口。测试注入本地 watch 与空发送，不触碰真实网络。

## start_once / start_silent_check / spawn_module

- `start_once`：启动钩子：进程内幂等，首次调用只拉起模块线程（停在 `Idle`），不发起检查——静默检查由 `start_silent_check` 在首帧就绪后补发，避免挤占启动窗口。
- `start_silent_check`：启动静默检查：发现新版只置状态由设置页提示，失败落 `Failed` 供被动渲染，不打扰划词。
- `spawn_module`：模块线程：专用通道 + 独立 current-thread 运行时。线程/运行时起来失败属降级——日志有痕、更新不可用，主流程不受影响。

## PendingResult / Hooks / Pending / Broadcaster

- `PendingResult`：在途任务的结果：检查或下载，二者必有其一。
- `Hooks`：模块任务对外的动作集合：真实实现走网络与文件系统，L1 时序测试注入桩（同步点全走 watch，不碰网络）。
- `Pending`：在途任务：结果回包、取消令牌与任务类别一起携带——迟到回包按令牌核对丢弃，任务 panic（JoinError）按类别降级成对应失败。release 以 `panic = "abort"` 构建时 panic 的任务在事发点直接 abort 进程，panic 分支只在 unwind 构建（dev/test）可达。`check`：起一个清单检查任务：取消令牌同时握在回包侧（迟到回包凭它作废）。`download`：起一个下载任务；`zip` 槽位由调用方清空（新产物尚未落位）。
- `Broadcaster`：模块的状态出口：watch 发送端 + 与 `UpdateHandle` 共享的唤醒槽。`send`：主线程睡在 `ControlFlow::Wait` 时靠唤醒出口醒来重绘设置窗。

## run（模块任务主循环）

- 用户消息与在途回包在此收敛；每一步状态机采纳后都广播最新快照。返回真表示替换已成功（调用方随即退出进程）。
- 迟到回包（令牌已被新检查/取消作废）一律丢弃，不回写状态。

## RunState（模块运行态）

- 状态机与它之外的两件局量（在途任务、zip 槽位）一起走完一次运行——消息处理与回包采纳都以它为对象，不必逐层拖一组 `&mut`。
- `handle_msg`：一条用户消息的去向；确认重启替换成功返回真（调用方随即退出进程）。
- `apply_command`：执行状态机给出的一条命令：起在途任务（新下载清空 zip 槽位）或跑替换。返回真表示替换已成功。入口与命令变体的配对由 `UpdateMachine` 的方法保证：`check` / `confirm_download` / `confirm_restart` 返回 `Some` 时恒为各自的变体（相位不符时返回 `None`），`retry` 返回三者之一——逐入口的变体匹配原样保留在这一层契约里。进入 ReadyToRestart 先广播（替换执行中），再跑替换。

## restart / apply_result / pending_outcome / broadcast / real_hooks / download_work_dir

- `restart`：zip 缺失（防御分支）或替换失败都落 `Failed(Install)`；失败时 zip 放回槽位——重试回到替换步还要靠它。
- `apply_result`：在途回包的采纳：按结果类型进状态机，成功路径记下 zip 落位路径。
- `pending_outcome`：在途回包流：无在途任务时永远挂起（select 的另一半继续等消息）。任务 panic 在这里降级成对应失败，主循环不崩；release 的 `panic = "abort"` 构建下该分支不可达（见 `Pending`）。
- `broadcast`：广播最新快照（watch 的 send_replace 每次都唤醒订阅者；唤醒出口随 `Broadcaster::send` 一并触发）。
- `real_hooks`：真实 hooks：清单与整包下载共用进程内唯一 HTTP 客户端（见 `shared_client`），替换走 `install::install`。
- `download_work_dir`：下载工作目录（临时目录）：zip 的落位卷不必与 bundle 同卷——只有解压目标必须同卷（ditto 跨卷复制无碍，改名对必须同卷）。
