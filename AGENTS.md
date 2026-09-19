# AGENTS.md

本文件面向 AI 编码代理。人类贡献者请先读 [README.md](./README.md)。

## 项目

Gloss —— 划词翻译桌面工具：选中文字即弹出 LLM 结果。纯 Rust + WebGPU + egui，单进程桌面应用，**不使用任何 WebView / 浏览器运行时**。

## 目录结构

```
gloss/
├── Cargo.toml           # workspace 根 + 根包 gloss（bin gloss，唯一入口 src/main.rs）
├── src/main.rs          # 唯一入口 / 唯一组装点
└── crates/
    ├── gloss-core/      # 领域层 + 端口：模型、任务、提示词、引擎、缓存、配置、日志（零平台依赖）
    ├── gloss-platform/  # 适配器层：实现 core 的端口——选区读取、热键与鼠标事件源、配置与密钥存储
    └── gloss-app/       # 表现层 + 应用层：状态机、窗口、wgpu 与 egui、通道类型、tokio 消费桥
```

具体文件清单以 `crates/*/src` 为准，本节只讲分层职责——文件名会随重构漂移，职责不会。

## 架构速览

- **四线程**：主线程（winit 事件循环 + UI）、平台事件线程（NSRunLoop：热键与取材，有线程亲和性要求）、鼠标监听线程（`gloss-mouse-tap`：全局事件 tap 在此收口）、tokio 后台（网络与缓存）。鼠标 tap 的回调是阻塞式的，panic 穿过其 C 回调会 abort 进程；**CGEventTap 只订阅左键按下/释放**——把按键翻成字符要调主线程专属的 TSM/HIToolbox，回调跑在监听线程上会以 SIGILL 打死进程，订阅面必须窄。
- **四通道**：① `PlatformEvent`（事件线程 → 主）② `AcquireCommand`（主 → 事件线程）③ `Command`（主 → tokio）④ `Event`（流式回传 → 主）。代数 `gen` 由 App 赋值以丢弃陈旧响应；取消统一走 `CancellationToken`。
- **任务化 AI 层**：`TaskKind` + `TaskInput` → 统一的 `AiEngine`，不按模态拆分客户端。

## 常用命令

```bash
just install-hooks     # 安装本地 git hooks（clone 后运行一次）
just setup             # clone 后一键环境初始化：工具链校验 + 可选工具清点 + git hooks（幂等）
just run               # 运行开发版（debug）
just logs              # 跟随最新日志文件（Ctrl-C 退出）
just precommit         # 提交前门禁：约束 + 文档引用 + fmt + TOML + clippy + 密钥扫描
just check             # 完整质量门禁：precommit 的全部 + test
just test              # 运行全部测试
just fmt-fix           # 自动格式化
just lint              # Clippy 严格检查（警告即失败）
just secrets           # 硬编码密钥扫描（命中即失败）
just agents-doc        # 校验 AGENTS.md 引用的配方/路径/测试目标/约束名真实存在
just constraints       # 「不可协商的约束」的机械门禁（依赖方向 / 日志 / 版本单点 / 依赖特性）
just icons             # 重建应用图标产物（.icns 与 Dock 图标 PNG）
just --list            # 查看全部配方
```

## 不可协商的约束

以下 11 条一律不可协商——按优先级降序排列（定义见下节）只为在取舍冲突时指明先保哪条，级别低不等于可以放松。

1. **[CRITICAL] 纯 Rust 技术栈**：新增依赖前确认它是 Rust 生态的纯逻辑库，不引入 Node / Python / WebView 运行时。
2. **[CRITICAL] 日志统一出口**：只用 `gloss_core::log` 的宏（`info!` / `warn!` / `error!` 等），不用 `println!`；库 crate 不初始化 subscriber。日志目录由入口算好传给 `log::init`，其余实现见 `crates/gloss-core/src/log.rs`。
3. **[CRITICAL] 依赖只开需要的特性**：新增依赖一律写 `default-features = false` 并显式列出所需特性；无特性可关的也照写，保持写法统一。默认集常带用不到的图形后端（vulkan / gles / webgpu）、wasm 专用项，或整条用不上的子树——既拖慢编译，也可能带进有问题的包（winit 默认集就经 sctk-adwaita 拖进过已停止维护的 `ttf-parser`）。Gloss 渲染后端是 Metal，见 `crates/gloss-app/Cargo.toml` 的写法。
4. **[CRITICAL] unsafe 有据**：每个 unsafe 块前必须带 `// SAFETY:` 注释写明不变量。
5. **[HIGH] 依赖方向**（Ports & Adapters）：
   - `gloss`（根包，bin）→ `gloss-app` + `gloss-core` + `gloss-platform`
   - `gloss-app` → `gloss-core` + `gloss-platform`
   - `gloss-platform` → `gloss-core`（实现其端口）
   - `gloss-core` → **不依赖任何本仓库 crate**，只依赖纯逻辑第三方库。红线：不得出现 winit / wgpu / 平台 API。
6. **[HIGH] 生产路径传播错误**：错误一律用 `Result`/`Option` 传播或降级，不靠 panic 收场（宏清单与门禁见「门禁对照」）。确需绕过时必须在旁边注释文档化不变量（为什么 panic 不可能），并配 `#[allow(clippy::unwrap_used)]` 之类的显式豁免。
7. **[MEDIUM] 组装点唯一**：只有根包入口 `src/main.rs` 把适配器注入端口、分发通道 Sender；其他模块不得持有组装逻辑。
8. **[MEDIUM] 版本单点维护**：`version` / `edition` 写在根 `Cargo.toml` 的 `[workspace.package]`，子 crate 以 `*.workspace = true` 继承，不要硬写。
9. **[LOW] 注释纪律**：代码内注释基于代码逻辑，准确描述代码的行为——这段代码在做什么、契约与单位、边界条件；「为什么」（决策、备选、历史、取舍）一律外置到本地 docs/comments/ 目录。
   - **考虑长期维护价值**：注释与外置记录都是写给未来维护者（包括未来的自己）的——只写能降低未来理解与修改成本的；一条注释该不该写、该留在代码还是外置，都以此判断。
   - **描述行为，不解释动机**：注释跟着代码逻辑走，说的都是「这段代码做什么」；「为什么这样写」从命名与代码里读不出来，属于外置记录。SAFETY 除外——必须紧贴 unsafe 块、写明单块不变量，永不外置。
   - **「为什么」外置**：决策过程、备选方案对比、任务/PR/评审引入的来龙去脉，写进 `docs/comments/<文件名>_comment.md`，按符号锚定（不写行号），改到相关代码时同步更新；mod.rs 同名冲突带父目录前缀。docs/ 整目录本地 gitignore，被跟踪文件（含代码）不留指向外置决策记录的指针，记录仅本地可见。
   - **其它文件同一原则**：justfile、Cargo.toml 与其余 TOML/YAML 配置、shell 脚本、workflow 等非 Rust 文件的注释同样只写行为与用法；决策/历史/取舍外置到 `docs/comments/<文件名>_comment.md`，命名与锚定规则同上。
   - **测试代码禁止注释**：`#[cfg(test)]` 模块与 `tests/` 目录里一律不写注释（`// SAFETY:` 除外，同上）；测试的验收标准、形态说明与辅助契约外置到 `docs/comments/<文件名>_tests_comment.md`，锚定与同步规则同上，文件顶部维护一份测试行为速览——每条测试按 BDD 顺序（给定/当/则）一句话描述，新增或修改测试时同步登记。test-util 特性模块按生产代码对待，不在此列。
10. **[LOW] 日志一律英文**：日志消息、字段值、span 名只用英文——日志是面向终端的诊断文本，不做本地化；中文只出现在注释、文档与用户可见文案里。
11. **[LOW] 公共 API 有文档注释**：公共 API 必须有文档注释，文档里的示例代码由 doc test 验证可编译可运行。

## 代码评审原则与门禁对照

评审（人与 AI）按下表执行：**能机械化的条目已全部落入门禁**——本地 `just precommit` 与 CI 的 quality job 跑同一套配方，规则一律由 workspace lints / 脚本强制，不靠口头约定。发现可机械化的新检查项时，优先补门禁而不是写进评审清单。

### 优先级

- **CRITICAL**：安全漏洞、内存安全问题、数据泄漏
- **HIGH**：逻辑错误、错误处理不当、API 误用
- **MEDIUM**：代码质量、性能隐患、非惯用 Rust
- **LOW**：风格问题、文档改进、小型重构

下面两个清单按优先级降序排列，每条的定级理由就是上面四个定义；新增条目按同一规则插到对应位置，不要按加入时间追加在末尾。

### 门禁对照（机械强制）

| 优先级 | 原则条目 | 门禁 |
| --- | --- | --- |
| CRITICAL | unsafe 有据 | clippy `undocumented_unsafe_blocks` = deny；edition 2024 下 `unsafe_op_in_unsafe_fn` 默认报警，被 `-D warnings` 兜底 |
| CRITICAL | 内存泄漏 | clippy `mem_forget` = deny（长驻进程禁 `mem::forget` 式泄漏） |
| CRITICAL | 借用与生命周期健全性 | rustc 类型系统在编译期拒绝（`just lint` / `just check` 即覆盖） |
| CRITICAL | 无硬编码密钥/凭据 | `just secrets`：`scripts/hooks/check-secrets.sh` 扫全部 git 跟踪文件，命中即失败；合法字面量用行尾 `secrets:allow` 放行并注明缘由 |
| CRITICAL | 密钥不进日志/stdout | clippy `print_stdout` / `print_stderr` = deny，堵住绕过日志出口的打印 |
| CRITICAL | 依赖供应链（用成熟库、无已知漏洞、来源可信） | `just audit`（RustSec 已知漏洞）+ `just deny`（许可证 / 来源 / 重复依赖，配置见 deny.toml） |
| CRITICAL | 日志统一出口 | `just constraints`：只有 gloss-core 可以直接依赖 tracing 三件套，其余 crate 只经 `gloss_core::log` |
| CRITICAL | 依赖只开需要的特性 | `just constraints`：每条第三方依赖声明必须带 `default-features = false` |
| HIGH | panic 家族禁入生产路径 | clippy `unwrap_used` / `expect_used` / `panic` / `unreachable` / `todo` / `unimplemented` = deny（测试经 clippy.toml 放行） |
| HIGH | 依赖方向 | `just constraints`：各 crate 的直接依赖必须落在允许的边上（新 crate 要在脚本里登记），gloss-core 不得出现平台 / 渲染栈 |
| MEDIUM | 惯用 Rust / 类型安全 / 明显冗余 clone | clippy 全量集合（correctness + style + complexity + perf）`-D warnings`，如 `clone_on_copy` |
| MEDIUM | 关键路径有测试 | `just coverage` 行覆盖率下限判定（`coverage_min`） |
| MEDIUM | 版本单点维护 | `just constraints`：子 crate 的 `version` / `edition` 必须 `*.workspace = true`，字面量只允许在根 `[workspace.package]` |
| LOW | 公共 API 有文档注释 | rustc `missing_docs` = deny；文档示例由 `just test` 里的 doc test 验证 |
| LOW | 格式一致 | `just fmt`（rustfmt） |
| LOW | 日志一律英文 | `just constraints`：日志宏实参不得含非 ASCII 字节（日志面向终端诊断，不做本地化） |
| LOW | 本文件描述的仓库事实不漂移 | `just agents-doc`：校验本文件提到的每个 `just` 配方、仓库路径与测试目标真实存在，并核对仓库各处对约束的指名引用（引用一律写成 `AGENTS.md 约束「名字」`——条号会随重排失效） |

约束条目里的「纯 Rust 技术栈」「组装点唯一」与「注释纪律」**不在上表**：它们的判断没有可靠的机械门禁（纯 Rust 技术栈只作口头约束——这类判断发生在「要不要引入这个新依赖」的评审现场，枚举包名的 deny 名单覆盖不了没见过的运行时；注释纪律要判的是「有没有长期维护价值、该外置的「为什么」有没有外置」，不是措辞），靠人工评审，别为它们硬造检查。表里与约束同名的行就是该约束的门禁实现，其余行是评审层面的额外强制项。

### 人工评审关注点（无可靠机械门禁，勿硬造）

- **[CRITICAL] 密钥的运行时处理**：API key 只从配置存储读入内存，不写日志、不进错误消息。
- **[CRITICAL] crypto 与随机数**：一律用成熟库，不手搓算法、不用弱随机源做安全用途。
- **[CRITICAL] unsafe 的设计面**：SAFETY 注释管单块不变量；整段 unsafe 设计是否可避免（如换用安全封装）仍靠评审。
- **[HIGH] 输入校验**：外部输入（选区文本、屏幕截图、配置文件、LLM 响应）进核心逻辑前是否验证边界与格式。
- **[HIGH] 错误处理设计**：错误变体能否支撑状态机分支与重试（见 `GlossError`）；`?` 传播是否恰当，有没有被 `let _ =` 吞掉的错误。
- **[MEDIUM] clone 的语义成本**：跨 `Arc`、大缓冲区（如图像字节）的冗余克隆——clippy 抓不到，靠 review。

## 测试

两条原则，其余都是推论：

1. **断言可观察契约，不碰内部状态。** 测试红时要说明「行为变了」，而不是「实现重构了」——所以测试从公共 API 与通道两端驱动，不伸手去摸结构体字段或私有函数。
2. **同步点用事件，不用时间。** 任何 `sleep` 都是在赌机器负载。

选层先问一句：**这条断言需要什么条件才成立？** 纯逻辑留在单元测试；需要 egui 渲染就归 L2；需要窗口服务与 GPU 归 L3；需要真实辅助功能授权归 L4。不要为了少写一个测试替身把测试塞进更贵的层，也不要把需要真机的测试硬塞进 CI。源码注释里的 **L1–L4** 就是这套分层（`Cargo.toml`、`kittest.toml` 与各测试模块都在用），调层要一起改。

### 单元测试

逻辑写在源文件内的 `#[cfg(test)] mod tests`，只断言公开契约。`gloss-core` 是纯逻辑层，应能被完整单测覆盖，新增逻辑优先在这一层补测试。

端口与编排的替身用 `test-util` 特性提供的实现，别在测试里手搓 fake：`MockEngine` 可注入产出内容、chunk 间延迟、一次性或持续失败，并可读 `call_count` 观测调用次数——**注入点要能造出想测的那种时序，观测点要能证明它发生过**，只回固定值的 fake 测不出取消、重试、去重这些真正会坏的路径。

行覆盖率下限单点维护在 justfile 的 `coverage_min`，`just coverage`（本地）与 CI 的 coverage job 共用同一判定。**下限是下限，不是目标**：不要用 `--ignore-filename-regex` 排除代码，也不要写「调一次、不断言」的空测试凑数——那只是把红线画到别处。

### 集成测试

- **L1 库级集成**（`crates/gloss-app/tests/pipeline.rs`）：从公共 API 与通道两端驱动 状态机 + 通道③④ + tokio 桥 + mock 引擎 的全时序，随 `just test` 跑。断言的是时序与状态转换（哪些事件该按什么顺序到达、哪些不该到达），不是内部字段的值。
- **L4 真机 opt-in**（不进 CI）：`gloss-platform` 内的 live 测试覆盖真实 OS 边界——选区读取、鼠标·热键注入、keychain 往返。形态是 `#[cfg(test)] mod live_tests` + `#[ignore = "..."]`，忽略理由里写清授权步骤；测试第一行调 `live_test_support::require_accessibility()` 做前置检查，未授权时以**带修复指引**的消息立刻失败，而不是让测试以「注入被忽略 / tap 未建立」这类间接症状超时——前置条件不满足就要当场说清怎么修。

  ```bash
  cargo test -p gloss-platform -- --ignored
  ```

  授权步骤：系统设置 → 隐私与安全性 → 辅助功能 → 打开运行测试的终端 App。

- **L4 环境变量型 opt-in**（同样不进 CI）：需要外部凭据的真机测试走这一形态——目前只有 LLM 端点（`gloss-platform::engine::llm` 的 live 测试）。它不碰 OS 授权，因此不调 `require_accessibility()`，而是自己在前置检查里读 `GLOSS_LIVE_API_KEY` / `GLOSS_LIVE_BASE_URL` / `GLOSS_LIVE_MODEL`，缺项时用**可直接照抄的命令**当场失败（缺哪个变量、怎么跑都写在消息里），密钥不打印。注意 `cargo test -- --ignored` 会把两种形态一起跑起来，撞上缺环境变量的报错时按消息补齐即可，不必以为哪里坏了。

### 快照测试

- **L2 UI harness**（`crates/gloss-app/src/ui/popup.rs` 模块内的 kittest 测试）：AccessKit 树断言 + 点击复制按钮 + wgpu 渲染快照。多个 harness 的快照结果要合并进同一个 `SnapshotResults`。

基线图提交在 `crates/gloss-app/tests/snapshots/`，所以**基线变化会出现在 PR diff 里**——更新基线时要说清为什么变了，别默默覆盖（阈值与输出路径在仓库根 `kittest.toml`，分平台节名是 `[mac]`，写错整轮测试编译不过）。更新失败基线用 `UPDATE_SNAPSHOTS=1 cargo test`（`=force` 连阈值内的差异也重写）：更新当轮即转绿并打印 `Updated snapshot: …`，跑完 `git diff` 看一眼基线图到底改了什么。对比产生的 `.diff.png` / `.new.png` / `.old.png` 已 gitignore，别提交。

快照锁的是**布局回归**，不是正确性：断言文案与状态该用 AccessKit 树断言，别指望从像素里读出语义。

### 基准测试

当前没有基准目标，也没有引入基准框架。要量化性能时新增 criterion 目标（放 benches 目录），**不要**把计时断言塞进 `#[test]`——CI 的负载波动会让它变成 flaky 门禁。基准数字要连同运行环境一起记录，CI 上只跑不判定趋势。

### 性能测试

- **L3 显隐自检**（根包 `tests/overlay_selftest.rs`，`harness = false` 自带 `main()`）：经公共 API 驱动与生产相同的窗口栈跑 100 轮浮层显隐，跑满 100 轮且首帧在预算内才返回 0，无帧或首帧超预算则非 0 退出。需要窗口服务与 GPU：

  ```bash
  just selftest
  ```

### 跨层纪律

- **测试替身放端口边界**：真实时钟、网络、磁盘、剪贴板、keychain 都不进常规测试，需要时用端口替身或注入延迟。
- **clippy 在测试代码里的三种情形**（别照搬网上的豁免写法）：
  - `clippy.toml` 已对 `#[test]` / `#[cfg(test)]` 自动放行 unwrap / expect / panic / print——测试正文里直接断言失败即可；
  - `tests/` 下的集成测试**辅助函数**不在自动放行范围，要显式 `#[allow(clippy::expect_used, clippy::panic)]`——缘由（测试辅助：失败即 panic 是断言语义）不写代码注释（测试代码禁止注释），记入对应文件的 tests 外置记录（见 `crates/gloss-app/tests/pipeline.rs`）；
  - `harness = false` 的目标**完全不豁免**（既无 `#[test]` 也非 `cfg(test)`，已实测 `expect` 会被 `expect_used` 拦下）：代码要写成无 panic，输出走 `gloss_core::log`，失败靠返回非 0 退出码表达。
- **命名与放置**：测试文件按被测功能域命名，不加 `_test` / `_e2e` 后缀。**E2E 代码不得进入生产路径**——只放 `tests/`、`cfg(test)` 或 `#[ignore]`。
