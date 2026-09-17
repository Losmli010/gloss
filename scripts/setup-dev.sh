#!/usr/bin/env bash
# 一键环境初始化：校验 Rust 工具链与配套工具、安装 git hooks。clone 后跑一次：
#   ./scripts/setup-dev.sh   （或 just setup）
# 原则：硬失败只留给「没有 Rust 什么都干不了」这一件事；其余缺失只提示安装
# 命令，不替用户做全局安装。全部幂等，重复跑无副作用。
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

# CI 与 release workflow 钉住的工具链版本（dtolnay/rust-toolchain@<该版本>），
# 与 CI 漂移会造成「本地过、CI 挂」。
EXPECTED_TOOLCHAIN="1.96.1"

MISSING_OPTIONAL=0

warn_missing() {
  local name="$1" install_cmd="$2"
  MISSING_OPTIONAL=$((MISSING_OPTIONAL + 1))
  echo "  ✗ 缺 $name —— 安装：$install_cmd"
}

echo "== 1/4 Rust 工具链 =="
if ! command -v cargo >/dev/null 2>&1; then
  echo "错误：没有 cargo，什么都编译不了。先装 rustup：" >&2
  echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh" >&2
  exit 1
fi
rustc_version="$(rustc --version | awk '{print $2}')"
# 变量名后紧跟全角字符时必须加花括号：bash 3.2 会把全角字符首字节吞进变量名
# （报 unbound variable），这是本机 bash 3.2 实测踩过的坑。
if [ "$rustc_version" = "$EXPECTED_TOOLCHAIN" ]; then
  echo "  ✓ rustc ${rustc_version}（与 CI 一致）"
else
  echo "  ⚠ rustc ${rustc_version} ≠ CI 钉住的 ${EXPECTED_TOOLCHAIN}，建议对齐：" >&2
  echo "      rustup install $EXPECTED_TOOLCHAIN && rustup override set $EXPECTED_TOOLCHAIN" >&2
fi
# 组件存在性用命令探测而非 rustup component list：仓库用目录级 pin 时，
# 不带 --toolchain 的 rustup 查的是默认工具链，会误报缺组件。
if cargo fmt --version >/dev/null 2>&1; then
  echo "  ✓ 组件 rustfmt 已装"
else
  echo "  ⚠ 缺组件 rustfmt，安装：rustup component add rustfmt" >&2
fi
if cargo clippy --version >/dev/null 2>&1; then
  echo "  ✓ 组件 clippy 已装"
else
  echo "  ⚠ 缺组件 clippy，安装：rustup component add clippy" >&2
fi

echo "== 2/4 just 命令入口 =="
# 仓库所有配方（门禁 / 测试 / 打包）都经 just，没有它后面寸步难行
if command -v just >/dev/null 2>&1; then
  echo "  ✓ just $(just --version | awk '{print $2}')"
else
  warn_missing "just" "brew install just（或 cargo install just --locked）"
fi

echo "== 3/4 质量 / 发布配套（可选，按需安装）=="
# 各工具只做存在性检查：具体版本交给 Dependabot 与 CI 兜底
command -v cargo-bundle >/dev/null 2>&1 \
  || warn_missing "cargo-bundle（发布打包）" "cargo install cargo-bundle --locked"
command -v cargo-llvm-cov >/dev/null 2>&1 \
  || warn_missing "cargo-llvm-cov（覆盖率）" "cargo install cargo-llvm-cov --locked"
command -v cargo-audit >/dev/null 2>&1 \
  || warn_missing "cargo-audit（漏洞审计）" "cargo install cargo-audit --locked"
command -v cargo-deny >/dev/null 2>&1 \
  || warn_missing "cargo-deny（依赖合规）" "cargo install cargo-deny --locked"

echo "== 4/4 git hooks =="
just install-hooks

echo
if [ "$MISSING_OPTIONAL" -eq 0 ]; then
  echo "✓ 环境就绪，可跑 just check 做全量验证"
else
  echo "✓ 环境基本就绪；上面有 $MISSING_OPTIONAL 个可选项缺失，按提示装齐后再用对应配方"
fi
