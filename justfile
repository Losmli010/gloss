# Gloss 项目命令入口
# 本地开发与 CI/CD 共用同一套命令，保证行为一致。
# 用法：just <recipe>   查看全部：just --list

# ---- 全局默认值 ----
set shell := ["bash", "-uc"]

# 默认目标：显示帮助
default:
    @just --list

# 项目名称 / 版本（供打包用）
name := "gloss"
version := env_var_or_default("GLOSS_VERSION", "0.1.0")

# 行覆盖率下限：低于该值即失败（本地 just coverage 与 CI 的 coverage job 共用）
# M3 分层测试（状态机抽出 + pipeline/popup 集成与 harness 测试）落地后，
# 总量实测 81%，35 的临时值还账调回 70。
coverage_min := "70"

# ---- 本地开发 ----

# 运行开发版（debug）
run:
    cargo run

# 监听文件变化自动重编译运行（需 cargo-watch）
watch:
    cargo watch -x run

# 日志目录（~/.gloss/logs，与代码里的 log_dir() 保持一致）
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

# ---- 代码质量门禁（CI 也用这些）----

# 格式化检查
fmt:
    cargo fmt --all -- --check

# 自动格式化
fmt-fix:
    cargo fmt --all

# Clippy 严格检查（警告即失败）
lint:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# 自动修复部分 Clippy 建议
lint-fix:
    cargo clippy --workspace --all-targets --all-features --fix --allow-dirty

# 硬编码密钥扫描（脚本同时被 CI quality job 复用；规则与放行标记见脚本头注释）
secrets:
    ./scripts/check-secrets.sh

# AGENTS.md 引用一致性：文档里提到的每个 just 配方 / 仓库路径 / 测试目标必须真实
# 存在（脚本同时被 CI quality job 复用）。指令文件是唯一会被逐次加载的文档，
# 它描述的世界一旦过期，代理就照着错的信息干活。
agents-doc:
    ./scripts/check-agents-doc.sh

# 「不可协商的约束」里可机械判定的部分：依赖方向 / 日志统一出口 / 日志英文 /
# 版本单点 / 依赖特性（按 manifest 与源码解析，纯 bash，不碰 cargo，秒级）。
# 逐条覆盖与不覆盖的理由见脚本头注释。
constraints:
    ./scripts/check-constraints.sh

# 运行单元测试
test:
    cargo test --workspace --all-features

# L3 显隐自检：100 轮浮层显隐 + 首帧延迟预算（需要窗口服务与 GPU）。
# harness=false 的自检不是普通单测，不随 test 配方一起跑。
selftest:
    cargo test -p gloss --test overlay_selftest

# 测试覆盖率：终端摘要 + HTML 报告（→ target/llvm-cov/html；CI 也跑这条）
# 最后一步带阈值，行覆盖率低于 coverage_min 时整个配方失败
coverage:
    cargo llvm-cov clean --workspace
    cargo llvm-cov --no-report --workspace --all-features
    cargo llvm-cov report --workspace --html
    cargo llvm-cov report --workspace --fail-under-lines {{coverage_min}}

# 测试覆盖率：只打终端摘要并按同一阈值判定（不生成 HTML）
coverage-check:
    cargo llvm-cov --workspace --all-features --fail-under-lines {{coverage_min}}

# 完整质量门禁：约束检查 + 格式化 + Clippy + 测试 + 密钥扫描 + 文档引用校验（CI 核心；本地要全量验证时手动跑）
check: constraints agents-doc fmt lint test secrets
    @echo "✓ 质量门禁全部通过"

# 提交前门禁：约束检查 + 文档引用校验 + 格式化 + Clippy + 密钥扫描（pre-commit 用）
# 不含 test：测试由 CI 跑，本地提交不必等编译测试
precommit: constraints agents-doc fmt lint secrets
    @echo "✓ 提交前检查通过（测试交给 CI）"

# 校验 commit message 是否符合 Conventional Commits（与 CI 共用同一脚本，手动排查用）
lint-commit file:
    ./scripts/check-commit-msg.sh "{{file}}"

# ---- 构建 ----

# Debug 构建
build:
    cargo build --workspace

# 指定 target 构建（CI 构建矩阵用）
build-target target:
    cargo build --workspace --target {{target}}

# Release 构建（优化）
build-release:
    cargo build --release

# ---- 发布打包（需 cargo-bundle）----

# 生成发布包（.app + .dmg，当前架构）
package-macos: build-release
    cargo bundle --release --format osx

# ---- 清理 ----

# 清理构建产物
clean:
    cargo clean

# ---- 辅助 ----

# 安装本地 git hooks（clone 后运行一次）
install-hooks:
    ./scripts/install-hooks.sh

# 列出所有依赖树
deps:
    cargo tree

# ---- 安全与合规 ----

# 安全审计（已知漏洞）
audit:
    cargo audit

# 依赖合规检查：漏洞 / 许可证 / 重复依赖 / 来源（配置见 deny.toml）
deny:
    cargo deny check

# ---- 变更日志 ----

# 生成/更新 CHANGELOG.md（基于 conventional commits，需 git-cliff）
changelog:
    git cliff -o CHANGELOG.md

# 预览下次发版将生成的 CHANGELOG（不写文件）
changelog-preview:
    git cliff --unreleased
