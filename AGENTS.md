# AGENTS.md

本文件面向 AI 编码代理。人类贡献者请先读 [README.md](./README.md)。

## 项目

Gloss —— 划词翻译桌面工具：选中文字即弹出 LLM 结果。纯 Rust + WebGPU + egui，单进程桌面应用（macOS 优先，Windows 其次），**不使用任何 WebView / 浏览器运行时**。

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

- **取材链路**：平台事件线程读取选区或截图，产物经 `Event::InputReady` 回到主线程；由主线程组装 `Task` 下发 tokio 推理，因此过期任务的取材产物不会触发推理。
- **四线程**：主线程（winit 事件循环 + UI）/ 平台事件线程（NSRunLoop：热键与取材，有线程亲和性要求）/ 鼠标监听线程（`gloss-mouse-tap`：rdev 全局监听是阻塞式的，且 panic 穿过它的 C 回调会 abort 进程，故单独一条线程收口）/ tokio 后台（网络与缓存）。
- **四通道**：① `PlatformEvent`（事件线程 → 主）② `AcquireCommand`（主 → 事件线程）③ `Command`（主 → tokio，mpsc）④ `Event`（流式回传 → 主）。请求代数 `gen` 一律由 App 赋值，用于丢弃陈旧响应；取消统一走 `CancellationToken`。
- **任务化 AI 层**：`TaskKind`（单词/句子翻译、代码解释、图片 OCR、图片解释）+ `TaskInput`（文本 / 图像，语音为预留模态）→ 统一的 `AiEngine`，不按模态拆分客户端。

## 常用命令

```bash
just install-hooks     # clone 后执行一次，安装本地 git hooks
just run               # 运行开发版
just logs              # 跟随最新日志文件（~/.gloss/logs）
just logs-dir          # 打印日志目录
just precommit         # 提交前静态检查：约束 + 文档引用 + fmt + clippy + 密钥扫描（pre-commit 钩子跑的就是它）
just check             # 全量门禁：precommit 的全部 + test（本地要跑测试时用这条）
just fmt-fix           # 自动格式化
just lint              # Clippy 严格检查（警告即失败）
just secrets           # 硬编码密钥扫描（命中即失败；放行规则见 scripts/check-secrets.sh）
just agents-doc        # 校验本文件提到的仓库事实（配方 / 路径 / 测试目标 / 约束名引用）未漂移
just constraints       # 校验「不可协商的约束」里可机械判定的那几条（依赖方向 / 日志出口 / 版本单点 …）
just audit             # cargo audit 依赖漏洞审计
just deny              # cargo deny 依赖合规（许可证 / 重复依赖 / 来源，配置见 deny.toml）
just changelog         # 基于 conventional commits 生成 CHANGELOG
just --list            # 查看全部 recipe
```

测试与覆盖率命令见「测试」一节；上表没列的 recipe（build / package / clean 等）用 `just --list` 看。

## 不可协商的约束

以下 11 条一律不可协商——按优先级降序排列（定义见下节）只为在取舍冲突时指明先保哪条，级别低不等于可以放松。

1. **[CRITICAL] 纯 Rust 技术栈**：新增依赖前确认它是 Rust 生态的纯逻辑库，不引入 Node / Python / WebView 运行时。
2. **[CRITICAL] 日志统一出口**：只用 `gloss_core::log` 的宏（`info!` / `warn!` / `error!` 等），不用 `println!`；库 crate 不初始化 subscriber。日志目录由入口算好传给 `log::init`，其余实现见 `crates/gloss-core/src/log.rs`。
3. **[CRITICAL] 依赖只开需要的特性**：新增依赖一律写 `default-features = false` 并显式列出所需特性；无特性可关的也照写，保持写法统一。默认集常带目标平台用不到的图形后端（vulkan / gles / webgpu）、wasm 专用项，或整条用不上的子树——既拖慢编译，也可能带进有问题的包（winit 默认集就经 sctk-adwaita 拖进过已停止维护的 `ttf-parser`）。Gloss 目标平台是 macOS（Metal）与 Windows（DX12），Linux 只跑 CI，见 `crates/gloss-app/Cargo.toml` 的写法。
4. **[CRITICAL] unsafe 有据**：每个 unsafe 块前必须带 `// SAFETY:` 注释写明不变量。
5. **[HIGH] 依赖方向**（Ports & Adapters）：
   - `gloss`（根包，bin）→ `gloss-app` + `gloss-core` + `gloss-platform`
   - `gloss-app` → `gloss-core` + `gloss-platform`
   - `gloss-platform` → `gloss-core`（实现其端口）
   - `gloss-core` → **不依赖任何本仓库 crate**，只依赖纯逻辑第三方库。红线：不得出现 winit / wgpu / 平台 API。
6. **[HIGH] 生产路径传播错误**：错误一律用 `Result`/`Option` 传播或降级，不靠 panic 收场（宏清单与门禁见「门禁对照」）。确需绕过时必须在旁边注释文档化不变量（为什么 panic 不可能），并配 `#[allow(clippy::unwrap_used)]` 之类的显式豁免。
7. **[MEDIUM] 组装点唯一**：只有根包入口 `src/main.rs` 把适配器注入端口、分发通道 Sender；其他模块不得持有组装逻辑。
8. **[MEDIUM] 版本单点维护**：`version` / `edition` 写在根 `Cargo.toml` 的 `[workspace.package]`，子 crate 以 `*.workspace = true` 继承，不要硬写。
9. **[LOW] 注释从简**：只写解释「为什么」的必要注释。任务编号可以用来交代某段代码为何处于当前临时状态（如 `M5-T4 前为占位`），但它只是出处，不能代替「为什么」本身；计划式内容（`// TODO: 稍后补 X` 这类没有对应实现的许诺）与冒烟标记不写。
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
| CRITICAL | 无硬编码密钥/凭据 | `just secrets`：`scripts/check-secrets.sh` 扫全部 git 跟踪文件，命中即失败；合法字面量用行尾 `secrets:allow` 放行并注明缘由 |
| CRITICAL | 密钥不进日志/stdout | clippy `print_stdout` / `print_stderr` = deny，堵住绕过日志出口的打印 |
| CRITICAL | 依赖供应链（用成熟库、无已知漏洞、来源可信） | `just audit`（RustSec 已知漏洞）+ `just deny`（许可证 / 来源 / 重复依赖，配置见 deny.toml） |
| CRITICAL | 纯 Rust 技术栈（不嵌入别的语言运行时 / 浏览器引擎） | `just deny`：deny.toml 的 `[bans] deny` 列名禁 JS 引擎、Python 解释器与 WebView 栈，命中即失败 |
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

约束条目里的「组装点唯一」与「注释从简」**不在上表**：它们的判断没有可靠的机械门禁（见 `scripts/check-constraints.sh` 头注释里逐条说明的取舍），靠人工评审，别为它们硬造检查。表里与约束同名的行就是该约束的门禁实现，其余行是评审层面的额外强制项。

### 人工评审关注点（无可靠机械门禁，勿硬造）

- **[CRITICAL] 密钥的运行时处理**：API key 只从配置存储读入内存，不写日志、不进错误消息。
- **[CRITICAL] crypto 与随机数**：一律用成熟库，不手搓算法、不用弱随机源做安全用途。
- **[CRITICAL] unsafe 的设计面**：SAFETY 注释管单块不变量；整段 unsafe 设计是否可避免（如换用安全封装）仍靠评审。
- **[HIGH] 输入校验**：外部输入（选区文本、屏幕截图、配置文件、LLM 响应）进核心逻辑前是否验证边界与格式。
- **[HIGH] 错误处理设计**：错误变体能否支撑状态机分支与重试（见 `GlossError`）；`?` 传播是否恰当，有没有被 `let _ =` 吞掉的错误。
- **[MEDIUM] clone 的语义成本**：跨 `Arc`、大缓冲区（如图像字节）的冗余克隆——clippy 抓不到，靠 review。

## 测试

测试按粒度分五类。源码注释里的 **L1–L4** 是同一套分层的标签（`Cargo.toml`、`kittest.toml` 与各测试模块头注释都在用），调整分层时本节与那些注释要一起改。**E2E 代码不得进入生产路径**——只放 `tests/`、`cfg(test)` 或 `#[ignore]`；测试文件按被测功能域命名，不加 `_test` / `_e2e` 后缀。

### 单元测试

逻辑写在源文件内的 `#[cfg(test)] mod tests`，`just test`（`cargo test --workspace --all-features`）全平台跑。`gloss-core` 是纯逻辑层，应能被完整单测覆盖；新增逻辑优先补单测。

行覆盖率下限单点维护在 justfile 的 `coverage_min`：`just coverage`（本地）与 CI 的 coverage job 共用同一判定，低于即失败。不要用 `--ignore-filename-regex` 排除代码，或写空测试来凑数。

### 集成测试

- **L1 库级集成**（`crates/gloss-app/tests/pipeline.rs`）：公共 API 驱动 状态机 + 通道③④ + tokio 桥 + mock 引擎 的全时序，随 `just test` 全平台跑。
- **L4 真机 opt-in**（不进 CI）：`gloss-platform` 内的 live 测试覆盖真实 OS 边界——选区读取、鼠标·热键注入、keychain 往返，一律 `#[ignore]` 标记，需授权后手动跑：

  ```bash
  cargo test -p gloss-platform -- --ignored
  ```

  授权步骤：系统设置 → 隐私与安全性 → 辅助功能 → 打开运行测试的终端 App（未授权时 `live_test_support` 会毫秒级快速失败并给出指引）。

### 快照测试

- **L2 UI harness**（`crates/gloss-app/src/ui/popup.rs` 模块内的 kittest 测试）：AccessKit 树断言 + 点击复制按钮 + wgpu 渲染快照。基线图在 `crates/gloss-app/tests/snapshots/`，阈值与输出路径在仓库根 `kittest.toml`。树断言与交互断言全平台跑，快照对比仅在 macOS 生效（跨平台渲染差异）。

### 基准测试

当前没有基准目标，也没有引入基准框架。需要量化性能时新增 criterion 目标，**不要**把计时断言塞进 `#[test]`——CI 的负载波动会让它变成 flaky 门禁。

### 性能测试

- **L3 显隐自检**（根包 `tests/overlay_selftest.rs`，`harness = false` 自带 `main()`）：经公共 API 驱动与生产相同的窗口栈跑 100 轮浮层显隐，跑满 100 轮且首帧在预算内才返回 0，无帧或首帧超预算则非 0 退出。仅 macOS（需窗口服务 + GPU），其余平台直通成功：

  ```bash
  just selftest
  ```
