# AGENT.md

本文件面向 AI 编码代理。人类贡献者请先读 [README.md](./README.md)。

## 项目

Gloss —— 划词翻译桌面工具：选中文字即弹出 LLM 结果。纯 Rust + WebGPU + egui，单进程桌面应用（macOS 优先，Windows 其次），**不使用任何 WebView / 浏览器运行时**。

## 不可协商的约束

1. **纯 Rust 技术栈**：新增依赖前确认它是 Rust 生态的纯逻辑库，不引入 Node / Python / WebView 运行时。
2. **依赖方向**（Ports & Adapters，可机械验证）：
   - `gloss`（根包，bin）→ `gloss-app` + `gloss-core` + `gloss-platform`
   - `gloss-app` → `gloss-core` + `gloss-platform`
   - `gloss-platform` → `gloss-core`（实现其端口）
   - `gloss-core` → **不依赖任何本仓库 crate**，只依赖纯逻辑第三方库。红线：不得出现 winit / wgpu / 平台 API。
3. **组装点唯一**：只有根包入口 `src/main.rs` 把适配器注入端口、分发通道 Sender；其他模块不得持有组装逻辑。
4. **注释从简**：只写解释「为什么」的必要注释。不写任务编号、规划性说明、冒烟标记等临时内容。
5. **日志统一出口**：只用 `gloss_core::log` 的宏（`info!` / `warn!` / `error!` 等）；库 crate 不初始化 subscriber，不用 `println!`（clippy `print_stdout` / `print_stderr` = deny，机械强制）。日志同时落盘到 `~/.gloss/logs/`（按天滚动，留 7 份），目录由入口算好传给 `log::init`。
6. **日志一律英文**：日志消息、字段值、span 名只用英文——日志是面向终端的诊断文本，不做本地化；中文只出现在注释、文档与用户可见文案里。
7. **版本单点维护**：`version` / `edition` 写在根 `Cargo.toml` 的 `[workspace.package]`，子 crate 以 `*.workspace = true` 继承，不要硬写。
8. **依赖只开需要的特性**：新增依赖一律写 `default-features = false` 并显式列出所需特性。默认集常带目标平台用不到的图形后端（vulkan / gles / webgpu）、wasm 专用项，或整条用不上的子树——既拖慢编译，也可能带进有问题的包（winit 默认集就经 sctk-adwaita 拖进过已停止维护的 `ttf-parser`）。Gloss 目标平台是 macOS（Metal）与 Windows（DX12），Linux 只跑 CI，见 `crates/gloss-app/Cargo.toml` 的写法。
9. **生产路径传播错误**：`unwrap()` / `expect()` / `panic!()` 禁止出现在生产代码（clippy `unwrap_used` / `expect_used` / `panic` = deny，`unreachable!` / `todo!` / `unimplemented!` 同禁，由 workspace lints 机械强制；测试代码经 clippy.toml 放行）。错误一律用 `Result`/`Option` 传播或降级；确需绕过时必须在旁边注释文档化不变量（为什么 panic 不可能），并配 `#[allow(clippy::unwrap_used)]` 之类的显式豁免。
10. **unsafe 有据、API 有文档**：每个 unsafe 块前必须带 `// SAFETY:` 注释写明不变量（clippy `undocumented_unsafe_blocks` = deny）；公共 API 必须有文档注释（rustc `missing_docs` = deny），文档里的示例代码由 doc test 验证可编译可运行。

## 代码评审原则与门禁对照

评审（人与 AI）按下表执行：**能机械化的条目已全部落入门禁**——本地 `just precommit` 与 CI 的 quality job 跑同一套配方，规则一律由 workspace lints / 脚本强制，不靠口头约定。发现可机械化的新检查项时，优先补门禁而不是写进评审清单。

### 优先级

- **CRITICAL**：安全漏洞、内存安全问题、数据泄漏
- **HIGH**：逻辑错误、错误处理不当、API 误用
- **MEDIUM**：代码质量、性能隐患、非惯用 Rust
- **LOW**：风格问题、文档改进、小型重构

### 门禁对照（机械强制）

| 原则条目 | 门禁 |
| --- | --- |
| panic 家族禁入生产路径 | clippy `unwrap_used` / `expect_used` / `panic` / `unreachable` / `todo` / `unimplemented` = deny（测试经 clippy.toml 放行，见「不可协商的约束」第 9 条） |
| 公共 API 有文档注释 | rustc `missing_docs` = deny；文档示例由 `just test` 里的 doc test 验证 |
| unsafe 最小化且有据 | clippy `undocumented_unsafe_blocks` = deny；edition 2024 下 `unsafe_op_in_unsafe_fn` 默认报警，被 `-D warnings` 兜底 |
| 内存泄漏 | clippy `mem_forget` = deny（长驻进程禁 `mem::forget` 式泄漏） |
| 无硬编码密钥/凭据 | `just secrets`：`scripts/check-secrets.sh` 扫全部 git 跟踪文件，命中即失败；合法字面量用行尾 `secrets:allow` 放行并注明缘由 |
| 密钥不进日志/stdout | clippy `print_stdout` / `print_stderr` = deny（堵住绕过日志出口的打印，见「不可协商的约束」第 5 条） |
| 依赖供应链（用成熟库、无已知漏洞、来源可信） | `just audit`（RustSec 已知漏洞）+ `just deny`（许可证 / 来源 / 重复依赖，配置见 deny.toml） |
| 惯用 Rust / 类型安全 / 明显冗余 clone | clippy 全量集合（correctness + style + complexity + perf）`-D warnings`，如 `clone_on_copy` |
| 借用与生命周期健全性 | rustc 类型系统在编译期拒绝（`just lint` / `just check` 即覆盖） |
| 格式一致 | `just fmt`（rustfmt） |
| 关键路径有测试 | `just coverage` 行覆盖率下限判定（`coverage_min`） |

### 人工评审关注点（无可靠机械门禁，勿硬造）

- **输入校验**：外部输入（选区文本、屏幕截图、配置文件、LLM 响应）进核心逻辑前是否验证边界与格式。
- **错误处理设计**：错误变体能否支撑状态机分支与重试（见 `GlossError`）；`?` 传播是否恰当，有没有被 `let _ =` 吞掉的错误。
- **密钥的运行时处理**：API key 只从配置存储读入内存，不写日志（日志只落英文诊断文本，见约束第 6 条）、不进错误消息。
- **crypto 与随机数**：一律用成熟库，不手搓算法、不用弱随机源做安全用途。
- **clone 的语义成本**：跨 `Arc`、大缓冲区（如图像字节）的冗余克隆——clippy 抓不到，靠 review。
- **unsafe 的设计面**：SAFETY 注释管单块不变量；整段 unsafe 设计是否可避免（如换用安全封装）仍靠评审。

## 目录结构

```
gloss/
├── Cargo.toml           # workspace 根 + 根包 gloss（bin gloss，唯一入口 src/main.rs）
├── src/main.rs          # 唯一入口 / 唯一组装点
└── crates/
    ├── gloss-core/      # 领域层 + 端口（零平台依赖）：model / task / ports / prompt / engine / cache / config / pipeline / log
    ├── gloss-platform/  # 适配器层：选区读取、区域截图、热键·鼠标事件源、LLM 客户端、配置存储
    └── gloss-app/       # 表现层 + 应用层：App 状态机、窗口管理、wgpu 渲染、egui UI、通道类型
```

## 架构速览

- **取材链路**：平台事件线程读取选区或截图，产物经 `Event::InputReady` 回到主线程；由主线程组装 `Task` 下发 tokio 推理，因此过期任务的取材产物不会触发推理。
- **三线程**：主线程（winit 事件循环 + UI）/ 平台事件线程（NSRunLoop，热键·鼠标·取材，有线程亲和性要求）/ tokio 后台（网络与缓存）。
- **四通道**：① `PlatformEvent`（事件线程 → 主）② `AcquireCommand`（主 → 事件线程）③ `Command`（主 → tokio，mpsc）④ `Event`（流式回传 → 主）。请求代数 `gen` 一律由 App 赋值，用于丢弃陈旧响应；取消统一走 `CancellationToken`。
- **任务化 AI 层**：`TaskKind`（单词/句子翻译、代码解释、图片 OCR、图片解释…）+ `TaskInput`（文本 / 图像，预留语音）→ 统一的 `AiEngine`，不按模态拆分客户端。

## 常用命令

```bash
just install-hooks     # clone 后执行一次，安装本地 git hooks
just run               # 运行开发版
just logs              # 跟随最新日志文件（~/.gloss/logs）
just logs-dir          # 打印日志目录
cargo run -- --overlay-selftest  # M1 验收入口：100 轮浮层显隐自检（首帧延迟/句柄泄漏），需图形环境
just precommit         # 提交前静态检查：fmt + clippy + 密钥扫描（pre-commit 钩子跑的就是它）
just check             # 全量门禁：fmt + clippy + test + 密钥扫描（测试由 CI 兜底，本地按需）
just fmt-fix           # 自动格式化
just lint              # Clippy 严格检查（警告即失败）
just secrets           # 硬编码密钥扫描（命中即失败；放行规则见 scripts/check-secrets.sh）
just test              # 运行单元测试
just coverage          # 测试覆盖率：终端摘要 + HTML（→ target/llvm-cov/html）；行覆盖率 < 70% 即失败
just audit             # cargo audit 依赖漏洞审计
just deny              # cargo deny 依赖合规（许可证 / 重复依赖 / 来源，配置见 deny.toml）
just changelog         # 基于 conventional commits 生成 CHANGELOG
just --list            # 查看全部 recipe
```

## 工作流

- **分支**：一任务一分支，命名 `feat/<主题>`，合入用 squash。
- **提交信息**：Conventional Commits；**标题 ≤ 72 字节**（本地 commitlint 会拒绝超长标题），正文说明「为什么」。
- **门禁**：本地 `pre-commit` 只跑 `just precommit`（fmt + clippy + secrets；命令一律带 `--workspace`——根目录存在根包时，不加则只作用于根包、漏掉成员 crate），**测试交给 CI**，本地提交不必等编译测试。`just check`（fmt + clippy + test + secrets）保留给需要本地全量验证的场合。**不要用 `--no-verify` 绕过**。commit message 规范不在本地校验（git 跑 pre-commit 时消息还没落盘，读到的会是上一条），由 CI 的 commitlint job 兜底。
- **CI**（ci.yml）：`quality`（fmt + clippy + secrets）最快，`test`（Linux/macOS/Windows 三平台矩阵）、`coverage`（行覆盖率 ≥ 70%，报告进 job summary 与 artifact）、`security`（cargo audit + deny）三个 job 都 `needs: quality`；另有每日定时安全检查（security-audit.yml）。
- **推送与 PR**：不要自行推送或开 PR，等用户明确要求。
- **测试**：新增逻辑优先补单测；**行覆盖率下限 70%**（justfile 的 `coverage_min`，本地 `just coverage` 与 CI 同一判定），低于即失败；不要用 `--ignore-filename-regex` 排除代码或写空测试来凑数。`gloss-core` 是纯逻辑层，应能被完整单测覆盖。
