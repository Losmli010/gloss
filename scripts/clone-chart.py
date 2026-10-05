#!/usr/bin/env python3
"""把 clone 基线 JSON 渲染为确定性 SVG 图表。

三块面板：各 crate 生产代码 `.clone()` 计数（含密度）、criterion clone 基准
的分配次数/迭代、分配字节/迭代（对数刻度——字节跨三个数量级）。布局按
数据排序推导，不含时间戳，同一基线重绘逐字节一致。

用法：
  scripts/clone-chart.py [输出路径]   # 缺省写 scripts/baselines/clone-stats.svg
"""

import json
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
BASELINE_PATH = SCRIPT_DIR / "baselines" / "clone-stats.json"

WIDTH = 960
LABEL_X = 16
BAR_X = 270
BAR_MAX_W = 460
VALUE_X = 744
BAR_H = 24
ROW_STEP = 36
PANEL_GAP = 34
TITLE_H = 30
SUBTITLE_H = 20
CAPTION_H = 26
BOTTOM_PAD = 30

COLOR_BAR = "#5b7db1"
COLOR_BAR_MAX = "#3d5a8a"
COLOR_TEXT = "#333333"
COLOR_MUTED = "#8a8f98"
COLOR_TRACK = "#eceff3"


def esc(text):
    return (
        str(text)
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
    )


def fmt_int(value):
    return f"{value:,}"


class Svg:
    def __init__(self, width):
        self.width = width
        self.parts = []
        self.y = 0

    def text(self, x, y, content, size=13, color=COLOR_TEXT, bold=False, anchor="start"):
        weight = "600" if bold else "400"
        self.parts.append(
            f'<text x="{x}" y="{y}" font-size="{size}" fill="{color}" '
            f'font-weight="{weight}" text-anchor="{anchor}">{esc(content)}</text>'
        )

    def rect(self, x, y, w, h, color, rx=3):
        self.parts.append(
            f'<rect x="{x}" y="{y}" width="{max(w, 0):.1f}" height="{h}" '
            f'fill="{color}" rx="{rx}"/>'
        )

    def panel_title(self, title, subtitle):
        self.y += TITLE_H
        self.text(LABEL_X, self.y, title, size=15, bold=True)
        self.y += SUBTITLE_H
        self.text(LABEL_X, self.y, subtitle, size=11.5, color=COLOR_MUTED)
        self.y += 10

    def rows(self, items, scale, value_fmt):
        """items: [(label, value, value_text)]，scale(value)→条宽。"""
        max_value = max((v for _, v, _ in items), default=0)
        for label, value, value_text in items:
            self.y += ROW_STEP - 12
            bar_color = COLOR_BAR_MAX if value == max_value else COLOR_BAR
            self.rect(LABEL_X, self.y, BAR_X + BAR_MAX_W - LABEL_X, BAR_H, COLOR_TRACK, rx=4)
            self.rect(BAR_X, self.y, scale(value), BAR_H, bar_color, rx=4)
            self.text(LABEL_X, self.y + BAR_H - 7, label, size=12)
            self.text(VALUE_X, self.y + BAR_H - 7, value_text, size=12, bold=True)
            self.y += 12
        self.y += CAPTION_H - 12


def linear_scale(max_value):
    return lambda v: BAR_MAX_W * v / max_value if max_value else 0


def log_scale(values):
    positive = [v for v in values if v > 0]
    if not positive:
        return linear_scale(1)
    lo, hi = min(positive), max(positive)
    import math

    span = math.log10(hi / lo) if hi > lo else 1.0
    return lambda v: BAR_MAX_W * (math.log10(v / lo) / span if v > 0 and span else 1.0)


def build_svg(baseline):
    crates = baseline.get("crates", {})
    perf = baseline.get("perf", {})

    crate_rows = sorted(
        ((name, agg["clone"], agg["loc"]) for name, agg in crates.items()),
        key=lambda t: (-t[1], t[0]),
    )
    total = sum(c for _, c, _ in crate_rows)
    total_loc = sum(loc for _, _, loc in crate_rows)

    alloc_rows = sorted(
        (
            (bid[len("clone_allocs/") :], entry["allocs"])
            for bid, entry in perf.items()
            if "allocs" in entry
        ),
        key=lambda t: (-t[1], t[0]),
    )
    byte_rows = sorted(
        (
            (bid[len("clone_bytes/") :], entry["bytes"])
            for bid, entry in perf.items()
            if "bytes" in entry
        ),
        key=lambda t: (-t[1], t[0]),
    )

    svg = Svg(WIDTH)
    svg.y = 26
    svg.text(LABEL_X, svg.y, "clone 基线", size=19, bold=True)
    svg.y += 20
    density = total / total_loc * 1000 if total_loc else 0
    svg.text(
        LABEL_X,
        svg.y,
        f"生产代码 .clone() 合计 {total} 次 · 密度 {density:.1f} 次/千行 · "
        f"性能基准 {len(alloc_rows)} 项（scripts/baselines/clone-stats.json）",
        size=12,
        color=COLOR_MUTED,
    )

    svg.panel_title(
        "一、各 crate 生产代码 .clone() 计数",
        "口径：src/ + src/main.rs，剥离注释/字符串、剔除 #[cfg(test)]；条宽按计数线性",
    )
    max_count = max((c for _, c, _ in crate_rows), default=0)
    svg.rows(
        [
            (name, count, f"{count} · {count / loc * 1000:.1f}/千行" if loc else f"{count}")
            for name, count, loc in crate_rows
        ],
        linear_scale(max_count),
        lambda v: f"{v}",
    )

    svg.panel_title(
        "二、热点分配次数 / 迭代",
        "benches/clone.rs（criterion 自定义测量，均值）；条宽线性",
    )
    svg.rows(
        [(name, value, f"{value} 次/迭代") for name, value in alloc_rows],
        linear_scale(max((v for _, v in alloc_rows), default=0)),
        lambda v: f"{v}",
    )

    svg.panel_title(
        "三、热点分配字节 / 迭代",
        "同一基准的字节口径；跨三个数量级，条宽对数刻度",
    )
    svg.rows(
        [(name, value, f"{value:,} 字节/迭代") for name, value in byte_rows],
        log_scale([v for _, v in byte_rows]),
        lambda v: fmt_int(v),
    )

    height = svg.y + BOTTOM_PAD
    head = (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{height}" '
        f'viewBox="0 0 {WIDTH} {height}" font-family="-apple-system, '
        f'PingFang SC, Hiragino Sans GB, Microsoft YaHei, sans-serif">'
    )
    return "\n".join([head, *svg.parts, "</svg>"]) + "\n"


def main():
    out_path = Path(sys.argv[1]) if len(sys.argv) > 1 else SCRIPT_DIR / "baselines" / "clone-stats.svg"
    baseline = json.loads(BASELINE_PATH.read_text(encoding="utf-8"))
    svg = build_svg(baseline)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(svg, encoding="utf-8")
    print(f"图表已生成：{out_path.relative_to(out_path.parent.parent.parent)}")


if __name__ == "__main__":
    main()
