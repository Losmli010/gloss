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

# ---- 本地开发 ----

# 运行开发版（debug）——可执行 crate 是 gloss-app
run:
    cargo run -p gloss-app

# 监听文件变化自动重编译运行（需 cargo-watch）
watch:
    cargo watch -x "run -p gloss-app"

# ---- 代码质量门禁（CI 也用这些）----

# 格式化检查
fmt:
    cargo fmt --all -- --check

# 自动格式化
fmt-fix:
    cargo fmt --all

# Clippy 严格检查（警告即失败）
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# 自动修复部分 Clippy 建议
lint-fix:
    cargo clippy --all-targets --all-features --fix --allow-dirty

# 运行单元测试
test:
    cargo test --all-features

# 测试覆盖率报告（需 cargo-llvm-cov，会生成 HTML 报告）
coverage:
    cargo llvm-cov --all-features --html

# 测试覆盖率（文本摘要，不设失败阈值）
coverage-check:
    cargo llvm-cov --all-features

# 完整质量门禁：格式化 + Clippy + 测试（CI 核心）
check: fmt lint test
    @echo "✓ 质量门禁全部通过"

# ---- 构建 ----

# Debug 构建
build:
    cargo build

# Release 构建（优化）
build-release:
    cargo build --release

# ---- 发布打包（平台相关，需 cargo-bundle）----
# workspace 下 cargo-bundle 需在可执行 crate（gloss-app）目录执行；
# 产物仍输出到 workspace 根 target/ 目录。

# 生成 macOS 发布包（.app + .dmg，当前架构）
package-macos: build-release
    cd crates/gloss-app && cargo bundle --release --format osx

# 生成 Linux 发布包（.deb）
package-linux: build-release
    cd crates/gloss-app && cargo bundle --release --format deb

# 生成 Windows 发布包（.msi，需在 Windows 或交叉编译环境）
package-windows:
    cd crates/gloss-app && cargo bundle --release --format msi

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

# ---- 变更日志 ----

# 生成/更新 CHANGELOG.md（基于 conventional commits，需 git-cliff）
changelog:
    git cliff -o CHANGELOG.md

# 预览下次发版将生成的 CHANGELOG（不写文件）
changelog-preview:
    git cliff --unreleased
