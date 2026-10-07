# Gloss 项目命令入口
# 本地开发与 CI/CD 共用同一套命令，保证行为一致
# 配方按 dev / ci / release / hooks 四桶组织，与 scripts/ 及 .github/workflows/ 同构：
#   dev     本地开发与构建
#   ci      需要编译的质量门禁（CI 的 ci.yml 用）
#   release 发布链（CI 的 release.yml 用）
#   hooks   pre-commit 同款的纯文本检查，秒级（CI 的 hooks.yml 用）
# 用法：just <recipe>   查看全部：just --list

# ---- 全局默认值 ----
set shell := ["bash", "-uc"]

# 默认目标：显示帮助
default:
    @just --list

# 行覆盖率下限：低于即失败（本地与 CI 共用同一判定）
coverage_min := "70"

# ---- dev：本地开发与构建 ----

# 运行开发版（debug）
run:
    cargo run

# 监听文件变化自动重编译运行（需 cargo-watch）
watch:
    cargo watch -x run

# 打印日志目录（~/.gloss/logs）
logs-dir:
    @echo "${HOME}/.gloss/logs"

# 跟随最新日志文件（Ctrl-C 退出）
logs *args:
    @python3 scripts/dev/gloss-logs.py {{args}}

# 本地预览站点（web/，同源无 manifest.json 时页面走 GitHub Releases 降级路径）
site-preview port="8137":
    python3 -m http.server {{port}} --directory web

# Debug 构建整个 workspace
build:
    cargo build --workspace

# 指定 target 三元组构建（CI 构建矩阵用）
build-target target:
    cargo build --workspace --target {{target}}

# Release 优化构建
build-release:
    cargo build --release

# 清理构建产物
clean:
    cargo clean

# 列出依赖树
deps:
    cargo tree

# clone 后一键环境初始化：工具链校验 + 可选工具清点 + git hooks（幂等）
setup:
    ./scripts/dev/setup-dev.sh

# 重建应用图标产物（.icns 与 Dock 图标 PNG）
icons:
    ./scripts/dev/build-app-icon.sh

# 跑 gloss-core 热点基准（criterion）：自动对照上一次运行，并把结果存为本机滚动基线 last
bench:
    cargo bench --bench core -- --save-baseline last

# 把结果另存为命名基线（审计锚点，如在干净 main 上存 main），数据在 target/criterion/ 各基准目录下
bench-save name:
    cargo bench --bench core -- --save-baseline {{name}}

# 以命名基线为对照重跑基准（缺省 last；供 bench-summary 呈现对照数字，退出码不是门禁）
bench-check name="last":
    cargo bench --bench core -- --baseline {{name}}

# 汇总最近一次基准运行为 Markdown 表（均值、95% 区间、对照变化），标签仅用于展示
bench-summary baseline="上一次运行":
    ./scripts/perf/bench-summary.py "{{baseline}}"

# 跑 clone 热点基准（criterion 自定义测量：分配次数/字节，benches/clone.rs），结果存滚动基线 last
clone-bench:
    cargo bench --bench clone -- --save-baseline last

# 打印 clone 分布与热点分配（口径见脚本 docstring）
clone-stats:
    ./scripts/perf/clone-stats.py

# 重建性能基线并重绘趋势图：clone 计数+分配（需先跑 just clone-bench）→ clone-stats.json；core 墙钟快照（含环境，需先跑 just bench）→ core-baseline.json；聚合点追加 history.jsonl
perf-baseline:
    ./scripts/perf/clone-stats.py --write
    ./scripts/perf/bench-baseline.py
    ./scripts/perf/perf-history.py
    ./scripts/perf/perf-chart.py

# 对照 clone 基线报告变化（clone 计数或热点分配上升即失败；core 墙钟仅记录不判罚；审计对照，不在 precommit）
clone-check:
    ./scripts/perf/clone-stats.py --check

# 量化应用运行时资源并重绘趋势：构建 → spawn 实例采启动里程碑/RSS/CPU（建议先退出在跑的 gloss）→ app-runtime-baseline.json；聚合点随 perf-history 进 history.jsonl，app-runtime.svg 随 perf-chart 重绘
app-metrics:
    cargo build --release
    ./scripts/perf/app-metrics.py
    ./scripts/perf/perf-history.py
    ./scripts/perf/perf-chart.py

# 以 profiling 配置构建并启动带符号实例（release 同级优化但保留符号表；先退出在跑的 gloss）
profile-run:
    cargo build --profile profiling
    ./target/profiling/gloss &

# 采运行中的 gloss 实例 CPU 产出火焰图 SVG（sudo dtrace 采样 + inferno 折叠渲染，dtrace 需 root；输出 target/profile/，附原始栈与栈顶自耗 Top）
flame pid="" duration="10":
    ./scripts/perf/flamegraph.py --pid "{{pid}}" --duration {{duration}}

# 从启动起采样出启动路径火焰图（--root 只让 dtrace 提权，gloss 仍以普通用户运行；采样到 gloss 退出为止，采完退出 gloss 即出图）
flame-startup:
    mkdir -p target/profile
    cargo flamegraph --root --profile profiling --bin gloss --output target/profile/flamegraph-startup.svg

# 二进制体积归因（记录性不判罚）：节级 size -m + 符号级 otool/nm 地址差分（strip 过的产物只做节级，符号级用 profiling/dev 产物）
size-report bin="target/release/gloss" top="30":
    ./scripts/perf/size_report.py --bin {{bin}} --top {{top}}

# Prompt 评测 live 轨：真实 LLM（需 GLOSS_LIVE_*，opt-in）
# --record 回写夹具；--judge 启用 judge 评分轨（额外一次模型调用/条）
eval *args:
    cargo run -p gloss-eval --bin eval -- live {{args}}

# Prompt 评测离线重放：夹具驱动，无网络无凭据（CI 安全）
eval-replay:
    cargo run -p gloss-eval --bin eval -- replay

# 把 target/eval 下最近一次评测报告渲染为 Markdown 表（--dir 换目录）
eval-report *args:
    cargo run -p gloss-eval --bin eval -- report {{args}}

# ---- ci：需要编译的质量门禁（本地与 CI 共用同一配方）----

# 格式化检查
fmt:
    cargo fmt --all -- --check

# 自动格式化
fmt-fix:
    cargo fmt --all

# TOML 格式检查（tombi --check，不落盘）
fmt-toml:
    tombi format --check $(git ls-files '*.toml')

# TOML 自动格式化（落盘）
fmt-toml-fix:
    tombi format $(git ls-files '*.toml')

# TOML 语法与 schema lint（error 级仍会失败）
lint-toml:
    tombi lint $(git ls-files '*.toml')

# Clippy 严格检查（警告即失败）
lint:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# 自动修复部分 Clippy 建议
lint-fix:
    cargo clippy --workspace --all-targets --all-features --fix --allow-dirty

# 运行全部测试
test:
    cargo test --workspace --all-features

# L3 显隐自检：100 轮浮层显隐 + 首帧预算（需窗口服务与 GPU）
selftest:
    cargo test -p gloss --test overlay

# L3 启动预算自检：渲染栈初始化墙钟对 STARTUP_BUDGET（需窗口服务与 GPU）
startup-selftest:
    cargo test -p gloss --test startup

# L3 双自检 wrapper：跑显隐+启动自检，从 stderr 提取结构化信号行并外部轮询完整 RSS 曲线，落 out（缺省 target/perf/selftest.jsonl，本地 handoff 不入 git）
selftest-report out="target/perf/selftest.jsonl":
    ./scripts/perf/selftest_wrapper.py --out {{out}}

# 测试覆盖率（摘要 + HTML 报告），低于 coverage_min 即失败
coverage:
    cargo llvm-cov clean --workspace
    cargo llvm-cov --no-report --workspace --all-features
    cargo llvm-cov report --workspace --html
    cargo llvm-cov report --workspace --fail-under-lines {{coverage_min}}

# 测试覆盖率（仅终端摘要），按同一阈值判定
coverage-check:
    cargo llvm-cov --workspace --all-features --fail-under-lines {{coverage_min}}

# 安全审计（RustSec 已知漏洞）
audit:
    cargo audit

# 依赖合规检查（许可证 / 重复依赖 / 来源）
deny:
    cargo deny check

# Miri 未定义行为检测（nightly，只覆盖 gloss-core 纯逻辑层）
miri:
    MIRIFLAGS="-Zmiri-disable-isolation" cargo +nightly miri test -p gloss-core --all-features -- --skip cache:: --skip engine:: --skip file_writer_persists_lines_into_daily_file

# 完整质量门禁：precommit 的全部 + test
check: constraints agents-doc test-bdd i18n fmt fmt-toml lint lint-toml test secrets
    @echo "✓ 质量门禁全部通过"

# 提交前门禁：约束 + 文档引用 + BDD 清单 + 文案表 + fmt + TOML + clippy + 密钥扫描
precommit: constraints agents-doc test-bdd i18n fmt fmt-toml lint lint-toml secrets
    @echo "✓ 提交前检查通过（测试交给 CI）"

# ---- release：发布链（CI 的 release.yml 用同一批脚本）----

# 发布打包：release 构建 → bundle 出 .app → ad-hoc 签名 → 压 .dmg
package-macos: build-release
    #!/usr/bin/env bash
    set -euo pipefail
    cargo bundle --release --format osx
    app="target/release/bundle/osx/Gloss.app"
    codesign --force --deep --sign - "$app"
    codesign --verify --deep --strict "$app"
    ./scripts/release/bundle-dmg.sh "$app"
    echo "产物：$app 与 ${app%.app}.dmg"

# 发布冒烟：启动 .app，验证能启动、活得住、日志无 panic
smoke-app app:
    ./scripts/release/smoke-app.sh {{app}}

# 校验发布 tag 与版本单点一致
release-check tag:
    ./scripts/release/check-release-tag.sh {{tag}}

# 本地生成站点 manifest.json 到 stdout（artifacts 目录须含双架构 zip/dmg 四个产物）
gen-manifest tag artifacts-dir:
    ./scripts/release/gen-manifest.sh {{tag}} {{artifacts-dir}}

# ---- hooks：pre-commit 同款检查（纯 bash 秒级，CI 的 hooks.yml 也跑）----

# 硬编码密钥扫描（命中即失败）
secrets:
    ./scripts/hooks/check-secrets.sh

# 校验 AGENTS.md 引用的配方/路径/测试目标/质量条目名真实存在
agents-doc:
    ./scripts/hooks/check-agents-doc.sh

# bdd.md 测试行为清单与测试源码双向核对（缺登记 / 失同步即失败）
test-bdd:
    ./scripts/hooks/check-test-bdd.sh

# 自动化门禁（依赖方向 / 日志 / egui 上下文装入点 / 版本单点 / 依赖特性 / 残留标记）
constraints:
    ./scripts/hooks/check-constraints.sh

# 界面文案统一进文案表（文案字面量 / 装入点 / 词条表唯一）
i18n:
    ./scripts/hooks/check-i18n.sh

# 校验单条 commit message 是否符合 Conventional Commits
lint-commit file:
    ./scripts/hooks/check-commit-msg.sh "{{file}}"

# 安装本地 git hooks（clone 后运行一次）
install-hooks:
    ./scripts/hooks/install-hooks.sh
