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
    ├── gloss-core/      # 领域层 + 端口（ports：core 里定义的 trait 契约）：模型、任务、提示词、引擎、缓存、配置、日志（零平台依赖）
    ├── gloss-platform/  # 适配器层：实现 core 的端口——选区读取、热键与鼠标事件源、配置与密钥存储
    └── gloss-app/       # 表现层 + 应用层：状态机、窗口、wgpu 与 egui、通道类型、tokio 消费桥
```

具体文件清单以 `crates/*/src` 为准，本节只讲分层职责——文件名会随重构漂移，职责不会。

## 架构速览

- **四线程**：主线程（winit 事件循环 + UI）、平台事件线程（NSRunLoop：热键与取材，有线程亲和性要求）、鼠标监听线程（`gloss-mouse-tap`：全局事件 tap 在此收口）、tokio 后台（网络与缓存）。平台回调的订阅面必须最小（鼠标 tap 只订阅左键按下/释放）；回调里只做事件搬运——读事件字段、把事件投进通道（满则丢弃），不调用要求主线程的 API；panic 穿过 C 回调会 abort 进程。
- **四通道**：① `PlatformEvent`（事件线程 → 主）② `AcquireCommand`（主 → 事件线程）③ `Command`（主 → tokio）④ `Event`（流式回传 → 主）。请求代数（字段名 `generation`）由 App 赋值、随消息贯穿各通道，主线程据此丢弃陈旧响应；取消统一走 `CancellationToken`。
- **任务化 AI 层**：`TaskKind` + `TaskInput` → 统一的 `AiEngine`，不按输入模态（文本/图像/音频）拆分客户端。

## 常用命令

```bash
just setup             # 环境初始化（clone 后一次，幂等）
just run               # 运行开发版
just logs              # 跟随最新日志
just lint              # Clippy 严格检查
just fmt-fix           # 自动格式化
just test              # 运行全部测试
just bench             # 跑 gloss-core 热点基准（criterion，benches/core.rs）
just precommit         # 提交前门禁（git hook 自动跑）
just check             # 完整质量门禁（precommit + test）
just --list            # 全部配方与说明
```

## 质量条目与门禁对照

评审（人与 AI）按本节执行：**能自动化的条目已全部落入门禁**——规则一律由 workspace lints 与脚本强制，不靠口头约定；本地 `just precommit` 与 CI 共用同一批脚本，CI 另跑全量测试、覆盖率、Miri、Valgrind 与依赖审计等较慢的门禁。发现可自动化的新检查项时，优先补门禁而不是写进评审清单。

### 定级原则与优先级

- **CRITICAL**：失守即不可逆或不可察觉的损害——安全漏洞、内存安全（UB）、数据泄漏、供应链风险。
- **HIGH**：造成可观察的功能故障或显著返工——逻辑错误、错误处理不当、边界情况。
- **LOW**：不影响正确性，只影响长期成本——注释、文档。

维度序列（同级别内稳定排序）：正确性 → 安全性 → 健壮性 → 高效性 → 可维护性 → 可读性。

### 质量条目总表

被仓库其它文件指名的条目以粗体短名起始；跨文件引用一律写成 `AGENTS.md 质量条目「名字」`，由 `just agents-doc` 校验一致性。

| 维度 | 级别 | 条目 | 强制方式 |
| --- | --- | --- | --- |
| 正确性 | CRITICAL | 借用与生命周期健全性，不绕过类型系统检查 | `just check` |
| 正确性 | CRITICAL | 并发代码无数据竞争与悬垂，UB 零容忍 | `just miri`（仅覆盖 gloss-core，跳过 cache、engine 模块）+ 人工评审 |
| 安全性 | CRITICAL | 敏感文本不写进日志与错误消息（选区内容可能含密码），测试断言不打印现值 | 人工评审 |
| 安全性 | CRITICAL | 不硬编码密钥与凭据；合法字面量行尾加 `secrets:allow` 并注明缘由 | `just secrets` |
| 安全性 | CRITICAL | 密钥不经 stdout/stderr 打印 | `just lint` |
| 安全性 | CRITICAL | API key 只从配置存储读入内存，不进错误消息 | 人工评审 |
| 安全性 | CRITICAL | 配置文件解析失败走隔离降级（备份原文件并回落默认值），错误提示只报行号、不回显原文 | `just test` |
| 安全性 | CRITICAL | crypto 与随机数一律用成熟库，不手搓算法、不用弱随机源做安全用途 | 人工评审 |
| 安全性 | CRITICAL | 依赖无 RustSec 已知漏洞、许可证在白名单内、来源仅 crates.io | `just audit` + `just deny` |
| 安全性 | CRITICAL | 每个 unsafe 块前带 `// SAFETY:` 注释写明单块不变量 | `just lint` |
| 健壮性 | CRITICAL | 长驻进程不泄漏内存：禁 `mem::forget` | `just lint` + CI valgrind job（valgrind 仅覆盖 gloss-core 单测） |
| 高效性 | CRITICAL | **依赖只开需要的特性**：每条第三方依赖声明 `default-features = false` 并显式列出所需特性，无特性可关的也照写 | `just constraints` |
| 可维护性 | CRITICAL | **日志统一出口**：只经 `gloss_core::log` 宏出口，不直接依赖 tracing 三件套、不用 `println!`，库 crate 不初始化 subscriber | `just constraints` |
| 正确性 | HIGH | 错误统一 `Result`/`Option` 传播或降级（降级＝有痕迹的兜底，禁静默吞错）；`GlossError` 变体支撑状态机分支与重试；panic 家族禁入生产路径；确需绕过须旁注不变量并加显式豁免 | `just lint` + 人工评审 |
| 正确性 | HIGH | 不用 `let _ =` 丢弃 must_use 返回值；确需丢弃须旁注缘由 | `just lint` |
| 正确性 | HIGH | 新增输入路径（选区文本、配置文件、LLM 响应、屏幕截图等）接入端口时，先定义边界与格式校验并补测试再合入 | 人工评审 |
| 正确性 | HIGH | 新增或改动时序路径：先补集成测试锁定事件顺序、取消与陈旧响应过滤 | `just test` + 人工评审 |
| 正确性 | HIGH | 跨线程不变量（线程职责、通道方向、代数语义）与「架构速览」一致 | 人工评审 |
| 安全性 | HIGH | 整段 unsafe 设计优先换用安全封装 | 人工评审 |
| 健壮性 | HIGH | 远程输入设界：SSE 单行与错误响应体设长度上限，无界内存禁入 | `just test` |
| 健壮性 | HIGH | 外部依赖故障（keychain、剪贴板、网络）走降级或失败卡，不裸崩 | 人工评审 |
| 健壮性 | HIGH | panic 不穿 FFI 与线程边界（C 回调 panic 即 abort，见架构速览） | 人工评审 |
| 可维护性 | HIGH | crate 依赖只落在许可边表内；gloss-core 禁平台与渲染栈依赖；新 crate 须登记边表 | `just constraints` |
| 可维护性 | HIGH | 保持惯用 Rust，clippy 全集合零警告 | `just lint` |
| 高效性 | LOW | 跨 `Arc`、大缓冲区（图像字节）不冗余克隆 | 人工评审 |
| 可维护性 | LOW | **代码不留残留标记**：不留 TODO、FIXME、HACK、TBD | `just constraints` |
| 可维护性 | LOW | 注释只描述行为，为什么外置 docs/comments | 「注释纪律」节（人工评审） |
| 可维护性 | LOW | `#[allow(...)]` 豁免必须带缘由：生产代码写不变量旁注，测试代码记入对应文件的 tests 外置记录 | 人工评审 |
| 可读性 | LOW | 命名遵循生态惯例 | 人工评审 |

## 注释纪律

判据只有一条——**长期维护价值**：注释与外置记录都是写给未来维护者（包括未来的自己）的，只写能降低未来理解与修改成本的；一条注释该不该写、该留在代码还是外置，都以此判断。

- **描述行为，不解释动机**：注释跟着代码逻辑走，说的都是「这段代码做什么」；「为什么这样写」从命名与代码里读不出来，属于外置记录。例外是**不变量旁注**——`// SAFETY:` 的 unsafe 前提，与显式豁免旁的「为什么此处不可能失败」，必须紧贴代码、写明不变量，永不外置。
- **「为什么」外置**：决策过程、备选方案对比、任务/PR/评审引入的来龙去脉，写进 `docs/comments/<文件名>_comment.md`，按符号锚定（不写行号），改到相关代码时同步更新；mod.rs 同名冲突带父目录前缀。docs/ 除 `docs/tests/` 外不进 git；被跟踪文件不留指向这些本地记录的指针。
- **其它文件同一原则**：justfile、Cargo.toml 与其余 TOML/YAML 配置、shell 脚本、workflow 等非 Rust 文件的注释同样只写行为与用法；决策/历史/取舍外置到 `docs/comments/<文件名>_comment.md`，命名与锚定规则同上。
- **测试代码禁止注释**：`#[cfg(test)]` 模块与 `tests/` 目录里一律不写注释（`// SAFETY:` 除外，同上）；测试的验收标准、形态说明与辅助契约外置到 `docs/comments/<文件名>_tests_comment.md`（只留缘由与形态说明）。各 crate 的 tests/stubs/ 测试桩模块按生产代码对待，不在此列。

## 测试

测试纪律条目在本节维护，不进质量条目总表。每条测试行为的名称、目标、场景与更新时间登记在 `docs/tests/bdd.md`（人工测试另附可照抄的步骤），git 追踪、随代码同 PR 更新，`just test-bdd` 校验清单与源码双向一致；描述与代码冲突时以代码为准。

分层 L1–L4 按「断言成立的最低必要条件」划分：L1 纯逻辑与进程内时序，L2 egui 渲染，L3 窗口服务与 GPU，L4 真实授权或外部凭据；层由断言的需要决定，不由被测对象大小决定。成本随层单调上升，默认最低层。CI 归宿按稳定性分：L1/L2 随 `just test` 进 CI；L3 的显隐自检在同一条命令内、当前也在 CI 上跑（macOS runner 有窗口服务，预算判定对负载敏感，偶发失败先复跑确认）；L4 opt-in、不进 CI。源码注释里的 L1–L4 与本节标签同源，调层要一起改。

两条原则，其余都是推论：

1. **断言可观察契约，不碰内部状态。** 测试从公共 API 与通道两端驱动，不摸结构体字段或私有函数；测试红说明行为变了，而不是实现重构了。
2. **同步点用事件，不用时间。** 任何 sleep 都是在赌机器负载。

### 单元测试（L1）

- **主要职责**：验证最小可测单元的纯逻辑与公开契约，是数量最多、运行最快的一层。
- **测试原则**：隔离——真实时钟、网络、磁盘不进测试，外部依赖一律用替身（mock/stub）；相互独立、无顺序依赖，可任意并行。

### 快照测试（L2）

- **主要职责**：锁定渲染产物与界面结构，防视觉与无障碍树回归。
- **测试原则**：快照锁形态不锁语义——文案与状态用结构化树断言，不从像素读语义；基线是受控工件——变更必须显式审查并说明缘由，禁止静默覆盖；一个界面状态一份基线，命名可检索。

### 集成测试（L1）

- **主要职责**：验证多个组件组装后的协作时序与状态流转。
- **测试原则**：断言交互契约（顺序、取消、过滤）而非字段值；外部系统用替身替换，只测组装行为。

### 性能测试（L3）

- **主要职责**：以预算判定真实渲染栈的性能与生命周期稳定性。
- **测试原则**：以明确预算为通过条件，不是计时参考；关注最差情况而非平均；在稳定环境运行，不把负载波动当回归；通过性判定与基准量化分离，计时断言不进普通测试。

### 基准测试（量化，不入层）

- **主要职责**：量化 gloss-core 纯逻辑热点的绝对成本与回归对照——criterion 目标唯一（根包 `benches/core.rs`，`just bench` 运行），四组：cache_key（Image 输入的 PNG 字节序列化是已知大头）、parse_structured、prompt_render、moka_cache。
- **测试原则**：基准是量化工具，不是门禁——不进 CI，`just bench` 本地按需跑；计时断言不进 `#[test]`；数字连同运行环境一起记录，趋势对照只认同机同环境的历史数据；bench 目标受 clippy 全量 lints 且无 test 豁免（同 harness = false 纪律）。

### 人工测试（L4）

- **主要职责**：验证需要真实授权、真实凭据或真实外设的系统边界。
- **测试原则**：前置检查先行，条件不满足当场带修复指引失败，不以间接症状超时；真实凭据不打印不落日志；步骤可照抄，任何人都可按清单执行；不进持续集成，靠清单保证不被遗忘。（运行命令与逐条测试步骤见 `docs/tests/bdd.md`。）
