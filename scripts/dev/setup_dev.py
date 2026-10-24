#!/usr/bin/env python3
"""一键环境初始化：校验本语言（Rust）工具链与配套工具、安装仓库钩子。克隆后跑一次
（走初始化配方）。全部幂等，重复跑无副作用。
"""

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
    print("  ✗ missing %s — install: %s" % (name, install_cmd))


def have(cmd):
    return shutil.which(cmd) is not None


def have_python_module(module):
    return (
        subprocess.run([sys.executable, "-m", module, "--version"], capture_output=True).returncode
        == 0
    )


def main():
    os.chdir(ROOT)

    print("== 1/4 Rust toolchain ==")
    if not have("cargo"):
        print("error: no cargo, nothing can compile. install rustup first:", file=sys.stderr)
        print("  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh", file=sys.stderr)
        return 1
    rustc_version = subprocess.run(
        ["rustc", "--version"], capture_output=True, text=True, check=True
    ).stdout.split()[1]
    if rustc_version == EXPECTED_TOOLCHAIN:
        print("  ✓ rustc %s (matches CI)" % rustc_version)
    else:
        print(
            "  ⚠ rustc %s != CI pinned %s, aligning recommended:"
            % (rustc_version, EXPECTED_TOOLCHAIN),
            file=sys.stderr,
        )
        print(
            "      rustup install %s && rustup override set %s"
            % (EXPECTED_TOOLCHAIN, EXPECTED_TOOLCHAIN),
            file=sys.stderr,
        )
    if subprocess.run(["cargo", "fmt", "--version"], capture_output=True).returncode == 0:
        print("  ✓ component rustfmt installed")
    else:
        print(
            "  ⚠ missing component rustfmt, install: rustup component add rustfmt", file=sys.stderr
        )
    if subprocess.run(["cargo", "clippy", "--version"], capture_output=True).returncode == 0:
        print("  ✓ component clippy installed")
    else:
        print("  ⚠ missing component clippy, install: rustup component add clippy", file=sys.stderr)

    print("== 2/4 just command runner ==")
    # 仓库所有配方（门禁、测试、打包）都经命令入口，没有它后面寸步难行
    if have("just"):
        version = subprocess.run(
            ["just", "--version"], capture_output=True, text=True, check=True
        ).stdout.split()[1]
        print("  ✓ just %s" % version)
    else:
        warn_missing("just", "brew install just (or cargo install just --locked)")

    print("== 3/4 quality / release extras (optional, install as needed) ==")
    if not have("cargo-bundle"):
        warn_missing("cargo-bundle (release packaging)", "cargo install cargo-bundle --locked")
    if not have("cargo-llvm-cov"):
        warn_missing("cargo-llvm-cov (coverage)", "cargo install cargo-llvm-cov --locked")
    if not have("cargo-audit"):
        warn_missing("cargo-audit (vulnerability audit)", "cargo install cargo-audit --locked")
    if not have("cargo-deny"):
        warn_missing("cargo-deny (dependency compliance)", "cargo install cargo-deny --locked")
    # 提交前门禁依赖配置格式（TOML）与脚本语言（Python）的格式化及检查工具，缺了它提交钩子会挂
    if not have("tombi"):
        warn_missing(
            "tombi (TOML format and lint, needed by precommit)",
            "npm i -g tombi@1.5.5 (or brew install tombi)",
        )
    if not have("ruff"):
        warn_missing(
            "ruff (Python format and lint, needed by precommit)",
            "uv tool install ruff@0.17.2 (or pip install ruff==0.17.2)",
        )
    if not have_python_module("pytest"):
        warn_missing(
            "pytest (checker unit tests, same as the CI hooks job)", "python3 -m pip install pytest"
        )
    # 火焰图配方的采样与渲染后端
    if not have("inferno-flamegraph"):
        warn_missing(
            "cargo-flamegraph/inferno (flamegraph profiling, needed by just flame and flame-startup)",
            "cargo install flamegraph inferno rustfilt --locked",
        )

    print("== 4/4 git hooks ==")
    subprocess.run(["just", "install-hooks"], check=True)

    print()
    if missing_optional == 0:
        print("✓ environment ready; run just check for full verification")
    else:
        print(
            "✓ environment mostly ready; %d optional items missing above, install them to use the related recipes"
            % missing_optional
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
