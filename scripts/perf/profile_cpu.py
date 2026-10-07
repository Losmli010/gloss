#!/usr/bin/env python3
"""对运行中的 gloss 实例做 CPU 采样并产出自包含火焰图 SVG。

链路：定位 gloss 进程（或用 --pid）→ macOS 自带 /usr/bin/sample 按
--interval 毫秒采样 --duration 秒 → 解析「Call graph」调用树 → 渲染成
经典底起火焰图（宽 ∝ 采样数，悬停显示函数/采样数/占比）。零第三方依
赖：sample 是系统自带，渲染是本脚本内置 SVG。

release 二进制 strip=true 无符号表，采样帧会显示 ???（二进制偏移）；
函数级归因请先 `just profile-run` 以 profiling 配置启动带符号实例再采。

用法：
  scripts/perf/profile_cpu.py                     # 采 gloss 进程 10 秒
  scripts/perf/profile_cpu.py --pid 1234 --duration 30
  scripts/perf/profile_cpu.py --out /tmp/flame.svg
"""

from __future__ import annotations

import argparse
import hashlib
import html
import re
import subprocess
import sys
import time
from pathlib import Path

ROW_H = 16
CANVAS_W = 1280
HEADER_H = 44
FONT_SIZE = 11

LINE_RE = re.compile(r"^(?P<pre>[ +!|:]*?)(?P<count>\d+)\s+(?P<label>\S.*?)\s*$")
FRAME_RE = re.compile(
    r"^(?P<name>.+?)\s+\(in (?P<img>[^)]+)\)\s+(?:load address 0x[0-9a-fA-F]+ \+ |\+ )(?P<off>\S+)"
)
TOP_RE = re.compile(r"^\s+(?P<label>\S.*?)\s{2,}(?P<count>\d+)\s*$")


class Node:
    __slots__ = ("name", "count", "children")

    def __init__(self, name: str, count: int) -> None:
        self.name = name
        self.count = count
        self.children: list[Node] = []


def default_pid() -> int:
    out = subprocess.run(
        ["pgrep", "-x", "gloss"], capture_output=True, text=True
    ).stdout.split()
    if not out:
        sys.exit("没有找到运行中的 gloss 实例：先启动应用，或用 --pid 指定进程")
    if len(out) > 1:
        sys.exit(f"发现多个 gloss 实例（{', '.join(out)}），用 --pid 指定其一")
    return int(out[0])


def run_sample(pid: int, duration: int, interval: int) -> str:
    proc = subprocess.run(
        ["/usr/bin/sample", str(pid), str(duration), str(interval)],
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0 or "Call graph:" not in proc.stdout:
        sys.exit(f"sample 失败（exit {proc.returncode}）：{proc.stderr.strip()}")
    return proc.stdout


def clean_label(raw: str) -> str:
    if raw.startswith("Thread_"):
        return re.sub(r"\s{2,}", " ", raw)
    match = FRAME_RE.match(raw)
    if match:
        return f"{match.group('name').strip()} [{match.group('img')}+{match.group('off')}]"
    return re.sub(r"\s{2,}", " ", raw)


def parse_call_graph(report: str) -> list[Node]:
    lines = report.splitlines()
    start = next(i for i, line in enumerate(lines) if line == "Call graph:")
    roots: list[Node] = []
    stack: list[tuple[int, Node]] = []
    base = 0
    for line in lines[start + 1 :]:
        if not line.strip() or line.startswith("Total number in stack"):
            break
        match = LINE_RE.match(line)
        if not match:
            continue
        if not stack and not roots:
            base = len(match.group("pre"))
        depth = (len(match.group("pre")) - base) // 2
        if depth < 0:
            continue
        node = Node(clean_label(match.group("label")), int(match.group("count")))
        while stack and stack[-1][0] >= depth:
            stack.pop()
        if stack:
            stack[-1][1].children.append(node)
        else:
            roots.append(node)
        stack.append((depth, node))
    if not roots:
        sys.exit("sample 报告里没有可解析的 Call graph")
    return roots


def parse_top_of_stack(report: str) -> list[tuple[str, int]]:
    inside = False
    rows = []
    for line in report.splitlines():
        if line.startswith("Sort by top of stack"):
            inside = True
            continue
        if inside:
            if line.startswith("Binary Images"):
                break
            match = TOP_RE.match(line)
            if match:
                rows.append((clean_label(match.group("label")), int(match.group("count"))))
    return rows


def frame_color(name: str) -> str:
    digest = hashlib.md5(name.encode()).digest()
    return f"hsl({digest[0] % 45 + 8},{55 + digest[1] % 30}%,{45 + digest[2] % 20}%)"


def layout(roots: list[Node], total: int) -> tuple[list[list[tuple[float, float, int, str]]], int]:
    """按深度分层铺所有线程的矩形：rows[depth] = [(x, w, count, name)]。"""
    rows: list[list[tuple[float, float, int, str]]] = []
    max_depth = 0
    pending = []
    x0 = 0.0
    for root in roots:
        width = CANVAS_W * root.count / max(total, 1)
        pending.append((root, x0, width, 0))
        x0 += width
    while pending:
        node, x, width, depth = pending.pop()
        max_depth = max(max_depth, depth)
        while len(rows) <= depth:
            rows.append([])
        if width >= 0.2:
            rows[depth].append((x, width, node.count, node.name))
        child_x = x
        for child in node.children:
            child_w = width * child.count / max(node.count, 1)
            pending.append((child, child_x, child_w, depth + 1))
            child_x += child_w
    return rows, max_depth


def render_svg(rows: list[list[tuple[float, float, int, str]]], total: int, title: str) -> str:
    max_depth = len(rows) - 1
    height = HEADER_H + (max_depth + 1) * ROW_H + 8
    parts = [
        '<?xml version="1.0" encoding="UTF-8"?>',
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{CANVAS_W}" height="{height}"'
        f' viewBox="0 0 {CANVAS_W} {height}" font-family="monospace" font-size="{FONT_SIZE}">',
        '<rect width="100%" height="100%" fill="#1e1e28"/>',
        f'<text x="10" y="18" fill="#e8e8f0" font-size="13">{html.escape(title)}</text>',
        f'<text x="10" y="34" fill="#8888a0" font-size="10">{total} samples · '
        "宽度 ∝ CPU 采样数 · 悬停看占比</text>",
    ]
    for depth, cells in enumerate(rows):
        y = height - (depth + 1) * ROW_H
        for x, w, count, name in cells:
            parts.append(
                f"<g><title>{html.escape(name)} — {count} samples"
                f"（{100.0 * count / total:.1f}%）</title>"
                f'<rect x="{x:.1f}" y="{y}" width="{w:.1f}" height="{ROW_H - 1}"'
                f' fill="{frame_color(name)}" rx="1"/></g>'
            )
            if w > 28:
                label = name[: max(4, int(w / 6.2))]
                parts.append(
                    f'<text x="{x + 2:.1f}" y="{y + ROW_H - 4}" fill="#101018">'
                    f"{html.escape(label)}</text>"
                )
    parts.append("</svg>")
    return "\n".join(parts)


def pid_arg(value: str) -> int:
    if not value:
        return 0
    try:
        return int(value)
    except ValueError:
        raise argparse.ArgumentTypeError("pid 必须是整数")


def main() -> None:
    parser = argparse.ArgumentParser(description="sample + 自包含 SVG 火焰图")
    parser.add_argument("--pid", type=pid_arg, default=0, help="目标进程（缺省自动找 gloss）")
    parser.add_argument("--duration", type=int, default=10, help="采样时长秒")
    parser.add_argument("--interval", type=int, default=1, help="采样间隔毫秒")
    parser.add_argument("--out", default="", help="SVG 输出路径")
    args = parser.parse_args()

    pid = args.pid or default_pid()
    report = run_sample(pid, args.duration, args.interval)
    roots = parse_call_graph(report)
    total = sum(root.count for root in roots)
    if total == 0:
        sys.exit("采样为空：进程在采样窗口内没有在 CPU 上运行")

    rows, _ = layout(roots, total)
    stamp = time.strftime("%Y%m%d-%H%M%S")
    out_dir = Path("target/profile")
    out = Path(args.out) if args.out else out_dir / f"flamegraph-{stamp}.svg"
    out.parent.mkdir(parents=True, exist_ok=True)
    report_path = out.with_suffix(".svg.sample.txt")
    report_path.write_text(report)

    out.write_text(render_svg(rows, total, f"gloss pid {pid} — {args.duration}s · {stamp}"))

    top = parse_top_of_stack(report)[:10]
    print(f"火焰图：{out}")
    print(f"原始报告：{report_path}")
    print(f"\n栈顶自耗 Top {len(top)}（共 {total} samples）：")
    for name, count in top:
        print(f"  {100.0 * count / total:5.1f}%  {name}")

    cells = [cell for row in rows for cell in row]
    unknown = sum(1 for cell in cells if cell[3].startswith("???"))
    if cells and unknown / len(cells) > 0.6:
        print(
            "\n提示：多数帧未符号化（???）——实例二进制被 strip；"
            "用 `just profile-run` 以 profiling 配置启动带符号实例后再采"
        )


if __name__ == "__main__":
    main()
