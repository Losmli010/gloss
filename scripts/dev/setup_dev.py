#!/usr/bin/env python3
"""一键环境初始化：校验本语言（Rust）工具链与配套工具、安装仓库钩子。克隆后跑一次
（走初始化配方）。全部幂等，重复跑无副作用；版本比对以仓库根版本清单与工具链文件
为单点——工具链版本解析原生钉版本文件，清单里钉死的按全等、记下限的按不小于、
显式浮动的只看在场。
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys

import tool_version

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))

missing_optional = 0


def warn_missing(name: str, install_cmd: str) -> None:
    global missing_optional
    missing_optional += 1
    print("  error: missing %s — install: %s" % (name, install_cmd))


def have(cmd: str) -> bool:
    return shutil.which(cmd) is not None


def have_python_module(module: str) -> bool:
    return (
        subprocess.run([sys.executable, "-m", module, "--version"], capture_output=True).returncode
        == 0
    )


def command_version(cmd: str) -> str | None:
    result = subprocess.run([cmd, "--version"], capture_output=True, text=True)
    if result.returncode != 0:
        return None
    fields = result.stdout.split()
    return fields[1] if len(fields) > 1 else None


def version_tuple(text: str | None) -> tuple[int, ...] | None:
    parts = text.split(".") if text else []
    if not parts or not all(part.isdigit() for part in parts):
        return None
    return tuple(int(part) for part in parts)


def toolchain_channel() -> str | None:
    sections = tool_version.parse_sections(os.path.join(ROOT, "rust-toolchain.toml"))
    return sections.get("toolchain", {}).get("channel")


def check_pinned(cmd: str, label: str, pin: str | None, install_hint: str) -> None:
    if pin is None:
        print(
            "  warn: %s not pinned in the tool list, skipping version check" % cmd, file=sys.stderr
        )
    elif not have(cmd):
        warn_missing(label, install_hint.format(pin=pin))
    else:
        actual = command_version(cmd)
        if actual == pin:
            print("  ok: %s %s (pinned)" % (cmd, actual))
        else:
            print(
                "  warn: %s %s != pinned %s, realign recommended: %s"
                % (cmd, actual, pin, install_hint.format(pin=pin)),
                file=sys.stderr,
            )


def main() -> int:
    os.chdir(ROOT)
    tools = tool_version.parse_sections(tool_version.TOOLS_PATH)

    print("== 1/6 Rust toolchain ==")
    if not have("cargo"):
        print("error: no cargo, nothing can compile. install rustup first:", file=sys.stderr)
        print("  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh", file=sys.stderr)
        return 1
    channel = toolchain_channel()
    rustc_version = subprocess.run(
        ["rustc", "--version"], capture_output=True, text=True, check=True
    ).stdout.split()[1]
    if channel is None:
        print("  warn: cannot read the pinned channel from the toolchain file", file=sys.stderr)
    elif rustc_version == channel:
        print("  ok: rustc %s (matches the pinned toolchain file)" % rustc_version)
    else:
        print(
            "  warn: rustc %s != pinned %s, aligning recommended:" % (rustc_version, channel),
            file=sys.stderr,
        )
        print(
            "      rustup install %s && rustup override set %s" % (channel, channel),
            file=sys.stderr,
        )
    if subprocess.run(["cargo", "fmt", "--version"], capture_output=True).returncode == 0:
        print("  ok: component rustfmt installed")
    else:
        print(
            "  warn: missing component rustfmt, install: rustup component add rustfmt",
            file=sys.stderr,
        )
    if subprocess.run(["cargo", "clippy", "--version"], capture_output=True).returncode == 0:
        print("  ok: component clippy installed")
    else:
        print(
            "  warn: missing component clippy, install: rustup component add clippy",
            file=sys.stderr,
        )

    print("== 2/6 gate interpreter ==")
    bound = tools.get("python", {}).get("min")
    bound_tuple = version_tuple(bound)
    if bound_tuple is None:
        print("  warn: interpreter bound missing from the tool list", file=sys.stderr)
    else:
        # 解释器入口与下限同名：任务配方经同名入口调用，校验落在实际调用的解释器上
        entry = "python%s" % bound
        actual_text = command_version(entry) if have(entry) else None
        actual = version_tuple(actual_text)
        if actual is not None and actual >= bound_tuple:
            print("  ok: %s %s (>= %s)" % (entry, actual_text, bound))
        else:
            print(
                "error: gate interpreter %s not found or below the pinned floor %s —"
                " every recipe runs on it." % (entry, bound),
                file=sys.stderr,
            )
            print(
                "  install: install Python %s or newer from python.org or your package manager"
                % bound,
                file=sys.stderr,
            )
            return 1

    print("== 3/6 just command runner ==")
    # 仓库所有配方（门禁、测试、打包）都经命令入口，没有它后面寸步难行
    if have("just"):
        version = subprocess.run(
            ["just", "--version"], capture_output=True, text=True, check=True
        ).stdout.split()[1]
        print("  ok: just %s" % version)
    else:
        warn_missing("just", "brew install just (or cargo install just --locked)")

    print("== 4/6 quality / release extras (optional, install as needed) ==")
    if not have("cargo-bundle"):
        warn_missing("cargo-bundle (release packaging)", "cargo install cargo-bundle --locked")
    if not have("cargo-llvm-cov"):
        warn_missing("cargo-llvm-cov (coverage)", "cargo install cargo-llvm-cov --locked")
    if not have("cargo-audit"):
        warn_missing("cargo-audit (vulnerability audit)", "cargo install cargo-audit --locked")
    if not have("cargo-deny"):
        warn_missing("cargo-deny (dependency compliance)", "cargo install cargo-deny --locked")
    # 提交前门禁依赖配置格式（TOML）与脚本语言（Python）的格式化及检查工具，缺了它提交钩子会挂
    check_pinned(
        "tombi",
        "tombi (TOML format and lint, needed by precommit)",
        tools.get("tombi", {}).get("version"),
        "npm i -g tombi@{pin}",
    )
    check_pinned(
        "ruff",
        "ruff (Python format and lint, needed by precommit)",
        tools.get("ruff", {}).get("version"),
        "uv tool install ruff@{pin} (or pip install ruff=={pin})",
    )
    if not have_python_module("pytest"):
        warn_missing(
            "pytest (checker unit tests, same as the CI hooks job)",
            "%s -m pip install pytest" % sys.executable,
        )
    # 火焰图配方的采样与渲染后端
    if not have("inferno-flamegraph"):
        warn_missing(
            "cargo-flamegraph/inferno (flamegraph profiling, needed by just flame and flame-startup)",
            "cargo install flamegraph inferno rustfilt --locked",
        )

    print("== 5/6 secrets gate engine ==")
    # 密钥门禁每提交必跑：引擎缺失当场失败而非静默跳过，版本不一致给对齐建议
    rg_pin = tools.get("ripgrep", {}).get("version")
    if not have("rg"):
        hint = (
            "cargo install ripgrep --version %s --locked" % rg_pin
            if rg_pin
            else "install ripgrep and pin its version in the tool list"
        )
        print(
            "error: ripgrep missing — the secrets gate runs on every commit"
            " and cannot scan without it.",
            file=sys.stderr,
        )
        print("  install: %s" % hint, file=sys.stderr)
        return 1
    check_pinned(
        "rg",
        "ripgrep (secrets gate engine, needed by every commit)",
        rg_pin,
        "cargo install ripgrep --version {pin} --locked",
    )

    print("== 6/6 git hooks ==")
    subprocess.run(["just", "install-hooks"], check=True)

    print()
    if missing_optional == 0:
        print("ok: environment ready; run just check for full verification")
    else:
        print(
            "ok: environment mostly ready; %d optional items missing above, install them to use the related recipes"
            % missing_optional
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
