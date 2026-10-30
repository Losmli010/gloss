# main.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关符号时同步更新本文件。

## 剪贴板图片哨兵的组装（2026-10-10，剪贴板图片方案 §5.2/§5.3）

- **为什么哨兵源与图片读取器在 `on_waker` 回调里同造（`ClipboardWatchSource::
  new` → `observer()` → `ClipboardImageReader::new`）**：①观察与②取材的竞态
  比对靠共享的 `PasteboardObserver` 句柄，两个消费者分属两个闭包
  （`event_sources` 的源列表与 `acquire_command_handler` 的命令处理器），
  唯一能同时递给两者的作用域就是组装回调；在回调里同造保证「句柄同源」
  在构造处即可见，而不是散在两个函数各自的参数默认值里。
- **为什么开关位是 `run_event_loop` 序盘创建的共享 `AtomicBool`
  （`clipboard_watch_enabled`），一份交 App（`Env` 同名字段）、一份随组装
  回调与哨兵源同造**（2026-10-10 整体验收修订）：初版只把启动快照的布尔
  值传进回调、原子位在回调里就地 `Arc::new`，App 拿不到句柄——设置页保存
  后无处置位，「热切换不重启」两个已登记 L4 步骤按现实现必失败。修订后
  序盘按快照初始化一份 `Arc<AtomicBool>`，两侧各持克隆：哨兵只读这一位
  （关态零开销），`save_settings` 落盘成功才置位（与运行时快照同进退，
  落盘失败不置位）。不走配置变更回调：`ConfigHandle` 无回调机制，为一位
  布尔建订阅是重武器；也不走配置快照——快照按批事件取，哨兵是逐 tick
  轮询的旁路读取点。
- **为什么 `AcquireClipboardImage` 的分支在 `acquire_command_handler` 里
  平铺而不是复用 `AcquireText` 的读写收口**：文本路径的成功形态是
  `TaskInput::Text`、失败恒上抛；图片路径多一个「竞态静默丢弃」的第三态
  （`Ok(None)` 不回传任何事件），且成功日志只记字节数。硬塞进同一收口
  需要把三态折叠回二元，可读性反而差；两个分支各自直白，共享的只有
  span 进入与 ④ 回传的形状。

> 按符号锚定（不写行号，防漂移）。本文件随仓库跟踪（docs/comments/ 入库）。

## preflight_event_permissions（启动骨架第 1.5 步）

- 2026-09-29 启动时预检辅助功能权限、缺失即触发系统授权引导：新装应用的第一次划词必然 AX 被拒，失败卡虽有指引但用户要先撞一次墙；把引导提前到启动，是 2026-09-28 真机排查（划词无弹窗、日志无记录）的直接教训。日志全部英文（约束），指引本体在失败卡（i18n）与系统对话框里。
- 2026-09-29 预检只覆盖辅助功能一项：输入监控无公开预检 API，其引导挂在手势降级提示上（build_event_sources 的 degraded 一次性提示，降级时顺带打开输入监控设置面板——打开失败只降级记日志，不拦启动）。

## preflight_input_monitoring 的移除与 permissions_ready 语义（2026-10-09）

- 2026-10-09 `preflight_input_monitoring` 整体删除（输入监控引导操作移除，缘由见 permissions_rs_comment.md）。`permissions_ready` 从「辅助功能 ∧ tap 建立成功」收敛为 `accessibility_ready`：门控语义是「没有系统授权引导弹窗在途」，移除输入监控引导后唯一可能在途的对话框只剩辅助功能引导，恰好由预检返回值完整表达；tap 失败不再暗示任何引导在途，保留合取只会无谓推迟预热。
- 2026-10-09 启动期 tap 成功改记 `event tap established for the selection gesture`（诚实表述）；启动失败日志仍在 mouse.rs 的 `spawn_tap`（warn），组装点不重复。build_event_sources 的运行中降级提示只留 warn，不再开任何设置面板。

## event_sources 的授权观察源（2026-10-09）

- 2026-10-09 `event_sources` 增加第三参 `accessibility_ready`：仅启动预检未就绪的会话武装 `AccessibilityWatch`（未武装时 poll 恒空、零查询成本），落定产出 `PlatformEvent::AccessibilityGranted` 经 ① 上行——事件线程不能碰通道③，预热补发必须由 App 收事件后执行（与手势同一条架构路径）。

## acquire_command_handler 的读取器状态（2026-10-12）

- 处理器注释从「选区组合读取器当前无状态」改为「激活器按 pid 记忆会话
  内已评估的前台应用」：CompositeReader 现在持有 AxActivator（AX 通道
  激活，见 selection/activation.rs），随闭包跨手势复用、单线程消费，
  无需跨通道传递状态。缘由与取舍见 activation_comment.md。

## 划词取材的收口时序（2026-10-16）

- AcquireText 处理器先上抛产物（InputReady/TaskFailed，触发弹窗链路），
  再 settle.finish() 收口兜底轮（恢复写盘 + 写盘归因）——恢复不挡在弹窗
  前；两步同在事件线程，哨兵消费时序（命令抽干后才轮询事件源）不变。

## GLOBAL_ALLOCATOR

- 全局分配器：长驻进程的常驻内存贴着活跃字节走（取舍见 Cargo.toml 依赖注）。

## log_dir / resolve_log_dir

- `log_dir`：日志目录——`GLOSS_LOG_DIR` 优先（量化脚本用它把测量日志隔离出日常日志），缺省 `~/.gloss/logs`（与 justfile 的 logs 配方保持一致）。
- `resolve_log_dir`：日志目录解析——覆盖值非空即用，否则回落 home 下的 `.gloss/logs`。

## ConfigWiring / load_config

- `ConfigWiring`：启动期装配产物——配置句柄（给 App 与引擎）与密钥存储（`Arc<dyn SecretStore>`，给引擎直查密钥；组合存储同时是配置句柄的落盘目标）。
- `load_config`（启动骨架第 2 步）：配置文件走标准配置目录的 `config.toml`，密钥走系统安全存储。
- 唯一必须成功的失败是「拿不到配置目录」——那时无处读写配置，属启动硬错误；文档本身损坏由 `ConfigHandle::load_or_default` 降级为出厂默认并记日志，应用照常起得来（用户还能进设置页改回来）。
- 返回句柄与存储两份：句柄给 App（任务选项）与引擎（端点），存储给引擎直查密钥——存储不下沉进句柄，因为密钥不经快照。

## build_service

- 装配推理服务（启动骨架第 6 步）：真实引擎。主产物缓存不在这里——缓存编排归 gloss-app 的消费桥（`pipeline::start_command_runtime`），服务只做渲染与转发。
- 引擎构造失败（HTTP/TLS 栈起不来）是启动硬错误——不装配服务就进事件循环的话，通道③没有消费者，用户触发的任务会静默石沉大海（「报错退出」而非带病运行）。

## run_event_loop

- 组装事件循环：拆分四通道端点（见 `wiring::distribute_channels`）、做启动期预检，随后启动应用——EventLoop 建好后的后台装配（tokio 消费桥与事件线程）在 `wiring::Assembly` 里进行，存活句柄记回组装态，循环结束后在这里统一收尾。
- 触发前场景探针：安全输入态与前台应用都由系统查询回答（纯查询、不索取新权限）。与系统语言同类——平台适配器的事，壳只消费。
- 启动钩子：更新子系统的进程内幂等入口，只拉起模块任务——静默检查在首帧就绪后由壳补发，发现新版仅置状态由设置页提示，失败落 Failed 供被动渲染，不打扰划词。
- 密钥预热门控（`permissions_ready`）：未就绪即辅助功能引导对话框在途，keychain 授权框不得抢在它之前（引导对话框无落定回调，就绪与否只能取启动时刻的静态事实），预热推迟到首次任务自然重读。
- 循环结束的收尾：App 已随事件循环结束 drop——通道②仅剩的 Sender（endpoints 内）归还后事件线程看到 Disconnected 自行退出；通道③关闭后消费循环退出，CommandRuntime drop 在超时内收尾 tokio。

## 首启权限引导顺序（2026-10-08，issue #134）

- 现象：首启三项引导出现顺序是「密码 → 监控输入 → 辅助功能」，期望「辅助功能 → 监控输入 → 密码」；密码（keychain 授权框）出现两次。
- 根因一（装配顺序）：原实现先 `MouseSource::spawn()` 再预检。监控输入的引导弹窗由 tap 建立动作本身异步触发，tap 在先就注定它先弹。修复为 `preflight_accessibility` → 建 tap → `preflight_input_monitoring`，原 `preflight_event_permissions` 一拆为二，调用序即引导序。
- 根因二（预热时机）：`resumed` 无条件发密钥预热，keychain 读取是同步阻塞弹框，抢在异步 TCC 弹窗之前。备选：轮询权限落定（需新增 listen-event 预检 FFI + 应用层定时器机械）或固定延时（赌负载，违背「同步点用事件不用时间」取向）。选定门控方案：组装点计算「两项授权启动时刻的合取」传入 Env，未就绪即跳过启动期预热，推迟到首次任务自然重读——TCC 弹窗无落定回调，就绪与否只能取启动时刻静态事实；代价是「AX 缺失、IM 已授予、会话内补授 AX」这一罕见路径下首次任务才弹 keychain 框，可接受（顺序仍正确）。
- 两次密码的构成：原行为下第 1 次来自启动预热弹框，第 2 次来自授权 IM 后重启的新进程（进程内缓存清零）。修复后权限未就绪的进程根本不预热，首启零密码框；权限就绪的进程至多一框，点「始终允许」后同构建不再弹。ad-hoc 跨版本重弹属系统语义，README 已注明，根治随正式分发。

## 取材失败先记日志再回传（P1-1 clone 削减，2026-10-13）

- 文本与剪贴板图片两条取材失败路径原是「先 `err.clone()` 发 TaskFailed、
  后用 `err` 记日志」；改为先按错误类别记日志、再把 `err` 原值 move 进
  事件——事件与日志是两个独立汇，先后无观察面，复制消失。
