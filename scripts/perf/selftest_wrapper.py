#!/usr/bin/env python3
"""L3 双自检 wrapper：跑显隐+启动自检，收集 stderr 结构化信号行与完整 RSS 曲线。

流程：cargo test --no-run 构建两个自检二进制 → 依次直接运行 overlay 与
startup（自检只读 GLOSS_PERF_COMMIT 环境变量，wrapper 负责注入短 commit）
→ overlay 运行期间按固定间隔轮询 ps 采本进程 RSS（完整曲线，趋势用）
→ 从 stderr 提取结构化信号行：kind=overlay_round / kind=overlay_perf 的
JSON 行原样保留；startup 提取 message=startup self-test finished 的 JSON 行；
非信号 stderr 原样回显到本脚本 stderr（error! 诊断随退出码一起可见）
→ 按采集顺序落 out（缺省 target/perf/selftest.jsonl，本地 handoff，不入
git），RSS 曲线补一条 kind=rss_curve 行（samples 为 [自进程启动的毫秒,
RSS kb] 数组）。

退出码：任一自检失败、构建失败、或自检退出 0 但预期信号行缺失，退出 1
（数据先落盘再判失败）；自检自身的预算门禁判定（首帧/RSS 尾段/启动预算）
不经 wrapper 改写。

用法：
  scripts/perf/selftest_wrapper.py
  scripts/perf/selftest_wrapper.py --out /tmp/selftest.jsonl --poll-ms 50
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
        raise SystemExit(f"构建 {test} 自检二进制失败")
    executables = [
        message["executable"]
        for line in proc.stdout.splitlines()
        if (message := _parse_json(line)) is not None
        and message.get("reason") == "compiler-artifact"
        and message.get("target", {}).get("name") == test
        and message.get("executable")
    ]
    if not executables:
        raise SystemExit(f"cargo 未报告 {test} 自检二进制路径")
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
    return proc.returncode, [
        line for line in raw_lines if signal_kind(line) in OVERLAY_SIGNAL_KINDS
    ], samples


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
        help="信号输出路径（相对仓库根，缺省 target/perf/selftest.jsonl）",
    )
    parser.add_argument("--poll-ms", type=int, default=50, help="RSS 轮询间隔毫秒")
    args = parser.parse_args()

    env = selftest_env()
    overlay_rc, overlay_lines, samples = run_overlay(build_test_binary("overlay"), env, args.poll_ms)
    startup_rc, startup_lines = run_startup(build_test_binary("startup"), env)

    commit = env["GLOSS_PERF_COMMIT"]
    records = overlay_lines + [json.dumps({"kind": "rss_curve", "commit": commit, "samples": samples})] + startup_lines
    out = Path(args.out)
    if not out.is_absolute():
        out = ROOT / out
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("\n".join(records) + "\n" if records else "")

    print(f"overlay: exit {overlay_rc}，信号 {sum(1 for line in overlay_lines if signal_kind(line) == 'overlay_perf')} 条 / round 标记 {sum(1 for line in overlay_lines if signal_kind(line) == 'overlay_round')} 条 / RSS 采样 {len(samples)} 点")
    print(f"startup: exit {startup_rc}，信号 {len(startup_lines)} 条")
    print(f"已写 {len(records)} 行 → {out}")

    failed = overlay_rc != 0 or startup_rc != 0
    if overlay_rc == 0 and not any(signal_kind(line) == "overlay_perf" for line in overlay_lines):
        print("overlay 退出 0 但未提取到 overlay_perf 信号行", file=sys.stderr)
        failed = True
    if startup_rc == 0 and not startup_lines:
        print("startup 退出 0 但未提取到启动信号行", file=sys.stderr)
        failed = True
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
