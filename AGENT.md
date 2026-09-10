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
5. **日志统一出口**：只用 `gloss_core::log` 的宏（`info!` / `warn!` / `error!` 等）；库 crate 不初始化 subscriber，不用 `println!`。日志同时落盘到 `~/.gloss/logs/`（按天滚动，留 7 份），目录由入口算好传给 `log::init`。
6. **日志一律英文**：日志消息、字段值、span 名只用英文——日志是面向终端的诊断文本，不做本地化；中文只出现在注释、文档与用户可见文案里。
7. **版本单点维护**：`version` / `edition` 写在根 `Cargo.toml` 的 `[workspace.package]`，子 crate 以 `*.workspace = true` 继承，不要硬写。
8. **依赖只开需要的特性**：新增依赖一律写 `default-features = false` 并显式列出所需特性。默认集常带目标平台用不到的图形后端（vulkan / gles / webgpu）、wasm 专用项，或整条用不上的子树——既拖慢编译，也可能带进有问题的包（winit 默认集就经 sctk-adwaita 拖进过已停止维护的 `ttf-parser`）。Gloss 目标平台是 macOS（Metal）与 Windows（DX12），Linux 只跑 CI，见 `crates/gloss-app/Cargo.toml` 的写法。

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
just precommit         # 提交前静态检查：fmt + clippy（pre-commit 钩子跑的就是它）
just check             # 全量门禁：fmt + clippy + test（测试由 CI 兜底，本地按需）
just fmt-fix           # 自动格式化
just lint              # Clippy 严格检查（警告即失败）
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
- **门禁**：本地 `pre-commit` 只跑 `just precommit`（fmt + clippy；命令一律带 `--workspace`——根目录存在根包时，不加则只作用于根包、漏掉成员 crate），**测试交给 CI**，本地提交不必等编译测试。`just check`（fmt + clippy + test）保留给需要本地全量验证的场合。**不要用 `--no-verify` 绕过**。commit message 规范不在本地校验（git 跑 pre-commit 时消息还没落盘，读到的会是上一条），由 CI 的 commitlint job 兜底。
- **CI**（ci.yml）：`quality`（fmt + clippy）最快，`test`（Linux/macOS/Windows 三平台矩阵）、`coverage`（行覆盖率 ≥ 70%，报告进 job summary 与 artifact）、`security`（cargo audit + deny）三个 job 都 `needs: quality`；另有每日定时安全检查（security-audit.yml）。
- **推送与 PR**：不要自行推送或开 PR，等用户明确要求。
- **测试**：新增逻辑优先补单测；**行覆盖率下限 70%**（justfile 的 `coverage_min`，本地 `just coverage` 与 CI 同一判定），低于即失败；不要用 `--ignore-filename-regex` 排除代码或写空测试来凑数。`gloss-core` 是纯逻辑层，应能被完整单测覆盖。
