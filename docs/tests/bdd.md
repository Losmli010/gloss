# 测试行为清单（`BDD`）

以测试代码为唯一事实源：本清单逐条描述仓库当前测试的行为；新增、修改、删除测试时在同一 `PR` 内登记并刷新该条目的更新时间；描述与代码冲突时以代码为准并立即修正。

组织：登记文件按包落位——根 `bdd.md` 留总纲（本节）、人工测试与发版人工步骤，包清单 `gloss-core.md`、`gloss-platform.md`、`gloss-app.md`、`gloss-eval.md`、`gloss.md` 各登记本包条目；单元测试章内逐源文件小节按源路径排序，新小节按序插入。条目四字段：测试名称、测试目标、测试场景（给定、当、则）、更新时间。总览是生成产物：`just test-index` 再生成，`just test-bdd` 校验新鲜度；查单个测试按名字在 `docs/tests/` 检索。

拆分前历史：2026-11 之前本清单为单文件，逐条登记的沿革见版本库历史（`git log docs/tests/bdd.md`）。

## 总览

本节由 `just test-index` 生成，请勿手改；`just test-bdd` 校验本节与登记文件一致。计数口径：表格计登记行（表头与分隔行除外），条目小节按标题计，逐源文件小节标题不是条目。

| 类别 | `bdd.md`（本文件） | [gloss-core.md](gloss-core.md) | [gloss-platform.md](gloss-platform.md) | [gloss-app.md](gloss-app.md) | [gloss-eval.md](gloss-eval.md) | [gloss.md](gloss.md) | 合计 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 人工测试 | 20 | 0 | 0 | 0 | 0 | 0 | 20 |
| 发版人工步骤 | 13 | 0 | 0 | 0 | 0 | 0 | 13 |
| 集成测试 | 0 | 0 | 0 | 17 | 0 | 0 | 17 |
| 性能测试 | 0 | 0 | 0 | 0 | 0 | 2 | 2 |
| 快照测试 | 0 | 0 | 0 | 46 | 0 | 0 | 46 |
| 单元测试 | 0 | 269 | 183 | 322 | 97 | 5 | 876 |

登记文件行数：`bdd.md`（本文件） 452 行、[gloss-core.md](gloss-core.md) 364 行、[gloss-platform.md](gloss-platform.md) 343 行、[gloss-app.md](gloss-app.md) 613 行、[gloss-eval.md](gloss-eval.md) 187 行、[gloss.md](gloss.md) 24 行。

## 人工测试

### watch_and_reader_carry_a_real_image_copy

- 测试目标：验证真实剪贴板链路上哨兵观察与取材读取的端到端协作（`--ignored`，`cargo test -p gloss-platform --lib clipboard::watch -- --ignored`）。
- 测试场景：给定真实剪贴板与武装哨兵（构造时记基线），当经 `NSPasteboard` 写入（TIFF）格式 `flavor` 的宽高各 2 像素图片，则哨兵恰产出一个观察事件（同读数下轮不再观察）且以观察句柄构造的读取器读回可解码的同尺寸位图（PNG）；再当写入仅含位图（PNG）单一 `flavor` 的宽高各 2 像素图片并重新武装哨兵，则同样恰产出一次观察并读回（纯位图粘贴板不再静默失败）。
- 测试步骤：

1. 运行总览中人工测试的命令（只跑 `clipboard::watch::live_tests`）
2. 测试会写真实剪贴板（先后写入 `TIFF` 与 `PNG-only` 测试图片；收场经 `Drop` 恢复测试开始时可读的文本内容，原内容若非文本则无法恢复），勿并行触发其它剪贴板测试

- 更新时间：2026-10-10

### reads_live_selection_when_authorized

- 测试目标：验证辅助功能授权下的真实选区读取。
- 测试场景：给定授权真机与前台选中文本，当 `reader.read()`，则读出非空选中文本。
- 测试步骤：

1. 系统设置 → 隐私与安全性 → 辅助功能 → 放行运行测试的终端应用
2. 在前台编辑器选中一段文字
3. 运行总览中人工测试的命令
4. 未授权时测试当场失败并打印修复指引

- 更新时间：2026-09-19

### reads_live_selection_via_simulated_copy

- 测试目标：验证辅助功能授权下经剪贴板兜底的真实选区读取。
- 测试场景：给定授权真机且前台有选中文本，当跑兜底读取，则读出非空文本。
- 测试步骤：

1. 系统设置 → 隐私与安全性 → 辅助功能 → 放行运行测试的终端应用
2. 前台保留可复制的文字
3. 运行总览中人工测试的命令
4. 测试会向前台应用注入 `Cmd+C` 并覆写系统剪贴板；全系列经 `CLIPBOARD_LIVE_LOCK` 串行，勿手动并行触发

- 更新时间：2026-09-19

### reads_live_selection_from_rich_clipboard_via_simulated_copy

- 测试目标：验证富剪贴板场景下兜底读取的恢复保真。
- 测试场景：给定预置富剪贴板与授权真机，当跑兜底读取，则读出选中文本且预置文本、自定义 `flavor` 原样恢复。
- 测试步骤：

1. 系统设置 → 隐私与安全性 → 辅助功能 → 放行运行测试的终端应用
2. 前台保留可复制的文字
3. 运行总览中人工测试的命令
4. 测试预置的剪贴板内容会被写入并恢复；经 `CLIPBOARD_LIVE_LOCK` 串行

- 更新时间：2026-09-19

### injected_drag_yields_selection_gesture

- 测试目标：验证真实 `CGEventTap` 下的注入拖拽手势判定。
- 测试场景：给定辅助功能授权与注入的拖拽序列（`(100,100)` 按下 → 五段移动 → 释放前真实睡过最短按压时长），当经真实事件 `tap`，则 2 秒内监听到手势且监听器未降级。
- 测试步骤：

1. 系统设置 → 隐私与安全性 → 辅助功能 → 放行运行测试的终端应用
2. 运行总览中人工测试的命令
3. 测试注入真实全局鼠标事件，屏幕光标会移动
4. 未授权时前置检查当场失败并打印修复指引

- 更新时间：2026-10-13

### keychain_round_trip_on_real_store

- 测试目标：验证真实 `keychain` 的写（删除加重建路径）→读→覆盖→删除往返。
- 测试场景：给定真实 `keychain` 测试服务名，当写→读→覆盖（再次走删除加重建）→删除，则各步读回一致、终态 `None`。
- 测试步骤：

1. 在非受限会话的终端运行总览中人工测试的命令
2. 沙箱或持续集成（CI）会拒绝 `keychain` 写入，属预期环境限制

- 更新时间：2026-10-06

### foreign_owned_entry_is_removed_by_reference_delete

- 测试目标：验证写删路径能移除非本二进制创建的 `keychain` 条目（条目所有权按创建二进制的代码签名归属，`dev` 重编译、换构建后旧条目对当前二进制必然是外部所有者，查询式 `SecItemDelete` 对它返回 `errSecInvalidOwnerEdit` 拒删）。
- 测试场景：给定经 `security` `CLI` 创建（外部所有者）的测试条目，当 `store.delete()`，则条目被引用式删除移除，`security find-generic-password` 不再命中。
- 测试步骤：

1. 在非受限会话的终端运行总览中人工测试的命令
2. 沙箱或持续集成（CI）会拒绝 `keychain` 写入（含 `CLI` 预置），属预期环境限制

- 更新时间：2026-10-14

### live_llm_streams_a_translation

- 测试目标：验证真实大语言模型（LLM）端点的流式翻译往返。
- 测试场景：给定 `GLOSS_LIVE_API_KEY`、`GLOSS_LIVE_BASE_URL`、`GLOSS_LIVE_MODEL` 三个变量与真实端点，当请求翻译并消费流，则累计 80 字符以上非空译文。
- 测试步骤：

1. 导出 `GLOSS_LIVE_API_KEY`、`GLOSS_LIVE_BASE_URL`、`GLOSS_LIVE_MODEL` 三个环境变量（密钥不打印）
2. 运行总览中人工测试的命令
3. 缺变量时测试当场打印可照抄的导出命令

- 更新时间：2026-09-19

### live_llm_streams_an_image_contract

- 测试目标：验证真实端点在（JSON）输出模式下接受图像请求并流式回传图注契约（契约校验与重试方案 `L4`：`vision` 与 `response_format` 流式真机验证）。
- 测试场景：给定 `GLOSS_LIVE` 三变量与支持图像输入的模型，当经 `LlmClient` 发送 `ImageExplain` 渲染的图像任务（请求恒带 `response_format: json_object`）并消费整条流，则增量非空、整段过 `validate_task(ImageExplain)` 且 `note` 非空；端点不支持图像输入或（JSON）模式时分别以 `UnsupportedModality` 与 `JsonModeUnsupported` 诊断当场失败。
- 测试步骤：

1. 导出 `GLOSS_LIVE_API_KEY`、`GLOSS_LIVE_BASE_URL`、`GLOSS_LIVE_MODEL` 三个环境变量（`MODEL` 须支持图像输入；密钥不打印）
2. 运行 `cargo test -p gloss-core -- --ignored live_llm`
3. 缺变量或模型不支持图像时测试当场打印修复指引（换 `vision` 模型重跑）

- 更新时间：2026-10-12

### fallback_read_restores_original_clipboard

- 测试目标：验证兜底读取后系统剪贴板恢复原内容。
- 测试场景：给定预置文本的系统剪贴板，当跑一次兜底读取（成败皆可），则原内容恢复（恢复只发生在成功读回且现值仍是自身读数的场合；失败轮一律不恢复——板上要么未变，要么是本轮之外的新写入或注入副产物，都不覆盖。注入副产物若为图片按外来写入照常触发图片任务）。另断言闩锁计数归因载荷：自报写盘数为 0（失败轮）或 2（读回成功加恢复），`pre` 不超前当前计数。
- 测试步骤：

1. 随常规测试自动运行，无需授权与 `--ignored`
2. 运行中会覆写并恢复本机剪贴板；经 `CLIPBOARD_LIVE_LOCK` 串行

- 更新时间：2026-10-15

### fallback_read_restores_multiflavor_clipboard

- 测试目标：验证多 `flavor` 剪贴板的恢复保真。
- 测试场景：给定“文本加自定义 `flavor`”双 `flavor` 条目，当跑一次兜底读取，则两种 `flavor` 字节原样恢复。
- 测试步骤：

1. 随常规测试自动运行，无需授权与 `--ignored`
2. 运行中会覆写并恢复本机剪贴板；经 `CLIPBOARD_LIVE_LOCK` 串行

- 更新时间：2026-09-19

### fallback_read_restores_empty_clipboard

- 测试目标：验证空剪贴板场景的兜底不引入内容。
- 测试场景：给定空剪贴板，当跑一次兜底读取，则剪贴板保持为空。
- 测试步骤：

1. 随常规测试自动运行，无需授权与 `--ignored`
2. 运行中会覆写并恢复本机剪贴板；经 `CLIPBOARD_LIVE_LOCK` 串行

- 更新时间：2026-09-19

### fallback_read_restores_multi_item_clipboard

- 测试目标：验证多条目剪贴板的逐条目恢复。
- 测试场景：给定两条目各带文本与自定义 `flavor`，当跑一次兜底读取，则四个 `flavor` 各归回原条目。
- 测试步骤：

1. 随常规测试自动运行，无需授权与 `--ignored`
2. 运行中会覆写并恢复本机剪贴板；经 `CLIPBOARD_LIVE_LOCK` 串行

- 更新时间：2026-09-19

### fallback_read_unwind_restores_the_clipboard_without_finish

- 测试目标：验证兜底读取在调用方 `panic` 展开、未收口 `finish` 时经 `Drop` 兜底按快照恢复原剪贴板（`PendingRestore` 的 `Drop` 路径——受控路径都已显式收口，`skip == false` 只在展开时出现）；粘贴板服务持续不可用使恢复重试耗尽时，退而验收契约规定的失败留痕（`error` 级“`failed to restore clipboard`”）。
- 测试场景：给定预置文本的剪贴板，当跑一次兜底读取后、`finish` 前把剪贴板覆写成注入产物并 `panic` 展开，则断言按事件同步推进而非时间窗单点观测：先经 `restores_on_unwind` 区分恢复契约——兜底整轮放弃（快照不可得）时无恢复无注入，板上保持覆写产物（外部写入不被兜底覆盖）；恢复有契约时在有界窗口内轮询等待恢复落地事件（板上回到预置文本）或失败留痕事件（`capture_global` 捕获，避开线程局部窗口的调用点缓存竞争），窗口上界覆盖恢复重试预算（6 次尝试 × 200 毫秒退避）加观测余量，事件在窗即通过、两事件皆空才判红。
- 测试步骤：

1. 随常规测试自动运行，无需授权与 `--ignored`
2. 运行中会覆写并恢复本机剪贴板；经 `CLIPBOARD_LIVE_LOCK` 串行

- 更新时间：2026-10-30

### fallback_read_restores_original_clipboard_on_windows

- 测试目标：验证系统（Windows）真机上兜底读取（模拟复制）后原剪贴板文本恢复。
- 测试场景：给定预置文本的剪贴板与前台选中文本，当跑一次兜底读取并收口，则读出非空选中文本且原文本恢复。
- 测试步骤：

1. 在系统（Windows）真机上运行总览中人工测试的命令（测试目标随目标平台编译，需 `x86_64-pc-windows-msvc` 或真机原生跑）
2. 前台保留可复制的文字（注入真实 `Ctrl+C`，光标焦点需在该应用）
3. 测试会覆写并恢复剪贴板；经 `CLIPBOARD_LIVE_LOCK` 串行，勿并行触发其它剪贴板测试

- 更新时间：2026-11-06

### fallback_read_unwind_restores_the_clipboard_without_finish_on_windows

- 测试目标：验证系统（Windows）真机上调用方未收口（`Drop` 路径）时注入产物不留在板上。
- 测试场景：给定预置文本的剪贴板与前台选中文本，当兜底读取后在收口前模拟调用方恐慌，则模拟覆写被 `Drop` 恢复为预置文本（放弃轮不注入即不恢复）。
- 测试步骤：

1. 同上一条的真机与前台前提
2. 测试会覆写并恢复剪贴板；经 `CLIPBOARD_LIVE_LOCK` 串行

- 更新时间：2026-11-06

### a_fallback_round_on_an_image_board_preserves_the_image_on_windows

- 测试目标：验证系统（Windows）真机上剪贴板挂图（截图注疏后的真实场景）不再让兜底轮整轮放弃：快照以图像风格原字节保全，注入照常进行，放弃恢复的轮次原图原样留板。
- 测试场景：给定仅挂图像风格（版本 5 位图（CF_DIBV5）探测字节）的剪贴板，当跑一次兜底读取并收口，则轮次照常前进（快照有保全——`restores_on_unwind` 为真）、前台无选中文本时按普通获取失败上抛、收口走跳过恢复（板上要么未变）、图像存在性与原字节逐字复读在板。
- 测试步骤：

1. 在系统（Windows）真机上运行总览中人工测试的命令（只跑 `selection::clipboard::live_tests`）
2. 前台停在任意应用且不要留选中文本（测试会向前台注入真实 `Ctrl+C`；有选中文本时该轮会走恢复路径，断言按无选区失败收场即红）
3. 测试会把剪贴板覆写为测试探测数据且不恢复原内容；经 `CLIPBOARD_LIVE_LOCK` 串行，勿并行触发其它剪贴板测试

- 更新时间：2026-11-10

### watch_and_reader_carry_a_real_image_copy_on_windows

- 测试目标：验证系统（Windows）真机上真实剪贴板链路（位图（CF_DIB）与便携式网络图形（PNG）注册格式）的哨兵观察与取材读取端到端协作（`--ignored`，系统（Windows）真机原生跑 `cargo test -p gloss-platform --lib clipboard::watch -- --ignored`）。
- 测试场景：给定真实剪贴板与武装哨兵（构造时记序列号基线），当以位图（CF_DIB）写入宽高各 2 像素的图片，则哨兵恰产出一个观察事件（同读数下轮不再观察）且以观察句柄构造的读取器读回可解码的同尺寸位图（PNG）；再当仅写便携式网络图形（PNG）注册格式并重新武装哨兵，则同样恰产出一次观察并读回。
- 测试步骤：

1. 在系统（Windows）真机上运行总览中人工测试的命令（只跑 `clipboard::watch::live_tests`）
2. 测试会写真实剪贴板（先后写入位图（CF_DIB）与仅便携式网络图形（PNG）的测试图片；收场经 `Drop` 恢复测试开始时可读的文本内容，原内容若非文本则无法恢复），勿并行触发其它剪贴板测试

- 更新时间：2026-11-07

### scene_probe_reports_the_frontmost_app

- 测试目标：验证真机上 `NSWorkspace` 取前台应用这条路径（`CI` 里只能断“一致快照”，`Some` 分支跑不到）。
- 测试场景：给定有前台应用的图形会话，当读一次场景事实，则报出前台应用且其身份字段非空。
- 测试步骤：

1. 在有窗口服务的会话里运行（无前台应用的环境会当场失败）
2. 运行总览中人工测试的命令
3. 前台开着任意应用即可；在密码管理器内划词的行为另见 `PR` 走查清单

- 更新时间：2026-09-26

### credential_round_trip_on_real_store

- 测试目标：验证真实凭据管理器写入路径的完整往返（`--ignored`，`cargo test -p gloss-platform --lib windows::storage::cred -- --ignored`）。
- 测试场景：给定真实凭据库与 `.test` 限定键串，当 `set`→`get`→`set`→`get`→`delete`→`get`，则读回两次写入值、删除后 `None`。
- 测试步骤：

1. 运行总览中人工测试的命令（只跑 `windows::storage::cred::tests::credential_round_trip_on_real_store`）
2. 测试写真实凭据管理器（键串带 `.test` 限定、收场即删），勿并行触发其它凭据测试

- 更新时间：2026-11-10

## 发版人工步骤

发版链路的一次性运维与真机步骤：没有可执行的自动化测试源码，`bdd` 门禁对本节豁免双向核对；
条目仍按“名称、目标、场景、照抄步骤、更新时间”登记，命名用测试风格的标识符。

### tray_menu_serves_settings_and_quit_on_windows

- 测试目标：验证系统（Windows）真机上托盘常驻与右键菜单两条词条动作（左键行为按移植方案后置，不在验收面）。
- 测试场景：给定真机上运行中的应用，当右键托盘图标逐项点选，则设置词条打开设置窗、退出词条结束进程且托盘图标随退出消失。
- 测试步骤：

1. 在系统（Windows）真机上运行应用（`cargo run` 即可）
2. 确认通知区域出现 `Gloss` 托盘图标；左键单击不弹菜单
3. 右键图标，确认菜单含“设置”与“退出 `Gloss`”两条词条（随系统语言）
4. 点“设置”，确认设置窗打开
5. 再右键点“退出 `Gloss`”，确认进程退出、托盘图标消失、日志无 `panic`

- 更新时间：2026-11-07

### real_update_round_trip_on_windows

- 测试目标：验证系统（Windows）真机上的真实清单拉取、整包下载校验与可执行文件替换重启（首个系统（Windows）发布缺可照抄的自更新验证清单，评审遗留 E3/F4 补登记）。
- 测试场景：给定运行着旧版 `Gloss` 便携压缩包（zip）的系统（Windows）真机（有网络），当设置页手动检查更新并确认下载、确认重启替换，则应用升到清单版本并正常启动，安装目录无替换残留。
- 测试步骤：

1. 在系统（Windows）真机把上一个发布版本的便携压缩包（zip）解压到独立目录并运行 `gloss.exe`
2. 设置页点“检查更新”，确认提示新版与目标版本
3. 确认下载，等待“更新就绪”提示
4. 点“重启更新”，确认替换后应用以新版本启动（进程重启且 `gloss.exe` 已换新）
5. 检查安装目录无 `.old` 残留、无解包临时目录残留；`just logs` 无 `panic`

- 更新时间：2026-11-07

### desktop_shortcut_is_ensured_on_startup

- 测试目标：验证系统（Windows）真机上启动期桌面快捷方式的幂等重建与跟随语义（桌面入口装配，评审提出“安装后自动建桌面图标”的验收清单）。
- 测试场景：给定桌面无快捷方式（或有旧快捷方式）的真机，当运行应用完成启动装配，则桌面出现指向当前可执行文件的 `Gloss` 快捷方式；可执行文件挪位后再次运行，则快捷方式跟随新位置。
- 测试步骤：

1. 在系统（Windows）真机删除桌面上的 `Gloss.lnk`（如有），记录安装目录位置
2. 运行应用（`cargo run` 或解压便携压缩包后的 `gloss.exe`），等待启动完成
3. 确认桌面出现 `Gloss` 快捷方式，图标为应用图标，双击可拉起应用
4. 把安装目录整体挪到另一位置，再次运行新位置的 `gloss.exe`，确认旧快捷方式指向新位置且双击可启动
5. 把 `gloss.exe` 直接复制到桌面运行一次，确认桌面不再多出指向自身的 `Gloss.lnk`
6. `just logs` 无 `panic`

- 更新时间：2026-11-10

### pages_source_is_github_actions

- 测试目标：验证仓库 `GitHub` `Pages` 已启用且 `source` 为 `GitHub` `Actions`（`deploy-web` `job` 的前置，一次性）。
- 测试场景：给定仓库管理员权限，当查看 `Settings` → `Pages`，则 `Build` `and` `deployment` 的 `Source` 为 `GitHub` `Actions`。
- 测试步骤：

1. 打开 `https://github.com/Losmli010/gloss/settings/pages`
2. `Build` `and` `deployment` → `Source` 选 `GitHub` `Actions`（若尚未选择）
3. 保存即可，无需手工建分支（`deploy-web` `job` 用 `actions/deploy-pages` 直接部署）

- 更新时间：2026-09-26

### pages_site_point_check

- 测试目标：验证 `Pages` 部署后站点的导航锚点、下载按钮、`manifest` 读取与降级路径。
- 测试场景：给定一次成功的 `deploy-web` 部署，当浏览器访问站点逐项点检，则锚点跳转正常、下载按钮指向 `latest/` 对应平台直链、`manifest.json` 可读取且字段齐备；`manifest` 取不到时页面降级为 `GitHub` `Releases` 外链。
- 测试步骤：

1. 打开 `https://losmli010.github.io/gloss/`
2. 依次点导航“任务、技术、下载、更新”，确认锚点跳转
3. 确认版本号显示；切换“苹果芯片（Apple Silicon）”“英特尔芯片（Intel）”与 `Windows x64`，确认下载链接随之指向对应平台的 `latest` 文件：系统（Windows）落压缩包（zip），苹果与英特尔芯片落镜像（dmg）
4. 确认首次打开指引随所选平台切换：系统（macOS）显示镜像拖入与放行指引，系统（Windows）显示解压与 `SmartScreen`“更多信息 → 仍要运行”指引
5. 直接访问 `https://losmli010.github.io/gloss/manifest.json`，确认 `schema/version/channels` 三平台字段齐备：苹果与英特尔芯片条目带 `dmg_url`，`Windows` 条目无安装器字段
6. 降级路径：本地 `just site-preview`（无 `manifest.json`）打开页面，确认显示“未能获取最新版本信息”且下载按钮退到 `GitHub` `Releases` 外链

- 更新时间：2026-11-07

### real_update_round_trip_on_device

- 测试目标：验证真机上的真实清单拉取、整包下载校验与替换重启，双架构各一次。
- 测试场景：给定装有旧版 `Gloss` 的真机（有网络），当设置页手动检查更新并确认下载、确认重启替换，则应用升到清单版本并正常启动，旧 `bundle` 无残留。
- 测试步骤：

1. 在 `Apple` `Silicon` 与 `Intel` 真机各装上一个发布版本的 `Gloss`
2. 设置页点“检查更新”，确认提示新版与目标版本
3. 确认下载，等待“更新就绪”提示
4. 点“重启更新”，确认替换后应用以新版本启动
5. 检查、`Applications` 无 `.app.old` 残留；`just logs` 无 `panic`

- 更新时间：2026-09-26

### release_pipeline_end_to_end

- 测试目标：验证打 `tag` 后 `Release` 与 `Pages` 同步发布的全链路。
- 测试场景：给定与版本单点一致的 `tag`，当推送 `tag` 触发 `release` `workflow`，则 `build/release` 产出 `Release` 资产、`deploy-web` 部署成功，站点 `manifest.json` 的 `version` 与 `tag` 一致。
- 测试步骤：

1. 本地 `just release-check vX.Y.Z` 确认版本一致后打 `tag` 并推送
2. 在 `Actions` 观察 `release` `workflow`：`build` → `release` → `deploy-web` 依次成功
3. 打开 `GitHub` `Release`，确认三平台共 5 个资产：苹果与英特尔芯片各 `zip` 与 `dmg` 共 4 个，`Windows` 压缩包 1 个
4. 打开站点确认 `manifest.json` 的 `version` 与 `tag`（去 `v`）一致，`latest/` 下 5 个文件可下载

- 更新时间：2026-11-07

### first_launch_guides_accessibility

- 测试目标：验证全新环境首次启动的系统级引导顺序与 `keychain` 授权框的落点（引导顺序口径：辅助功能 → 密钥），以及运行中授权落定后的预热补发。
- 测试场景：给定撤销 `TCC` 授权的干净环境，当首次启动应用，则弹“辅助功能”引导对话框且不出现其他系统授权引导；`keychain` 授权框不在它之前出现，且同一次进程内至多一次；运行中授予辅助功能后授权框随之出现（无须重启）。
- 测试步骤：

1. 撤销授权复位到首启状态：`tccutil reset Accessibility io.github.losmli010.gloss`（`bundle` `id` 以构建为准）
2. 首次启动应用，确认弹“辅助功能”引导对话框；期间不得出现“允许访问钥匙串”，也不得出现“监控输入”设置面板
3. 授予辅助功能：数秒内 `keychain` 旧条目的授权框出现（无须重启；授权观察器按 1 秒节流观察落定），点“始终允许”后同构建再重启不再弹
4. `ad-hoc` 构建跨版本重弹属预期（`README`“首次启动与系统授权”已注明）

- 更新时间：2026-10-09

### pasteboard_first_read_permission_closure

- 测试目标：验证系统（macOS）15 及以上版本首次读剪贴板内容的系统“允许粘贴”授权闭环：哨兵只读 `changeCount` 不触发授权，读内容才触发；拒绝后的失败路径可观察（失败卡或静默加日志）。
- 测试场景：给定系统（macOS）15 及以上真机、出厂默认配置与已授予的辅助功能，当开启“监听剪贴板图片”并复制一张图片，则首次出卡前系统弹“允许粘贴”授权框；允许后图片卡正常出卡（经为图、注为描述流式、疏为逐条解读）；拒绝后复制图片走可观察的失败路径，应用不崩、内容不落盘。
- 测试步骤：

1. 设置页开启“监听剪贴板图片”并保存（无需重启）
2. 在任意应用复制一张图片（`⌘C`），确认首次出卡前出现系统“允许粘贴”授权框；点“允许”，确认出卡
3. 再复制另一张图片：确认不再重复弹授权框、直接出卡
4. 拒绝路径：系统设置 → 隐私与安全性撤销 `Gloss` 的粘贴授权（或首次弹框点“不允许”）后复制图片，确认失败卡或静默（`just logs --no-follow --level warn` 有痕迹），应用不崩

- 更新时间：2026-10-10

### image_commentary_sources_toggle_and_limits_manual_checks

- 测试目标：验证图像卡的真实来源矩阵、监听开关热切换、`pastebomb` 双上限与同图缓存秒出（真机触发链路行为）。
- 测试场景：给定开启监听的真实剪贴板，当从各来源复制图片，则 `⌘⇧4+Ctrl` 截图、浏览器、微信、`Preview` 复制图片、`Excel` 图表均出卡；关闭开关后复制图片零反应；超大图片出“图片过大”失败卡；同图第二次复制缓存秒出；敏感场景下不触发。
- 测试步骤：

1. 开启“监听剪贴板图片”，逐来源复制并确认出卡（经为图、注为描述、疏为逐条）：`⌘⇧4+Ctrl` 截图；`Safari`、`Chrome` 复制图片；微信聊天图片右键复制；`Preview` 复制；`Excel` 复制图表
2. 热切换：设置页关闭开关（不重启），复制图片确认零反应；重新开启后恢复出卡
3. `pastebomb`：复制一张超大图片（宽高各超 4096 像素或 `flavor` 字节上限的 `TIFF` 图片），确认“图片过大”失败卡（整体拒绝、不截断、不崩）
4. 缓存：同一张图片复制两次，第二次秒出（`just logs` 可见 `cache` `hit`）
5. 敏感场景闸门：在 `Secure` `Input` 激活的密码框或敏感应用下复制图片，确认不触发

- 更新时间：2026-10-10

### app_survives_eight_hour_mixed_load_soak

- 测试目标：验证长驻进程 8 小时“静置为主加周期性人工划词”混合负载下的内存稳定性、日志有界性与运行健康。
- 测试场景：给定授权真机与 `release` 构建，当应用静置过夜并以约 2 小时间隔人工划词数次，则驻留集（RSS）曲线无持续上漂、`gloss` 日志文件数不超过 7、进程存活且划词出卡正常。
- 测试步骤：

1. 前置：辅助功能已授权（人工划词出卡依赖取材授权）；系统负载低（建议夜间）；磁盘余量不低于 2 吉字节（GB）
2. 构建 `release` 并启动：`cargo build --release` 然后 `./target/release/gloss`
3. 另开终端起驻留集（RSS）采样循环（每 30 分钟一次，照抄）：

   ```sh
   while true; do echo "$(date +%H:%M) rss=$(ps -o rss= -p $(pgrep -x gloss)) kb" >> /tmp/soak-rss.txt; sleep 1800; done
   ```

4. 每隔约 2 小时在任意含文本的应用做数次真实拖选划词，确认出卡正常（人工执行）
5. 8 小时后 `Ctrl-C` 结束采样循环，正常退出应用（菜单、设置页退出）
6. 核对：`cat /tmp/soak-rss.txt` 首尾对比无持续上漂（一次性的启动后爬升属正常）；`ls ~/.gloss/logs/gloss-*.jsonl | wc -l` 不超过 7；`just logs --no-follow --level error` 无异常堆栈（只渲染最新一份日志）
7. 收尾采一次运行时基线归档：`just app-metrics`（趋势点随 `git` 走）

- 更新时间：2026-10-06

### degraded_card_and_retry_manual_check

- 测试目标：真机全链路观察降级产物卡与重试出口（契约校验与重试方案 `L4`：降级卡与重试）。
- 测试场景：给定应用配置指向恒不守约的加密（HTTPS）端点（非 `LLM` 的 `POST` 回声服务返回非 `SSE` 内容，零增量完成恒现），当划词触发任务，则修复轮页脚切“重新注解”（三轮）、耗尽后出“输出异常”卡（不渲染正文残留、带“重试”），点重试后流程重走并再次落降级卡（降级产物不进缓存：重试必须重新走引擎与修复轮）。
- 测试步骤：

1. 前置：`curl -sS -o /dev/null -w "%{http_code}\\n" -X POST https://httpbin.org/post` 返回 200（第三方回声服务，不可达时换任一“`POST` 回 200 且正文无 `data:` 行”的 `HTTPS` 端点）
2. 应用设置：服务地址改 `https://httpbin.org/post`（配置层强制 `HTTPS`，本地 `http` 桩不可用——红线使然）、模型 `ID` 填非空占位、密钥填占位字符串，保存
3. 在任意含文本的前台应用选中一段文字划词：观察流式清稿与页脚“重新注解”交替出现 3 轮，之后出“输出异常”卡（无正文内容、有“重试”按钮）
4. 点“重试”：再次走修复轮并再次落降级卡（若直接显示缓存内容即失败）
5. 收尾：设置里把服务地址与密钥改回原值

- 更新时间：2026-10-12

### iterate_label_accept_closed_loop_with_live_credentials

- 测试目标：验证提示词迭代闭环在真实凭据下走通——迭代实跑、检视页标注导出、标注落库、基线固化、改提示词、再迭代出回归信号（第四层，需真实凭据，不进持续集成）。
- 测试场景：给定 `GLOSS_LIVE_*` 凭据与定稿精选集，当依次执行迭代、检视页标注导出、标注落库、基线固化与提示词改动后的再迭代，则迭代产物四件齐备、标注按条目落库、固化产出基线、再迭代的检视页给出回归或待复核徽标与四维分数、历史曲线出第二轮。
- 测试步骤：

1. 导出凭据：`export GLOSS_LIVE_API_KEY=sk-...`、`export GLOSS_LIVE_BASE_URL=https://api.deepseek.com/v1`、`export GLOSS_LIVE_MODEL=deepseek-flash`（密钥不落日志与任何输出）
2. 首轮小样迭代：`just eval-iterate --limit 4`，核对 `eval-state/` 下 `iterate.json`、`iterate.cases.json`、`iterate.outputs.jsonl`、`iterate.html` 四件齐备
3. 浏览器打开 `eval-state/iterate.html`：核对指标总览、历史曲线、测试用例三区与筛选
4. 标注导出：在检视页给若干用例点合格与不合格、填备注，点“导出标注”下载 `labels.json`
5. 标注落库：`cargo run -p gloss-eval --bin eval -- label --import labels.json`，核对 `eval-state/labels.jsonl` 行数与集外标识警告
6. 基线固化：`cargo run -p gloss-eval --bin eval -- accept`，核对 `eval-state/golden.jsonl` 与未标注警告
7. 改提示词：修改 `crates/gloss-core/prompts/zh/` 任一模板的一处措辞
8. 再迭代：`GLOSS_EVAL_SOURCE=prompt-tweak just eval-iterate`，核对检视页中被改条目出回归或待复核徽标、四维分数与首轮并排可读、`eval-state/history.svg` 出第二轮曲线
9. 收尾（可选）：不带 `--limit` 跑全量一轮，核对三率与历史曲线随全量刷新

- 更新时间：2026-11-11
