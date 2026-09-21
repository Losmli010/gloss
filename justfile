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
logs:
    #!/usr/bin/env bash
    set -euo pipefail
    dir="${HOME}/.gloss/logs"
    newest=$(ls -t "$dir"/gloss.log.* 2>/dev/null | head -n 1 || true)
    if [ -z "${newest:-}" ]; then
        echo "no log file under $dir yet (run the app first)" >&2
        exit 1
    fi
    echo "tailing $newest"
    tail -n +1 -f "$newest"

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
    ./scripts/bench-summary.py "{{baseline}}"

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
    cargo test -p gloss --test overlay_selftest

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
check: constraints agents-doc test-bdd fmt fmt-toml lint lint-toml test secrets
    @echo "✓ 质量门禁全部通过"

# 提交前门禁：约束 + 文档引用 + BDD 清单 + fmt + TOML + clippy + 密钥扫描
precommit: constraints agents-doc test-bdd fmt fmt-toml lint lint-toml secrets
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

# 基于 conventional commits 生成 CHANGELOG
changelog:
    git cliff -o CHANGELOG.md

# 预览将生成的 CHANGELOG（不写文件）
changelog-preview:
    git cliff --unreleased

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

# 自动化门禁（依赖方向 / 日志 / 版本单点 / 依赖特性 / 残留标记）
constraints:
    ./scripts/hooks/check-constraints.sh

# 校验单条 commit message 是否符合 Conventional Commits
lint-commit file:
    ./scripts/hooks/check-commit-msg.sh "{{file}}"

# 安装本地 git hooks（clone 后运行一次）
install-hooks:
    ./scripts/hooks/install-hooks.sh
