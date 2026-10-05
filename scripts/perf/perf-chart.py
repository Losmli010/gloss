#!/usr/bin/env python3
"""把性能基线历史渲染为确定性 SVG 趋势曲线。

数据源是 baselines/history.jsonl（perf-history.py 每次 perf-baseline 追加
一个快照点）。两张图：clone-stats.svg 画 clone 的整体聚合量（总数、密度、
热点分配次数/字节合计）四条趋势；core-baseline.svg 按基准组分面板画各组
内基准的均值曲线（对数纵轴），页头带最新快照的运行环境。同一 JSON 重绘
逐字节一致。

用法：
  scripts/perf/perf-chart.py    依据 history.jsonl 重绘两张趋势图
"""

import json
import math
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
BASELINE_DIR = SCRIPT_DIR / "baselines"
HISTORY_PATH = BASELINE_DIR / "history.jsonl"

WIDTH = 960
PLOT_X0 = 70
PLOT_X1 = 900
PLOT_H = 90
TITLE_H = 30
SUBTITLE_H = 20
PANEL_GAP = 22
BOTTOM_PAD = 26

COLOR_LINE = "#3d5a8a"
COLOR_TEXT = "#333333"
COLOR_MUTED = "#8a8f98"
COLOR_TRACK = "#f2f4f7"
PALETTE = ("#3d5a8a", "#c26b4a", "#4a8f6f", "#8a5fb0", "#a8842c", "#4a90a4")

CORE_GROUPS = ("cache_key", "complete", "prompt_render", "task_cache")

FONT_FAMILY = "-apple-system, PingFang SC, Hiragino Sans GB, Microsoft YaHei, sans-serif"


def esc(text):
    return (
        str(text)
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
    )


def human_ns(ns):
    if ns >= 1_000_000:
        return f"{ns / 1_000_000:.3f} ms"
    if ns >= 1_000:
        return f"{ns / 1_000:.2f} µs"
    return f"{ns:.1f} ns"


def fmt_int(value):
    return f"{value:,}"


class Svg:
    def __init__(self):
        self.parts = []
        self.y = 0
        self.height = 0

    def text(self, x, y, content, size=13, color=COLOR_TEXT, bold=False, anchor="start", halo=False):
        if halo:
            w = len(content) * size * 0.58 + 8
            rx = x - w / 2 if anchor == "middle" else x - 3
            self.parts.append(
                f'<rect x="{rx:.1f}" y="{y - size * 0.78:.1f}" width="{w:.1f}" '
                f'height="{size * 1.05:.1f}" fill="#ffffff" fill-opacity="0.92" rx="2"/>'
            )
        weight = "600" if bold else "400"
        self.parts.append(
            f'<text x="{x:.1f}" y="{y:.1f}" font-size="{size}" fill="{color}" '
            f'font-weight="{weight}" text-anchor="{anchor}">{esc(content)}</text>'
        )

    def rect(self, x, y, w, h, color, rx=3):
        self.parts.append(
            f'<rect x="{x:.1f}" y="{y:.1f}" width="{max(w, 0):.1f}" height="{h}" '
            f'fill="{color}" rx="{rx}"/>'
        )

    def circle(self, x, y, color, r=3.5):
        self.parts.append(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="{r}" fill="{color}"/>')

    def polyline(self, pts, color):
        points = " ".join(f"{x:.1f},{y:.1f}" for x, y in pts)
        self.parts.append(
            f'<polyline points="{points}" fill="none" stroke="{color}" stroke-width="2"/>'
        )

    def panel_title(self, title, subtitle):
        self.y += TITLE_H
        self.text(16, self.y, title, size=15, bold=True)
        self.y += SUBTITLE_H
        self.text(16, self.y, subtitle, size=11.5, color=COLOR_MUTED)
        self.y += 8

    def render(self, title_text):
        head = (
            f'<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{self.height}" '
            f'viewBox="0 0 {WIDTH} {self.height}" font-family="{FONT_FAMILY}">'
        )
        self.text(16, 26, title_text, size=19, bold=True)
        return "\n".join([head, *self.parts, "</svg>"]) + "\n"


def x_positions(count):
    if count == 1:
        return [(PLOT_X0 + PLOT_X1) / 2]
    step = (PLOT_X1 - PLOT_X0) / (count - 1)
    return [PLOT_X0 + i * step for i in range(count)]


def y_scale(values, log):
    present = [v for v in values if v is not None]
    if not present:
        return lambda v: 0
    if log:
        present = [math.log10(v) for v in present]
    lo, hi = min(present), max(present)
    if lo == hi:
        lo, hi = lo - 1, hi + 1
    pad = (hi - lo) * 0.18

    def scale(v):
        t = (math.log10(v) if log else v) - lo
        return PLOT_H - (t / (hi + pad - lo)) * PLOT_H

    return scale


def line_panel(svg, title, subtitle, points, series, fmt, log=False, legend=False):
    """points: [(commit, None)]；series: [(name, color, [v or None])]，与 points 对齐。"""
    svg.panel_title(title, subtitle)
    plot_y = svg.y
    commits = [c for c, _ in points]
    xs = x_positions(len(points))
    svg.rect(PLOT_X0 - 8, plot_y, PLOT_X1 - PLOT_X0 + 16, PLOT_H, COLOR_TRACK, rx=4)

    flat = [v for _, _, vals in series for v in vals]
    scale = y_scale(flat, log)
    labels = []
    for name, color, vals in series:
        pts = [(xs[i], plot_y + scale(v)) for i, v in enumerate(vals) if v is not None]
        if len(pts) > 1:
            svg.polyline(pts, color)
        for x, y in pts:
            svg.circle(x, y, color)
        if pts:
            lx, ly = pts[-1]
            last_val = next(v for v in reversed(vals) if v is not None)
            labels.append([lx, ly - 9, color, fmt(last_val)])
    by_x = {}
    for lab in labels:
        by_x.setdefault(round(lab[0]), []).append(lab)
    for group in by_x.values():
        group.sort(key=lambda t: -t[1])
        for i in range(1, len(group)):
            group[i][1] = min(group[i][1], group[i - 1][1] - 13)
        for lx, ly, color, text in group:
            svg.text(lx, ly, text, size=11, color=color, bold=True, anchor="middle", halo=True)
    tick_y = plot_y + PLOT_H + 14
    for x, commit in zip(xs, commits):
        svg.text(x, tick_y, commit, size=10, color=COLOR_MUTED, anchor="middle")
    svg.y = tick_y + 6
    if legend:
        rows = (len(series) + 3) // 4
        for idx, (name, color, _) in enumerate(series):
            col_i, row_i = idx % 4, idx // 4
            lx = 24 + col_i * 228
            ly = svg.y + 14 + row_i * 16
            svg.rect(lx, ly - 9, 10, 10, color, rx=2)
            svg.text(lx + 14, ly, name, size=10.5, color=COLOR_TEXT)
        svg.y += rows * 16 + 6
    svg.y += PANEL_GAP


def load_history():
    if not HISTORY_PATH.is_file():
        print("错误：缺少 history.jsonl，先运行 just perf-baseline", file=sys.stderr)
        raise SystemExit(1)
    return [
        json.loads(line)
        for line in HISTORY_PATH.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]


def build_clone_svg(history, clone_baseline):
    points = [(e["commit"], None) for e in history]

    def col(key):
        return [e.get("clone", {}).get(key) for e in history]

    svg = Svg()
    svg.y = 46
    last = history[-1]["clone"]
    crates = clone_baseline.get("crates", {})
    composition = " · ".join(
        f"{name} {agg['clone']}"
        for name, agg in sorted(crates.items(), key=lambda kv: -kv[1]["clone"])
    )
    svg.text(
        16,
        svg.y,
        f"当前：合计 {last['total']} 次 · 密度 {last['density']} 次/千行 · "
        f"热点分配合计 {last['allocs']} 次 / {fmt_int(last['bytes'])} 字节",
        size=12,
        color=COLOR_MUTED,
    )
    svg.y += 18
    svg.text(
        16,
        svg.y,
        f"构成：{composition}（明细见 scripts/perf/baselines/clone-stats.json）",
        size=12,
        color=COLOR_MUTED,
    )

    line_panel(
        svg,
        "一、clone 总数趋势",
        "整体聚合；x 轴为基线快照（just perf-baseline 追加，标注 commit）",
        points,
        [("总数", COLOR_LINE, col("total"))],
        lambda v: f"{v} 次",
    )
    line_panel(
        svg,
        "二、密度趋势",
        "次/千行生产代码，剔除规模变化后的公平口径",
        points,
        [("密度", COLOR_LINE, col("density"))],
        lambda v: f"{v}/千行",
    )
    line_panel(
        svg,
        "三、热点分配次数合计",
        "六项热点基准每迭代分配之和，作整体趋势指数",
        points,
        [("allocs", COLOR_LINE, col("allocs"))],
        lambda v: f"{v} 次",
    )
    line_panel(
        svg,
        "四、热点分配字节合计",
        "六项热点基准每迭代分配字节之和，作整体趋势指数",
        points,
        [("bytes", COLOR_LINE, col("bytes"))],
        lambda v: f"{fmt_int(v)} B",
    )

    svg.height = svg.y + BOTTOM_PAD
    return svg.render("clone 基线（整体趋势）")


def build_core_svg(history, core_baseline):
    benches = history[-1].get("core") or {}
    env_src = (core_baseline or {}).get("env", {})

    grouped = {group: [] for group in CORE_GROUPS}
    for full_id in benches:
        group = full_id.split("/", 1)[0]
        grouped.setdefault(group, []).append(full_id)

    svg = Svg()
    svg.y = 46
    svg.text(
        16,
        svg.y,
        f"{len(benches)} 项 · 组内对数纵轴 · 只记录不判罚"
        "（scripts/perf/baselines/core-baseline.json）",
        size=12,
        color=COLOR_MUTED,
    )
    svg.y += 18
    svg.text(
        16,
        svg.y,
        " · ".join(
            str(env_src.get(key, "-")) for key in ("generated_at", "os", "machine")
        ),
        size=11.5,
        color=COLOR_MUTED,
    )
    svg.y += 16
    svg.text(
        16,
        svg.y,
        " · ".join(
            str(env_src.get(key, "-")) for key in ("cpu", "rustc", "commit")
        ),
        size=11.5,
        color=COLOR_MUTED,
    )

    for group in list(CORE_GROUPS) + sorted(set(grouped) - set(CORE_GROUPS)):
        ids = sorted(grouped.get(group) or [])
        if not ids:
            continue
        series = [
            (
                full_id[len(group) + 1 :],
                PALETTE[i % len(PALETTE)],
                [e.get("core", {}).get(full_id) if e.get("core") else None for e in history],
            )
            for i, full_id in enumerate(ids)
        ]
        line_panel(
            svg,
            f"{group}",
            "组内各基准均值，对数纵轴；标注为该线最新值",
            [(e["commit"], None) for e in history],
            series,
            human_ns,
            log=True,
            legend=True,
        )

    svg.height = svg.y + BOTTOM_PAD
    return svg.render("core 基线（趋势）")


def main():
    history = load_history()
    clone_baseline = json.loads((BASELINE_DIR / "clone-stats.json").read_text(encoding="utf-8"))
    core_path = BASELINE_DIR / "core-baseline.json"
    core_baseline = json.loads(core_path.read_text(encoding="utf-8")) if core_path.is_file() else None

    outputs = (
        ("clone-stats.svg", build_clone_svg(history, clone_baseline)),
        ("core-baseline.svg", build_core_svg(history, core_baseline)),
    )
    for name, content in outputs:
        path = BASELINE_DIR / name
        path.write_text(content, encoding="utf-8")
        print(f"图表已生成：{path.relative_to(path.parents[2])}")


if __name__ == "__main__":
    main()
