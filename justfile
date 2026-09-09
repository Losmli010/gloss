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

# 运行开发版（debug）
run:
    cargo run

# 监听文件变化自动重编译运行（需 cargo-watch）
watch:
    cargo watch -x run

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

# ---- 发布打包（平台相关）----

# 生成 macOS 发布包（.app + .dmg）
package-macos: build-release
    @echo "生成 macOS 产物…"
    # 后续用 cargo-bundle / tauri-bundler 或 xcode 打包，这里先占位
    @echo "目标：target/release/{{ name }}.app → {{ name }}-{{ version }}-macos.dmg"

# 生成 Linux 发布包（.AppImage / .deb）
package-linux: build-release
    @echo "生成 Linux 产物…"

# 生成 Windows 发布包（.exe，需交叉编译）
package-windows:
    @echo "生成 Windows 产物（需 cargo cross 或 windows runner）…"

# ---- 清理 ----

# 清理构建产物
clean:
    cargo clean

# ---- 辅助 ----

# 安装本地 git hooks（clone 后运行一次）
install-hooks:
    ./scripts/install-hooks.sh

# 校验某条 commit message（用法：just check-commit "feat: xxx"）
check-commit msg:
    @echo "{{ msg }}" | ./scripts/check-commit-msg.sh -

# 运行 commit 校验脚本的单元测试
test-commit-msg:
    bash tests/test-check-commit-msg.sh

# 列出所有依赖树
deps:
    cargo tree
