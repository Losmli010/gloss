# pre-commit（.githooks）决策记录

> 按符号锚定（不写行号，防漂移）。改到相关段落时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## 头部（不跑 test）

- 2026-09-18 不跑 test 的原因：测试由 CI 负责，本地提交不必等编译测试。

## 头部（不校验 commit message）

- 2026-09-18 commit message 规范不在 pre-commit 校验的原因：git 跑 pre-commit 时还没把消息写进 COMMIT_EDITMSG（编辑器 / -m / -F 都一样），在这里读到的只会是上一条提交的消息，既可能误拦也可能放过。消息规范由 CI 的 commitlint job 覆盖（PR / push 会校验本次引入的全部 message）。

## 检查分流（编译型 vs 纯文本）

- 2026-09-18 为什么密钥/文档引用/约束检查任何提交都跑：否则往文档/脚本里塞一个真 key 就能静默绕过本地这道闸（CI 另有兜底，但密钥越晚发现吊销成本越高）；文档失效引用最可能出现在「只改文档」的提交里；约束检查盯着 manifest 与源码，「只改 Cargo.toml」这种没有 .rs 变更的提交同样不能跳过。
- 2026-10-25 分流改为派生判定（PR-S2）：暂存含 `.rs`、`Cargo.toml`（含各分包清单，按基名判）或 `rust-toolchain.toml` 变更即视为编译型变更走 `just precommit`，否则走 `just text-gates`——钩子不再手抄门禁清单，组合关系由 justfile 的配方层单点维护。旧快路径的手抄五门禁清单有漏检：纯 `.py`／`.md` 提交跳过 comments 与 fmt-py／lint-py，纯 `.toml` 提交跳过 fmt-toml／lint-toml，本地放行、只剩 CI 兜底；新路径对这类提交补齐全部纯文本门禁（组合只增不减）。构建清单与工具链清单计入编译型的缘由：依赖与特性改动影响编译，静态检查（Clippy）必须跑。

## 配方名对照（头注不再写配方名，注释纪律门禁只许纯中文）

- 2026-10-25 头注的「提交前门禁配方」= justfile 的 `precommit` 配方（纯文本门禁组合加 Clippy）；「纯文本门禁配方」= `text-gates`（六条纯文本门禁加五条工具格式检查，`fmt` 只解析不编译故归文本层）；「构建清单」「工具链清单」= `Cargo.toml`／`rust-toolchain.toml`（代码内 `COMPILE_MANIFESTS` 常量）。
- 2026-10-24 （已被上一条取代）「全套清单见配方清单」= justfile 的 `precommit` 配方；「手动运行根配方的全量检查配方」= `just check`（fmt + clippy + test）；分流处的「格式化与静态检查」= fmt / clippy，「提交前检查配方」同 `precommit`。
