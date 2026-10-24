#!/usr/bin/env python3
# 一键环境初始化：校验本语言（Rust）工具链与配套工具、安装仓库钩子。克隆后跑一次
# （走初始化配方）。全部幂等，重复跑无副作用。

import os
import shutil
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
EXPECTED_TOOLCHAIN = "1.96.1"

missing_optional = 0


def warn_missing(name, install_cmd):
    global missing_optional
    missing_optional += 1
    print("  ✗ 缺 %s —— 安装：%s" % (name, install_cmd))


def have(cmd):
    return shutil.which(cmd) is not None


def have_python_module(module):
    return (
        subprocess.run([sys.executable, "-m", module, "--version"], capture_output=True).returncode
        == 0
    )


def main():
    os.chdir(ROOT)

    print("== 1/4 Rust 工具链 ==")
    if not have("cargo"):
        print("错误：没有 cargo，什么都编译不了。先装 rustup：", file=sys.stderr)
        print("  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh", file=sys.stderr)
        return 1
    rustc_version = subprocess.run(
        ["rustc", "--version"], capture_output=True, text=True, check=True
    ).stdout.split()[1]
    if rustc_version == EXPECTED_TOOLCHAIN:
        print("  ✓ rustc %s（与 CI 一致）" % rustc_version)
    else:
        print(
            "  ⚠ rustc %s ≠ CI 钉住的 %s，建议对齐：" % (rustc_version, EXPECTED_TOOLCHAIN),
            file=sys.stderr,
        )
        print(
            "      rustup install %s && rustup override set %s"
            % (EXPECTED_TOOLCHAIN, EXPECTED_TOOLCHAIN),
            file=sys.stderr,
        )
    if subprocess.run(["cargo", "fmt", "--version"], capture_output=True).returncode == 0:
        print("  ✓ 组件 rustfmt 已装")
    else:
        print("  ⚠ 缺组件 rustfmt，安装：rustup component add rustfmt", file=sys.stderr)
    if subprocess.run(["cargo", "clippy", "--version"], capture_output=True).returncode == 0:
        print("  ✓ 组件 clippy 已装")
    else:
        print("  ⚠ 缺组件 clippy，安装：rustup component add clippy", file=sys.stderr)

    print("== 2/4 just 命令入口 ==")
    # 仓库所有配方（门禁、测试、打包）都经命令入口，没有它后面寸步难行
    if have("just"):
        version = subprocess.run(
            ["just", "--version"], capture_output=True, text=True, check=True
        ).stdout.split()[1]
        print("  ✓ just %s" % version)
    else:
        warn_missing("just", "brew install just（或 cargo install just --locked）")

    print("== 3/4 质量 / 发布配套（可选，按需安装）==")
    if not have("cargo-bundle"):
        warn_missing("cargo-bundle（发布打包）", "cargo install cargo-bundle --locked")
    if not have("cargo-llvm-cov"):
        warn_missing("cargo-llvm-cov（覆盖率）", "cargo install cargo-llvm-cov --locked")
    if not have("cargo-audit"):
        warn_missing("cargo-audit（漏洞审计）", "cargo install cargo-audit --locked")
    if not have("cargo-deny"):
        warn_missing("cargo-deny（依赖合规）", "cargo install cargo-deny --locked")
    # 提交前门禁依赖配置格式（TOML）与脚本语言（Python）的格式化及检查工具，缺了它提交钩子会挂
    if not have("tombi"):
        warn_missing(
            "tombi（TOML 格式与 lint，precommit 依赖）",
            "npm i -g tombi@1.5.5（或 brew install tombi）",
        )
    if not have("ruff"):
        warn_missing(
            "ruff（Python 格式与 lint，precommit 依赖）",
            "uv tool install ruff@0.17.2（或 pip install ruff==0.17.2）",
        )
    if not have_python_module("pytest"):
        warn_missing(
            "pytest（checker 单测，CI 的 hooks job 同款）", "python3 -m pip install pytest"
        )
    # 火焰图配方的采样与渲染后端
    if not have("inferno-flamegraph"):
        warn_missing(
            "cargo-flamegraph/inferno（火焰图剖析，just flame 与 flame-startup 依赖）",
            "cargo install flamegraph inferno rustfilt --locked",
        )

    print("== 4/4 git hooks ==")
    subprocess.run(["just", "install-hooks"], check=True)

    print()
    if missing_optional == 0:
        print("✓ 环境就绪，可跑 just check 做全量验证")
    else:
        print("✓ 环境基本就绪；上面有 %d 个可选项缺失，按提示装齐后再用对应配方" % missing_optional)
    return 0


if __name__ == "__main__":
    sys.exit(main())
