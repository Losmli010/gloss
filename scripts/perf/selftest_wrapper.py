#!/usr/bin/env python3
"""显隐与启动双自检（L3）的包装器：跑两个自检目标，收集标准错误上的结构化
信号行与完整常驻内存（RSS）曲线。

流程：先以测试构建模式编译两个自检二进制 → 依次直接运行显隐与启动自检
（自检只读提交号环境变量，包装器负责注入短提交号）→ 显隐自检运行期间按
固定间隔轮询进程表采本进程常驻内存（完整曲线，趋势用）→ 从标准错误提取
结构化（JSON）信号行：显隐轮次与显隐汇总两类信号行原样保留，启动自检提
取完成信号行；非信号的标准错误原样回显（错误诊断随退出码一起可见）→ 按
采集顺序落输出文件（缺省在构建产物目录下，本地交接产物，不入版本库），
常驻内存曲线补一条曲线信号行（样本是自进程启动起的毫秒数与千字节值数
组）。

退出码：任一自检失败、构建失败、或自检退出码为零但预期信号行缺失，退出
码为一（数据先落盘再判失败）；自检自身的预算门禁判定（首帧、常驻内存尾
段与启动预算）不经包装器改写。
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
ROOT = SCRIPT_DIR.parents[1]

STARTUP_SIGNAL_MESSAGE = "startup self-test finished"
OVERLAY_SIGNAL_KINDS = ("overlay_round", "overlay_perf")


def build_test_binary(test: str) -> Path:
    proc = subprocess.run(
        ["cargo", "test", "-p", "gloss", "--test", test, "--no-run", "--message-format=json"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr)
        raise SystemExit(f"failed to build the {test} selftest binary")
    executables = [
        message["executable"]
        for line in proc.stdout.splitlines()
        if (message := _parse_json(line)) is not None
        and message.get("reason") == "compiler-artifact"
        and message.get("target", {}).get("name") == test
        and message.get("executable")
    ]
    if not executables:
        raise SystemExit(f"cargo did not report a {test} selftest binary path")
    return Path(executables[-1])


def _parse_json(line: str):
    try:
        return json.loads(line)
    except json.JSONDecodeError:
        return None


def selftest_env() -> dict:
    commit = subprocess.run(
        ["git", "rev-parse", "--short", "HEAD"], cwd=ROOT, capture_output=True, text=True
    ).stdout.strip()
    env = os.environ.copy()
    env["GLOSS_PERF_COMMIT"] = commit
    env["RUST_LOG"] = env.get("RUST_LOG") or "info"
    return env


def signal_kind(line: str):
    message = _parse_json(line)
    if not isinstance(message, dict):
        return None
    return message.get("kind") or message.get("message")


def run_overlay(binary: Path, env: dict, poll_ms: int) -> tuple[int, list[str], list[list[int]]]:
    with tempfile.TemporaryFile(mode="w+") as stderr_file:
        started = time.monotonic()
        proc = subprocess.Popen(
            [str(binary)],
            cwd=ROOT,
            stdout=subprocess.DEVNULL,
            stderr=stderr_file,
            env=env,
        )
        samples = []
        while proc.poll() is None:
            kb = sample_rss_kb(proc.pid)
            elapsed_ms = round((time.monotonic() - started) * 1000)
            if kb is not None:
                samples.append([elapsed_ms, kb])
            time.sleep(poll_ms / 1000.0)
        stderr_file.seek(0)
        raw_lines = [line.strip() for line in stderr_file.read().splitlines()]
    for line in raw_lines:
        if signal_kind(line) not in OVERLAY_SIGNAL_KINDS:
            print(line, file=sys.stderr)
    return (
        proc.returncode,
        [line for line in raw_lines if signal_kind(line) in OVERLAY_SIGNAL_KINDS],
        samples,
    )


def run_startup(binary: Path, env: dict) -> tuple[int, list[str]]:
    proc = subprocess.run(
        [str(binary)],
        cwd=ROOT,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
        env=env,
    )
    raw_lines = [line.strip() for line in proc.stderr.splitlines()]
    for line in raw_lines:
        if signal_kind(line) != STARTUP_SIGNAL_MESSAGE:
            print(line, file=sys.stderr)
    return proc.returncode, [
        line for line in raw_lines if signal_kind(line) == STARTUP_SIGNAL_MESSAGE
    ]


def sample_rss_kb(pid: int) -> int | None:
    proc = subprocess.run(["ps", "-o", "rss=", "-p", str(pid)], capture_output=True, text=True)
    try:
        return int(proc.stdout.strip())
    except ValueError:
        return None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--out",
        default="target/perf/selftest.jsonl",
        help="signal output path (repo-root relative, default target/perf/selftest.jsonl)",
    )
    parser.add_argument("--poll-ms", type=int, default=50, help="RSS poll interval ms")
    args = parser.parse_args()

    env = selftest_env()
    overlay_rc, overlay_lines, samples = run_overlay(
        build_test_binary("overlay"), env, args.poll_ms
    )
    startup_rc, startup_lines = run_startup(build_test_binary("startup"), env)

    commit = env["GLOSS_PERF_COMMIT"]
    records = (
        overlay_lines
        + [json.dumps({"kind": "rss_curve", "commit": commit, "samples": samples})]
        + startup_lines
    )
    out = Path(args.out)
    if not out.is_absolute():
        out = ROOT / out
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("\n".join(records) + "\n" if records else "")

    print(
        f"overlay: exit {overlay_rc}, perf signals {sum(1 for line in overlay_lines if signal_kind(line) == 'overlay_perf')} / round markers {sum(1 for line in overlay_lines if signal_kind(line) == 'overlay_round')} / RSS samples {len(samples)}"
    )
    print(f"startup: exit {startup_rc}, signals {len(startup_lines)}")
    print(f"wrote {len(records)} lines -> {out}")

    failed = overlay_rc != 0 or startup_rc != 0
    if overlay_rc == 0 and not any(signal_kind(line) == "overlay_perf" for line in overlay_lines):
        print("overlay exited 0 but no overlay_perf signal line extracted", file=sys.stderr)
        failed = True
    if startup_rc == 0 and not startup_lines:
        print("startup exited 0 but no startup signal line extracted", file=sys.stderr)
        failed = True
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
