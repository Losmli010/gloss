#!/usr/bin/env python3
"""一键环境初始化：校验本语言（Rust）工具链与配套工具、安装仓库钩子。克隆后跑一次
（走初始化配方）。全部幂等，重复跑无副作用；版本比对以仓库根版本清单与工具链文件
为单点——工具链版本解析原生钉版本文件，清单里钉死的按全等、记下限的按不小于、
显式浮动的只看在场。
"""

import os
import shutil
import subprocess
import sys

import tool_version

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))

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


def command_version(cmd):
    result = subprocess.run([cmd, "--version"], capture_output=True, text=True)
    if result.returncode != 0:
        return None
    fields = result.stdout.split()
    return fields[1] if len(fields) > 1 else None


def version_tuple(text):
    parts = text.split(".") if text else []
    if not parts or not all(part.isdigit() for part in parts):
        return None
    return tuple(int(part) for part in parts)


def toolchain_channel():
    sections = tool_version.parse_sections(os.path.join(ROOT, "rust-toolchain.toml"))
    return sections.get("toolchain", {}).get("channel")


def check_pinned(cmd, label, pin, install_hint):
    if pin is None:
        print("  ⚠ %s not pinned in the tool list, skipping version check" % cmd, file=sys.stderr)
    elif not have(cmd):
        warn_missing(label, install_hint.format(pin=pin))
    else:
        actual = command_version(cmd)
        if actual == pin:
            print("  ✓ %s %s (pinned)" % (cmd, actual))
        else:
            print(
                "  ⚠ %s %s != pinned %s, realign recommended: %s"
                % (cmd, actual, pin, install_hint.format(pin=pin)),
                file=sys.stderr,
            )


def find_interpreter(bound, bound_tuple):
    found = None
    # 主名解释器可能低于下限：按下限取同名解释器作后备候选
    for cmd in ("python3", "python%s" % bound):
        if not have(cmd):
            continue
        actual_text = command_version(cmd)
        actual = version_tuple(actual_text)
        if actual is not None and actual >= bound_tuple:
            found = (cmd, actual_text)
            break
    return found


def main():
    os.chdir(ROOT)
    tools = tool_version.parse_sections(tool_version.TOOLS_PATH)

    print("== 1/5 Rust toolchain ==")
    if not have("cargo"):
        print("error: no cargo, nothing can compile. install rustup first:", file=sys.stderr)
        print("  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh", file=sys.stderr)
        return 1
    channel = toolchain_channel()
    rustc_version = subprocess.run(
        ["rustc", "--version"], capture_output=True, text=True, check=True
    ).stdout.split()[1]
    if channel is None:
        print("  ⚠ cannot read the pinned channel from the toolchain file", file=sys.stderr)
    elif rustc_version == channel:
        print("  ✓ rustc %s (matches the pinned toolchain file)" % rustc_version)
    else:
        print(
            "  ⚠ rustc %s != pinned %s, aligning recommended:" % (rustc_version, channel),
            file=sys.stderr,
        )
        print(
            "      rustup install %s && rustup override set %s" % (channel, channel),
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

    print("== 2/5 gate interpreter ==")
    bound = tools.get("python", {}).get("min")
    bound_tuple = version_tuple(bound)
    if bound_tuple is None:
        print("  ⚠ interpreter bound missing from the tool list", file=sys.stderr)
    else:
        found = find_interpreter(bound, bound_tuple)
        if found is not None:
            print("  ✓ %s %s (>= %s)" % (found[0], found[1], bound))
        else:
            warn_missing(
                "python >= %s" % bound,
                "install Python %s or newer from python.org or your package manager" % bound,
            )

    print("== 3/5 just command runner ==")
    # 仓库所有配方（门禁、测试、打包）都经命令入口，没有它后面寸步难行
    if have("just"):
        version = subprocess.run(
            ["just", "--version"], capture_output=True, text=True, check=True
        ).stdout.split()[1]
        print("  ✓ just %s" % version)
    else:
        warn_missing("just", "brew install just (or cargo install just --locked)")

    print("== 4/5 quality / release extras (optional, install as needed) ==")
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
            "pytest (checker unit tests, same as the CI hooks job)", "python3 -m pip install pytest"
        )
    # 火焰图配方的采样与渲染后端
    if not have("inferno-flamegraph"):
        warn_missing(
            "cargo-flamegraph/inferno (flamegraph profiling, needed by just flame and flame-startup)",
            "cargo install flamegraph inferno rustfilt --locked",
        )

    print("== 5/5 git hooks ==")
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
