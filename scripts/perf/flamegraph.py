#!/usr/bin/env python3
"""对运行中的 gloss 实例做 CPU 采样并产出 inferno 火焰图 SVG。

链路：定位 gloss 进程（或用 --pid）→ sudo dtrace profile 探针按 --rate Hz
采用户态调用栈 --duration 秒 → inferno-collapse-dtrace 折叠 →（装了 rustfilt
则就地解 Rust mangling）→ inferno-flamegraph 渲染 SVG。折叠与渲染交给
inferno，本脚本只做采集编排、栈顶自耗汇总与失败指引；从启动采到退出的
launch 模式走 `just flame-startup`（cargo-flamegraph）。

dtrace 采样需要 root：脚本经 sudo 运行 dtrace，非交互环境拿不到密码会当
场失败并给出指引。release 二进制 strip=true 无符号表，帧显示为
「二进制名`0x偏移」；函数级归因请先 `just profile-run` 以 profiling 配置
启动带符号实例再采。

用法：
  scripts/perf/flamegraph.py                     # 采 gloss 进程 10 秒
  scripts/perf/flamegraph.py --pid 1234 --duration 30
  scripts/perf/flamegraph.py --out /tmp/flame.svg
"""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

TOP_LIMIT = 10

# dtrace 对无符号帧输出「模块`0x偏移」，有符号帧输出「模块`符号+0x偏移」
UNSIZED_FRAME_RE = re.compile(r"`0x[0-9a-fA-F]+$")


def pid_arg(value: str) -> int:
    if not value:
        return 0
    try:
        return int(value)
    except ValueError:
        raise argparse.ArgumentTypeError("pid 必须是整数")


def default_pid() -> int:
    out = subprocess.run(
        ["pgrep", "-x", "gloss"], capture_output=True, text=True
    ).stdout.split()
    if not out:
        sys.exit("没有找到运行中的 gloss 实例：先启动应用，或用 --pid 指定进程")
    if len(out) > 1:
        sys.exit(f"发现多个 gloss 实例（{', '.join(out)}），用 --pid 指定其一")
    return int(out[0])


def require_tool(name: str) -> None:
    if shutil.which(name) is None:
        sys.exit(f"缺 {name} —— 安装：cargo install flamegraph inferno rustfilt --locked")


def require_alive(pid: int) -> None:
    proc = subprocess.run(["ps", "-p", str(pid)], capture_output=True, text=True)
    if proc.returncode != 0:
        sys.exit(f"pid {pid} 不存在或已退出：确认实例在跑，或用 pgrep -x gloss 查当前 pid")


def run_dtrace(pid: int, duration: int, rate: int, out: Path) -> None:
    program = (
        f"profile-{rate} /pid == {pid} && arg1/ {{ @[ustack(100)] = count(); }}\n"
        f"tick-{duration}s {{ exit(0); }}"
    )
    proc = subprocess.run(
        ["sudo", "dtrace", "-x", "ustackframes=100", "-n", program, "-o", str(out)],
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        err = proc.stderr.strip()
        if "password is required" in err or "additional privileges" in err:
            sys.exit(
                "dtrace 采样需要 root，当前环境没有交互 sudo——"
                "请在自己的终端里运行本命令（sudo 会提示输密码）"
            )
        sys.exit(f"dtrace 失败（exit {proc.returncode}）：{err}")


def demangle_inplace(folded: Path) -> None:
    if shutil.which("rustfilt") is None:
        print("提示：缺 rustfilt，帧名保留 Rust mangled 原样——安装：cargo install rustfilt --locked")
        return
    proc = subprocess.run(
        ["rustfilt"], input=folded.read_text(), capture_output=True, text=True
    )
    if proc.returncode != 0:
        print(
            f"警告：rustfilt 失败（exit {proc.returncode}）：{proc.stderr.strip()}——"
            "保留 mangled 帧名继续"
        )
        return
    folded.write_text(proc.stdout)


def collapse(raw: Path, folded: Path) -> None:
    with folded.open("w") as sink:
        proc = subprocess.run(
            ["inferno-collapse-dtrace", str(raw)],
            stdout=sink,
            stderr=subprocess.PIPE,
            text=True,
        )
    if proc.returncode != 0:
        sys.exit(f"inferno-collapse-dtrace 失败（exit {proc.returncode}）：{proc.stderr.strip()}")
    if not folded.read_text().strip():
        sys.exit("采样窗口内没有用户态 CPU 样本：进程可能全程空闲（或不在 CPU 上）")


def render(folded: Path, svg: Path) -> None:
    with svg.open("w") as sink:
        proc = subprocess.run(
            ["inferno-flamegraph", str(folded)],
            stdout=sink,
            stderr=subprocess.PIPE,
            text=True,
        )
    if proc.returncode != 0:
        sys.exit(f"inferno-flamegraph 失败（exit {proc.returncode}）：{proc.stderr.strip()}")


def top_of_stack(folded: Path) -> tuple[list[tuple[str, int]], int]:
    """折叠格式「帧1;帧2;…;叶子帧 N」的叶子帧即栈顶：自耗 = 各行叶子计数之和。"""
    self_counts: dict[str, int] = {}
    total = 0
    for line in folded.read_text().splitlines():
        stack, sep, count_s = line.rpartition(" ")
        if not sep or not count_s.isdigit() or not stack:
            continue
        count = int(count_s)
        total += count
        leaf = stack.rsplit(";", 1)[-1]
        self_counts[leaf] = self_counts.get(leaf, 0) + count
    ranked = sorted(self_counts.items(), key=lambda kv: kv[1], reverse=True)
    return ranked[:TOP_LIMIT], total


def main() -> None:
    parser = argparse.ArgumentParser(description="dtrace + inferno 火焰图（attach 采集）")
    parser.add_argument("--pid", type=pid_arg, default=0, help="目标进程（缺省自动找 gloss）")
    parser.add_argument("--duration", type=int, default=10, help="采样时长秒")
    parser.add_argument("--rate", type=int, default=997, help="采样频率 Hz")
    parser.add_argument("--out", default="", help="SVG 输出路径")
    args = parser.parse_args()

    require_tool("inferno-collapse-dtrace")
    require_tool("inferno-flamegraph")
    pid = args.pid or default_pid()
    require_alive(pid)

    stamp = time.strftime("%Y%m%d-%H%M%S")
    svg = Path(args.out) if args.out else Path("target/profile") / f"flamegraph-{stamp}.svg"
    svg.parent.mkdir(parents=True, exist_ok=True)
    raw = svg.with_suffix(".svg.dtrace.txt")
    folded = svg.with_suffix(".folded.txt")

    print(f"采集中：pid {pid}，{args.duration}s @ {args.rate}Hz（dtrace 经 sudo 运行，可能提示输密码）")
    run_dtrace(pid, args.duration, args.rate, raw)
    collapse(raw, folded)
    demangle_inplace(folded)
    render(folded, svg)

    top, total = top_of_stack(folded)
    print(f"火焰图：{svg}")
    print(f"原始栈：{raw}")
    print(f"折叠数据：{folded}")
    print(f"\n栈顶自耗 Top {len(top)}（共 {total} samples）：")
    for name, count in top:
        print(f"  {100.0 * count / max(total, 1):5.1f}%  {name}")

    frames: list[str] = []
    for line in folded.read_text().splitlines():
        stack, sep, count_s = line.rpartition(" ")
        if not sep or not count_s.isdigit() or not stack:
            continue
        frames.extend(stack.split(";"))
    if frames:
        unsized = sum(1 for frame in frames if UNSIZED_FRAME_RE.search(frame))
        if unsized / len(frames) > 0.6:
            print(
                "\n提示：多数帧未符号化（模块`0x偏移）——实例二进制被 strip；"
                "用 `just profile-run` 以 profiling 配置启动带符号实例后再采"
            )


if __name__ == "__main__":
    main()
