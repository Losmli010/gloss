#!/usr/bin/env python3
"""性能趋势三图渲染器：把 v2 history.jsonl 按指标类别画成确定性 SVG。

数据源是 baselines/history.jsonl（v2 扁平点：metrics/env/budgets 由点自承载）。
三张图按指标类别组织（替代按数据源三图）：

  perf-latency.svg    响应性：启动耗时 / 划词→首帧 / 隐藏延迟与动画帧时间 /
                      外部依赖·LLM 延迟（仅记录不判罚）
  perf-resources.svg  资源与体积：稳态内存 / 显隐 RSS 净增长 / 处理器 /
                      二进制体积 / 窗口句柄数
  perf-hotpath.svg    热路径：core 四组分面板（组内对数轴）+ clone 分配合计
                      与 clone 计数密度

预算参考线取「最新携带预算的点」上的预算值——预算数值随自检 stderr 结构化
行流出、由统一采集器折算进点 budgets 段，任何环节不手写数值；常量名溯源走
metrics.py 注册表。预算状态 chip 只按注册表中受预算约束（judge=预算门禁）的
系列判定，同面板仅记录系列不参与；预算系列尚无测量值时不挂状态 chip（宁缺
勿谎），预算 chip 与红线照挂；history 尚无 budgets 段（预算值未入库）时挂
「预算未入库」chip，不挂「仅记录」也不画红线。指标 id、单位格式化（fmt）
全部取自 metrics.py 注册表，三张图的面板清单（含热路径派生面板）在 import
期逐 id 校验，未登记 id 当场失败。

面板是类别视图的固定结构：某指标尚无数据时面板照挂（图例、预算线照画），
绘图区标「暂无数据（等待采集并入）」；某源中途才入库时跳段不造点，绘图区
右上小字标注起始提交（与右侧高位的数据带相撞时避让到右下，见
_partial_note）。峰值类（rss_peak / footprint_peak）与 frame_missed 不入图，
仅在 perf-report 总览表呈现。

确定性字节输出：无时间戳、无集合遍历（渐变色收集后排序）、浮点坐标一律一位
小数——同一 history 两次渲染逐字节一致，PR diff 即数据 diff。

用法：
  scripts/perf/perf-chart.py                        读 baselines/history.jsonl 重绘三图
  scripts/perf/perf-chart.py --history H --out D    覆盖数据/输出目录（本地验收用）
"""

import argparse
import json
import math
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

from metrics import BUDGET_CONSTS, CORE_CASES, CORE_GROUPS, METRICS, fmt

DEFAULT_HISTORY = SCRIPT_DIR / "baselines" / "history.jsonl"
DEFAULT_OUT_DIR = SCRIPT_DIR / "baselines"

# ---- 页面骨架：960px 画布，每面板一张 920×212 白卡片（统一节奏）----
WIDTH = 960
CARD_X, CARD_W, CARD_H = 20, 920, 212
CARD_PITCH = 226
HEADER_H = 64
TITLE_DY = 21
CHIP_DY, CHIP_H = 8, 18
CHIP_RIGHT = CARD_X + CARD_W - 16
LEGEND_DY = 38.5
PLOT_TOP_DY, PLOT_H = 54, 118
PLOT_X0, PLOT_X1 = 62, 830
GRID_DIV = 4
GUTTER_DIV_X, GUTTER_TEXT_X = 842, 850
GUTTER_BLOCK_PITCH = 26
GUTTER_VALUE_TO_CHANGE = 12
GUTTER_BOTTOM_MARGIN = 2
X_SHA_DY, X_DATE_DY = 16, 28
FOOTER_GAP, BOTTOM_PAD = 26, 34

# ---- 热路径面板（沿用旧 core-baseline 形态：组内对数轴 + 线端标值 + 4 列图例）----
HP_PLOT_TOP_DY, HP_PLOT_H = 46, 104
HP_PLOT_X0, HP_PLOT_X1 = 62, 900
HP_X_SHA_DY, HP_X_DATE_DY = 16, 28
HP_LEGEND_ROW0_DY, HP_LEGEND_ROW_PITCH, HP_LEGEND_COL_PITCH = 194, 14, 214
HP_LABEL_TO_POINT = 9
HP_LABEL_DE_COLLIDE = 13
PARTIAL_NOTE_DY = 11

PALETTE = ("#2563eb", "#059669", "#d97706", "#7c3aed", "#0891b2", "#db2777")
COLOR_BG = "#f8fafc"
COLOR_CARD = "#ffffff"
COLOR_CARD_STROKE = "#e2e8f0"
COLOR_TRACK = "#f2f4f7"
COLOR_GRID = "#eef2f6"
COLOR_AXIS = "#e2e8f0"
COLOR_TITLE = "#0f172a"
COLOR_BODY = "#334155"
COLOR_MUTED = "#94a3b8"
COLOR_FAINT = "#cbd5e1"
COLOR_BUDGET = "#ef4444"
COLOR_DOWN = "#059669"
COLOR_UP = "#dc2626"
CHIP_NEUTRAL_BG = "#f1f5f9"
CHIP_BUDGET_FG = "#475569"
CHIP_RECORD_FG = "#64748b"
CHIP_PASS_BG, CHIP_PASS_FG = "#ecfdf5", "#047857"
CHIP_FAIL_BG, CHIP_FAIL_FG = "#fef2f2", "#dc2626"
FONT_FAMILY = "-apple-system,'PingFang SC','Helvetica Neue',sans-serif"

NICE_STEPS = (1, 1.2, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10)
KB_PER_MB = 1024.0
MB_AXIS_MIN_KB = 10 * KB_PER_MB
GRADIENT_OPACITY = 0.16
AREA_MIN_SPAN = 0.05
LOG_PAD = 0.18
EMPTY_MESSAGE = "暂无数据（等待采集并入）"
PARTIAL_MESSAGE = "部分系列自 {sha} 起有数据"
ENV_TAIL = "数据随基线采集逐点沉淀"
OS_LABELS = {"macos": "macOS", "linux": "Linux", "windows": "Windows"}

FOOTER_GUTTER = (
    "▼/▲ 较上一点变化（数值越低越好）· 缺源即缺段 · 峰值类与 frame_missed 仅报告不入图"
    " · 图表为 history.jsonl 的派生工件，由 perf-chart.py 生成"
)
FOOTER_HOTPATH = (
    "组内对数轴，线端标注为该线最新值 · 缺源即缺段 · 图表为 history.jsonl 的派生工件，"
    "由 perf-chart.py 生成"
)

# ---- 面板清单：类别视图的固定结构，逐 id 对照 metrics.py 注册表（import 期校验）----
LATENCY_PANELS = (
    {
        "title": "启动耗时",
        "suffix": "",
        "series": (
            ("app.startup_ms", "app.startup_ms"),
            ("startup.elapsed_ms", "startup.elapsed_ms"),
        ),
        "budget": "startup.elapsed_ms",
    },
    {
        "title": "划词 → 首帧",
        "suffix": "100 轮显隐自检",
        "series": (
            ("overlay.show_first_ms", "show_first_ms"),
            ("overlay.show_p50_ms", "show_p50_ms"),
            ("overlay.show_p95_ms", "show_p95_ms"),
        ),
        "budget": "overlay.show_first_ms",
    },
    {
        "title": "隐藏延迟与动画帧时间",
        "suffix": "",
        "series": (
            ("overlay.hide_p50_ms", "hide_p50_ms"),
            ("overlay.hide_max_ms", "hide_max_ms"),
            ("overlay.frame_p95_ms", "frame_p95_ms"),
        ),
        "budget": None,
    },
    {
        "title": "外部依赖 · LLM 任务端到端延迟",
        "suffix": "",
        "series": (
            ("eval.task_latency_p50_ms", "task_latency_p50_ms"),
            ("eval.task_latency_p95_ms", "task_latency_p95_ms"),
        ),
        "budget": None,
        "note": "仅记录不判罚 · 数据源：just eval（live 轨）· 波动来自外部服务",
    },
)
RESOURCES_PANELS = (
    {
        "title": "稳态内存",
        "suffix": "RSS 与 phys_footprint 双口径",
        "series": (
            ("app.rss_idle_kb", "app.rss_idle_kb"),
            ("app.footprint_idle_kb", "app.footprint_idle_kb"),
        ),
        "budget": None,
    },
    {
        "title": "显隐 100 轮 RSS 净增长",
        "suffix": "门禁看尾段",
        "series": (
            ("overlay.rss_growth_kb", "rss_growth_kb（全程）"),
            ("overlay.rss_tail_growth_kb", "rss_tail_growth_kb（尾段）"),
        ),
        "budget": "overlay.rss_tail_growth_kb",
    },
    {
        "title": "处理器",
        "suffix": "启动与空闲窗口",
        "series": (
            ("app.startup_cpu_ms", "app.startup_cpu_ms"),
            ("app.cpu_idle_ms", "app.cpu_idle_ms（30s）"),
        ),
        "budget": None,
    },
    {
        "title": "二进制体积",
        "suffix": "release 产物",
        "series": (
            ("app.binary_x64_kb", "app.binary_x64_kb"),
            ("app.binary_arm64_kb", "app.binary_arm64_kb"),
        ),
        "budget": None,
    },
    {
        "title": "窗口句柄数",
        "suffix": "",
        "unit": False,
        "series": (("overlay.window_handles", "overlay.window_handles"),),
        "budget": None,
    },
)


def hotpath_panels():
    """core 四组 + clone 两面板；id 与用例全部取自注册表（benches 增删同步 metrics.py）。"""
    panels = [
        {
            "title": f"{zh}（{gid} · 对数轴）",
            "series": tuple((f"core.{gid}/{case}", case) for case in CORE_CASES[gid]),
        }
        for gid, zh in CORE_GROUPS
    ]
    panels.append(
        {
            "title": "clone 热点分配合计（allocs / bytes · 对数轴）",
            "series": (("clone.allocs", "allocs"), ("clone.bytes", "bytes")),
            "mixed_units": True,
        }
    )
    panels.append(
        {
            "title": "clone 总数与密度（total / density · 对数轴）",
            "series": (("clone.total", "total"), ("clone.density", "density")),
            "mixed_units": True,
        }
    )
    return tuple(panels)


FIGURES = (
    ("perf-latency.svg", "Gloss 性能趋势 · 响应性", LATENCY_PANELS, "linear", FOOTER_GUTTER),
    ("perf-resources.svg", "Gloss 性能趋势 · 资源与体积", RESOURCES_PANELS, "linear", FOOTER_GUTTER),
    ("perf-hotpath.svg", "Gloss 性能趋势 · 热路径", hotpath_panels(), "log", FOOTER_HOTPATH),
)


def _validate_panels():
    """import 期校验：面板 id 未注册、预算语义漂移、同面板单位混装当场失败。

    三张图的面板（含 hotpath_panels() 派生的热路径面板）全部过检；标记
    mixed_units 的面板（clone 两面板对数轴有意混装单位）豁免单位一致检查，
    id 注册校验不豁免。
    """
    problems = []
    for name, _title, panels, _kind, _footer in FIGURES:
        for panel in panels:
            units = set()
            for mid, _label in panel["series"]:
                metric = METRICS.get(mid)
                if metric is None:
                    problems.append(f"{name}/{panel['title']}: 未注册指标 {mid}")
                    continue
                units.add(metric.unit)
            if len(units) > 1 and not panel.get("mixed_units"):
                problems.append(f"{name}/{panel['title']}: 面板内单位混装 {sorted(units)}")
            budget_id = panel.get("budget")
            if budget_id is not None:
                metric = METRICS.get(budget_id)
                if metric is None or metric.judge != "预算门禁" or budget_id not in BUDGET_CONSTS:
                    problems.append(f"{name}/{panel['title']}: 预算 {budget_id} 不是注册的预算门禁指标")
                elif metric.unit not in units:
                    problems.append(f"{name}/{panel['title']}: 预算 {budget_id} 单位不在面板系列中")
    if problems:
        raise SystemExit("面板清单与注册表不一致：\n" + "\n".join(problems))


_validate_panels()


def esc(text):
    return (
        str(text)
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
    )


def f1(value):
    return f"{value:.1f}"


def fh(value):
    """长度类坐标：整数值不带小数（卡片 212 / chip 18），浮点一位小数（渐晕区高度）。"""
    return str(int(value)) if float(value).is_integer() else f1(value)


def text_w(text, size):
    """按字符宽度估算文本像素宽（CJK/全角 1em、空格 0.52em、其余 0.58em），防溢出。"""
    total = 0.0
    for ch in str(text):
        if ch == " ":
            total += 0.52
        elif ord(ch) >= 0x2E80 or ch in "●▲▼—…":
            total += 1.0
        else:
            total += 0.58
    return total * size


def t(x, y, content, size, color, weight="400", anchor="start", halo=False):
    halo_attr = ' stroke="#ffffff" stroke-width="2.5" paint-order="stroke"' if halo else ""
    return (
        f'<text x="{f1(x)}" y="{f1(y)}" font-size="{size}" fill="{color}" '
        f'font-weight="{weight}" text-anchor="{anchor}"{halo_attr}>{esc(content)}</text>'
    )


def rect(x, y, w, h, fill, rx=0, opacity=None, stroke=None):
    opacity_attr = f' fill-opacity="{opacity}"' if opacity is not None else ""
    stroke_attr = f' stroke="{stroke}"' if stroke else ""
    return (
        f'<rect x="{f1(x)}" y="{f1(y)}" width="{f1(max(w, 0))}" height="{fh(h)}" '
        f'fill="{fill}"{opacity_attr} rx="{rx}"{stroke_attr}/>'
    )


def line(x1, y1, x2, y2, color, width=1, dash=None, cap=None):
    dash_attr = f' stroke-dasharray="{dash}"' if dash else ""
    cap_attr = f' stroke-linecap="{cap}"' if cap else ""
    return (
        f'<line x1="{f1(x1)}" y1="{f1(y1)}" x2="{f1(x2)}" y2="{f1(y2)}" '
        f'stroke="{color}" stroke-width="{width}"{dash_attr}{cap_attr}/>'
    )


def circle(cx, cy, radius, fill, stroke=None, stroke_width=None, opacity=None):
    opacity_attr = f' opacity="{opacity}"' if opacity is not None else ""
    if stroke:
        return (
            f'<circle cx="{f1(cx)}" cy="{f1(cy)}" r="{radius}" fill="{fill}" '
            f'stroke="{stroke}" stroke-width="{stroke_width}"{opacity_attr}/>'
        )
    return f'<circle cx="{f1(cx)}" cy="{f1(cy)}" r="{radius}" fill="{fill}"{opacity_attr}/>'


def polyline(pts, color):
    points = " ".join(f"{f1(x)},{f1(y)}" for x, y in pts)
    return (
        f'<polyline points="{points}" fill="none" stroke="{color}" stroke-width="2" '
        'stroke-linejoin="round" stroke-linecap="round"/>'
    )


def x_positions(count, x0, x1):
    if count == 1:
        return [(x0 + x1) / 2.0]
    step = (x1 - x0) / (count - 1)
    return [x0 + i * step for i in range(count)]


def axis_top(peak, budget, count_floor):
    """线性轴顶值 = 容纳 peak 的最小 4×nice 档（档位 1/1.2/1.5/2/2.5/3/4/5/6/8/10×10^k）。

    预算计入比较：预算恰落在顶值上时自动升一档，防渐晕区高度归零、红线压轴。
    count 面板从 10^0 档起（步长下限 1），防 0.8 类碎刻度。
    """
    level = 0 if count_floor else -6
    while level <= 12:
        for step in NICE_STEPS:
            cand = 4 * step * (10 ** level)
            if cand >= peak and (budget is None or cand > budget):
                return cand
        level += 1
    raise SystemExit(f"错误：轴顶值计算越界（peak={peak}, budget={budget}）")


def value_label(value, unit, mb_axis):
    """轴刻度与数值槽共用的显示值格式（value 已是轴显示单位，单位在面板标题不带出）。"""
    if mb_axis:
        return f"{value:.1f}"
    if unit == "count":
        return f"{round(value):,}" if float(value).is_integer() else f"{value:,.1f}"
    return f"{round(value):,}"


def change_text(cur, prev):
    """较上一点变化：▼ 绿=改善 / ▲ 红=回归 / — 灰=持平（全部指标越低越好）。

    方向按数值本身比较（负值基线下 (cur-prev)/prev 会翻转符号——净增长从
    -340 降到 -520 是改善，不是回归），幅度用 |Δ|/|prev|。
    """
    if prev is None:
        return None
    if prev == 0:
        return ("— 持平", COLOR_MUTED) if cur == 0 else ("—", COLOR_MUTED)
    delta = cur - prev
    if delta == 0:
        return ("— 持平", COLOR_MUTED)
    rounded = round(abs(delta) / abs(prev) * 100, 1)
    if rounded == 0:
        return ("— 持平", COLOR_MUTED)
    if delta < 0:
        return (f"▼ {rounded:.1f}%", COLOR_DOWN)
    return (f"▲ {rounded:.1f}%", COLOR_UP)


def series_values(points, mid):
    return [point["metrics"].get(mid) for point in points]


def latest_budget(points, mid):
    for point in reversed(points):
        budgets = point.get("budgets")
        if isinstance(budgets, dict) and mid in budgets:
            return budgets[mid]
    return None


def latest_env(points):
    for point in reversed(points):
        env = point.get("env")
        if isinstance(env, dict) and env:
            return env
    return {}


def env_display(points):
    env = latest_env(points)
    os_name = env.get("os")
    left = " ".join(
        part
        for part in (OS_LABELS.get(os_name, os_name) if os_name else None, env.get("arch"))
        if part
    )
    parts = [part for part in (left, env.get("cpu"), env.get("gpu")) if part]
    body = " · ".join(parts) if parts else "暂无记录"
    return f"环境：{body} · {ENV_TAIL}"


def short_sha(point):
    return str(point.get("commit", "?"))[:7]


def short_date(point):
    raw = str(point.get("date", ""))
    return raw[5:10] if len(raw) >= 10 else "?"


def present(xs, values):
    return [(x, v) for x, v in zip(xs, values) if v is not None]


def present_log(xs, values):
    """对数轴的可画点：非正值无对数坐标，按缺源跳段处理（不进线、不标值）。"""
    return [(x, v) for x, v in zip(xs, values) if v is not None and v > 0]


def segments(xs, values):
    """连续在场点连成段（缺源跳段不造点，段内至少两点才画线）。"""
    out, cur = [], []
    for x, v in zip(xs, values):
        if v is None:
            if len(cur) > 1:
                out.append(cur)
            cur = []
        else:
            cur.append((x, v))
    if len(cur) > 1:
        out.append(cur)
    return out


def latest_index(values):
    for i in range(len(values) - 1, -1, -1):
        if values[i] is not None:
            return i
    return None


def prev_present(values, index):
    for i in range(index - 1, -1, -1):
        if values[i] is not None:
            return values[i]
    return None


class Figure:
    """单张图：页头 + 面板卡片序列 + 页脚；画布高度按面板数先算后画（旧版教训）。"""

    def __init__(self, page_title, points, footer, panel_count):
        self.points = points
        self.footer = footer
        self.gradients = set()
        last_bottom = HEADER_H + (panel_count - 1) * CARD_PITCH + CARD_H
        self.last_bottom = last_bottom
        height = last_bottom + BOTTOM_PAD
        self.parts = [
            rect(0, 0, WIDTH, height, COLOR_BG),
            rect(CARD_X, 14, 4, 20, "#2563eb", rx=2),
            t(32, 30, page_title, 17, COLOR_TITLE, weight="700"),
            t(32, 48, env_display(points), 11, COLOR_MUTED),
            t(
                WIDTH - 20,
                30,
                f"{len(points)} commits · 最新 {short_sha(points[-1])}",
                10.5,
                COLOR_MUTED,
                anchor="end",
            ),
        ]
        self.card_y = HEADER_H

    def _legend_marker(self, lx, baseline, color):
        self.parts.append(line(lx, baseline - 3.5, lx + 16, baseline - 3.5, color, width=2, cap="round"))
        self.parts.append(circle(lx + 8, baseline - 3.5, 2.6, "#ffffff", stroke=color, stroke_width=1.6))

    def _chips(self, y0, chip_defs):
        right = CHIP_RIGHT
        for label, bg, fg in chip_defs:
            w = text_w(label, 10) + 14
            self.parts.append(rect(right - w, y0 + CHIP_DY, w, CHIP_H, bg, rx=9))
            self.parts.append(t(right - w / 2, y0 + CHIP_DY + 14.5, label, 10, fg, anchor="middle"))
            right -= w + 8

    def _x_axis(self, xs, sha_dy, date_dy):
        for x, point in zip(xs, self.points):
            self.parts.append(t(x, self.card_y + sha_dy, short_sha(point), 10, COLOR_MUTED, anchor="middle"))
            self.parts.append(t(x, self.card_y + date_dy, short_date(point), 9, COLOR_FAINT, anchor="middle"))

    def _partial_note(self, plot_top, plot_bottom, note_x, series, xs, y_of):
        """「部分系列自 <sha> 起有数据」：默认绘图区右上，数据带相撞时避让。

        轴顶恒贴系列峰值（top ≥ peak 且同量级），线在右侧高位时注释必压线
        （二进制体积贴顶平线每次渲染都命中）。避让规则限定两条带——上带
        （默认右上）与下带（绘图区右下），按注释 x 跨度 +8px 光晕余量内的
        数据 y 范围（折线段 y 范围 = 端点范围，min/max 判交即精确）判定：
        上带相撞移下带，两带皆占则回右上靠 halo 兜底。纯数据函数，确定性。
        """
        firsts = []
        for _mid, _label, _color, vals in series:
            first = next((i for i, v in enumerate(vals) if v is not None), None)
            if first is not None:
                firsts.append(first)
        if not (firsts and max(firsts) > 0):
            return
        note = PARTIAL_MESSAGE.format(sha=short_sha(self.points[max(firsts)]))
        note_left = note_x - text_w(note, 9)

        def collides(band_top, band_bottom):
            ys = [
                y_of(v)
                for _mid, _label, _color, vals in series
                for x, v in zip(xs, vals)
                if v is not None and x >= note_left - 8
            ]
            if not ys:
                return False
            return min(ys) - 8 <= band_bottom and max(ys) + 8 >= band_top

        if collides(plot_top + 1, plot_top + 13.5) and not collides(
            plot_bottom - 13.5, plot_bottom - 1
        ):
            baseline = plot_bottom - 4.5
        else:
            baseline = plot_top + PARTIAL_NOTE_DY
        self.parts.append(
            t(note_x, baseline, note, 9, COLOR_MUTED, anchor="end", halo=True)
        )

    def linear_panel(self, panel):
        y0 = self.card_y
        plot_top, plot_bottom = y0 + PLOT_TOP_DY, y0 + PLOT_TOP_DY + PLOT_H
        xs = x_positions(len(self.points), PLOT_X0, PLOT_X1)
        series = [
            (mid, label, PALETTE[i % len(PALETTE)], series_values(self.points, mid))
            for i, (mid, label) in enumerate(panel["series"])
        ]

        unit = METRICS[series[0][0]].unit
        budget_id = panel.get("budget")
        budget = latest_budget(self.points, budget_id) if budget_id else None
        flat = [v for _, _, _, vals in series for v in vals if v is not None]
        peak = max(flat + [budget] if budget is not None else flat) if flat or budget is not None else 0
        if peak <= 0:
            peak = 1.0 if unit == "count" else 100.0
        mb_axis = unit == "kb" and peak >= MB_AXIS_MIN_KB
        disp = (lambda v: v / KB_PER_MB) if mb_axis else (lambda v: v)
        top = axis_top(disp(peak), disp(budget) if budget is not None else None, unit == "count")

        # 负值扩展：净增长类指标可为负，轴向下按同 step 整数倍延伸，0 轴保持在
        # 网格线上（无负值时 lo=0，映射与网格布局退化为原 0 基轴形态）。
        step = top / GRID_DIV
        n_neg = 0
        if flat and min(flat) < 0:
            n_neg = math.ceil(-disp(min(flat)) / step)
            lo = -n_neg * step
        else:
            lo = 0.0

        def y_of(v):
            return plot_bottom - (disp(v) - lo) / (top - lo) * PLOT_H

        if panel.get("unit", True):
            unit_label = ("MB" if mb_axis else "KB") if unit == "kb" else unit
            inner = f"{unit_label} · {panel['suffix']}" if panel["suffix"] else unit_label
            full_title = f"{panel['title']}（{inner}）"
        else:
            full_title = f"{panel['title']}（{panel['suffix']}）" if panel["suffix"] else panel["title"]

        self.parts.append(rect(CARD_X, y0, CARD_W, CARD_H, COLOR_CARD, rx=10, stroke=COLOR_CARD_STROKE))
        self.parts.append(t(CARD_X + 16, y0 + TITLE_DY, full_title, 12.5, COLOR_TITLE, weight="600"))

        budget_series = next((vals for mid, _l, _c, vals in series if mid == budget_id), None)
        chip_defs = []
        if budget is not None:
            if budget_series and any(v is not None for v in budget_series):
                within = max(v for v in budget_series if v is not None) <= budget
                chip_defs.append(
                    ("● 预算内", CHIP_PASS_BG, CHIP_PASS_FG)
                    if within
                    else ("● 超预算", CHIP_FAIL_BG, CHIP_FAIL_FG)
                )
            chip_defs.append((f"预算 {fmt(budget, unit)}", CHIP_NEUTRAL_BG, CHIP_BUDGET_FG))
        elif budget_id is not None:
            chip_defs.append(("预算未入库", CHIP_NEUTRAL_BG, CHIP_RECORD_FG))
        else:
            chip_defs.append(("仅记录", CHIP_NEUTRAL_BG, CHIP_RECORD_FG))
        self._chips(y0, chip_defs)

        baseline = y0 + LEGEND_DY
        lx = CARD_X + 16
        for _mid, label, color, _vals in series:
            self._legend_marker(lx, baseline, color)
            self.parts.append(t(lx + 21, baseline, label, 10.5, COLOR_BODY))
            lx += 21 + text_w(label, 10.5) + 15
        note = panel.get("note")
        if note is None and budget_id:
            const = BUDGET_CONSTS[budget_id]
            note = f"预算常量：{const.file} :: {const.name}"
        if note:
            self.parts.append(t(CHIP_RIGHT, baseline, note, 9.5, COLOR_MUTED, anchor="end"))

        n_lines = GRID_DIV + n_neg
        for k in range(n_lines + 1):
            value = lo + k * step
            gy = plot_bottom - k * (PLOT_H / n_lines)
            self.parts.append(line(PLOT_X0, gy, PLOT_X1, gy, COLOR_AXIS if k == n_neg else COLOR_GRID))
            self.parts.append(
                t(54, gy + 3.5, value_label(value, unit, mb_axis), 10, COLOR_MUTED, anchor="end")
            )

        if budget is not None:
            by = y_of(budget)
            self.parts.append(rect(PLOT_X0, plot_top, PLOT_X1 - PLOT_X0, by - plot_top, COLOR_BUDGET, opacity=0.05))
            self.parts.append(line(PLOT_X0, by, PLOT_X1, by, COLOR_BUDGET, width=1.3, dash="5 4"))

        visible = [s for s in series if any(v is not None for v in s[3])]
        if len(visible) == 1:
            _mid, _label, color, vals = visible[0]
            pts = present(xs, vals)
            idxs = [i for i, v in enumerate(vals) if v is not None]
            contiguous = idxs == list(range(idxs[0], idxs[-1] + 1))
            present_vals = [v for v in vals if v is not None]
            span = (max(present_vals) - min(present_vals)) / (top * (KB_PER_MB if mb_axis else 1))
            if contiguous and len(pts) > 1 and span >= AREA_MIN_SPAN:
                self.gradients.add(color)
                path = "M " + " L ".join(f"{f1(x)},{f1(y_of(v))}" for x, v in pts)
                self.parts.append(
                    f'<path d="{path} L {f1(pts[-1][0])},{f1(plot_bottom)} '
                    f'L {f1(pts[0][0])},{f1(plot_bottom)} Z" fill="url(#g-{color.lstrip("#")})"/>'
                )
        for _mid, _label, color, vals in series:
            for seg in segments(xs, vals):
                self.parts.append(polyline([(x, y_of(v)) for x, v in seg], color))
            pts = present(xs, vals)
            for x, v in pts[:-1]:
                self.parts.append(
                    circle(x, y_of(v), 3, "#ffffff", stroke=color, stroke_width=1.8)
                )
            if pts:
                x, v = pts[-1]
                y = y_of(v)
                self.parts.append(circle(x, y, 8, color, opacity=0.14))
                self.parts.append(circle(x, y, 3.5, color))

        self._partial_note(
            plot_top,
            plot_bottom,
            PLOT_X1 - 4,
            series,
            xs,
            y_of,
        )

        if not visible:
            self.parts.append(
                t(
                    (PLOT_X0 + PLOT_X1) / 2,
                    plot_top + PLOT_H / 2 + 4,
                    EMPTY_MESSAGE,
                    11,
                    COLOR_MUTED,
                    anchor="middle",
                )
            )
        else:
            self.parts.append(line(GUTTER_DIV_X, plot_top, GUTTER_DIV_X, plot_bottom, COLOR_AXIS))
            entries = []
            for _mid, _label, color, vals in series:
                li = latest_index(vals)
                if li is None:
                    continue
                entries.append(
                    {
                        "value": vals[li],
                        "prev": prev_present(vals, li),
                        "color": color,
                        "y": y_of(vals[li]),
                    }
                )
            entries.sort(key=lambda e: -e["value"])
            placed, prev_anchor = [], None
            for entry in entries:
                anchor = entry["y"] + 4
                if prev_anchor is not None:
                    anchor = max(anchor, prev_anchor + GUTTER_BLOCK_PITCH)
                placed.append((anchor, entry))
                prev_anchor = anchor
            overflow = placed[-1][0] + GUTTER_VALUE_TO_CHANGE - (plot_bottom - GUTTER_BOTTOM_MARGIN)
            if overflow > 0:
                placed = [(anchor - overflow, entry) for anchor, entry in placed]
            for anchor, entry in placed:
                self.parts.append(
                    t(
                        GUTTER_TEXT_X,
                        anchor,
                        value_label(disp(entry["value"]), unit, mb_axis),
                        11.5,
                        entry["color"],
                        weight="700",
                    )
                )
                change = change_text(entry["value"], entry["prev"])
                if change:
                    self.parts.append(
                        t(GUTTER_TEXT_X, anchor + GUTTER_VALUE_TO_CHANGE, change[0], 9.5, change[1])
                    )

        self._x_axis(xs, PLOT_TOP_DY + PLOT_H + X_SHA_DY, PLOT_TOP_DY + PLOT_H + X_DATE_DY)
        self.card_y += CARD_PITCH

    def log_panel(self, panel):
        y0 = self.card_y
        plot_top, plot_bottom = y0 + HP_PLOT_TOP_DY, y0 + HP_PLOT_TOP_DY + HP_PLOT_H
        xs = x_positions(len(self.points), HP_PLOT_X0, HP_PLOT_X1)
        series = [
            (mid, label, PALETTE[i % len(PALETTE)], series_values(self.points, mid))
            for i, (mid, label) in enumerate(panel["series"])
        ]
        self.parts.append(rect(CARD_X, y0, CARD_W, CARD_H, COLOR_CARD, rx=10, stroke=COLOR_CARD_STROKE))
        self.parts.append(t(CARD_X + 16, y0 + TITLE_DY, panel["title"], 12.5, COLOR_TITLE, weight="600"))
        self._chips(y0, [("仅记录", CHIP_NEUTRAL_BG, CHIP_RECORD_FG)])
        self.parts.append(rect(HP_PLOT_X0 - 8, plot_top, HP_PLOT_X1 - HP_PLOT_X0 + 16, HP_PLOT_H, COLOR_TRACK, rx=4))

        logs = [
            math.log10(v)
            for _m, _l, _c, vals in series
            for v in vals
            if v is not None and v > 0
        ]
        if logs:
            lo, hi = min(logs), max(logs)
            if lo == hi:
                lo, hi = lo - 1, hi + 1
            pad = (hi - lo) * LOG_PAD

            def scale(v):
                return plot_bottom - (math.log10(v) - lo) / (hi + pad - lo) * HP_PLOT_H

            labels = []
            for mid, _label, color, vals in series:
                for seg in segments(xs, vals):
                    seg = [(x, v) for x, v in seg if v > 0]
                    if len(seg) > 1:
                        self.parts.append(polyline([(x, scale(v)) for x, v in seg], color))
                pts = present_log(xs, vals)
                for x, v in pts[:-1]:
                    self.parts.append(circle(x, scale(v), 3, "#ffffff", stroke=color, stroke_width=1.8))
                if pts:
                    x, v = pts[-1]
                    y = scale(v)
                    self.parts.append(circle(x, y, 8, color, opacity=0.14))
                    self.parts.append(circle(x, y, 3.5, color))
                    labels.append([x, y - HP_LABEL_TO_POINT, color, fmt(v, METRICS[mid].unit)])
            labels.sort(key=lambda item: -item[1])
            for i in range(1, len(labels)):
                labels[i][1] = min(labels[i][1], labels[i - 1][1] - HP_LABEL_DE_COLLIDE)
            for x, y, color, label in labels:
                self.parts.append(t(x, y, label, 10.5, color, weight="600", anchor="middle", halo=True))
            self._partial_note(plot_top, plot_bottom, HP_PLOT_X1 - 4, series, xs, scale)
        else:
            self.parts.append(
                t(
                    (HP_PLOT_X0 + HP_PLOT_X1) / 2,
                    plot_top + HP_PLOT_H / 2 + 4,
                    EMPTY_MESSAGE,
                    11,
                    COLOR_MUTED,
                    anchor="middle",
                )
            )

        self._x_axis(xs, HP_PLOT_TOP_DY + HP_PLOT_H + HP_X_SHA_DY, HP_PLOT_TOP_DY + HP_PLOT_H + HP_X_DATE_DY)
        for i, (_mid, label, color, _vals) in enumerate(series):
            col, row = i % 4, i // 4
            lx = CARD_X + 16 + col * HP_LEGEND_COL_PITCH
            baseline = y0 + HP_LEGEND_ROW0_DY + row * HP_LEGEND_ROW_PITCH
            self._legend_marker(lx, baseline, color)
            self.parts.append(t(lx + 21, baseline, label, 10.5, COLOR_BODY))
        self.card_y += CARD_PITCH

    def render(self):
        self.parts.append(t(32, self.last_bottom + FOOTER_GAP, self.footer, 9.5, COLOR_MUTED))
        height = self.last_bottom + BOTTOM_PAD
        head = (
            f'<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{height}" '
            f'viewBox="0 0 {WIDTH} {height}" font-family="{FONT_FAMILY}">'
        )
        lines = [head, self.parts[0]]
        if self.gradients:
            defs = "<defs>" + "".join(self._gradient(color) for color in sorted(self.gradients)) + "</defs>"
            lines.append(defs)
        lines.extend(self.parts[1:])
        lines.append("</svg>")
        return "\n".join(lines) + "\n"

    @staticmethod
    def _gradient(color):
        key = color.lstrip("#")
        return (
            f'<linearGradient id="g-{key}" x1="0" y1="0" x2="0" y2="1">'
            f'<stop offset="0" stop-color="{color}" stop-opacity="{GRADIENT_OPACITY}"/>'
            f'<stop offset="1" stop-color="{color}" stop-opacity="0"/></linearGradient>'
        )


def build_figure(spec, points):
    file_name, page_title, panels, kind, footer = spec
    fig = Figure(page_title, points, footer, len(panels))
    for panel in panels:
        if kind == "linear":
            fig.linear_panel(panel)
        else:
            fig.log_panel(panel)
    return file_name, fig.render()


def load_history(path):
    if not path.is_file():
        raise SystemExit(f"错误：缺少 {path}，先运行统一采集器生成趋势历史")
    points = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]
    if not points:
        raise SystemExit(f"错误：{path} 为空")
    for i, point in enumerate(points):
        if not isinstance(point.get("metrics"), dict):
            raise SystemExit(
                f"错误：第 {i + 1} 点（{point.get('commit', '?')}）缺 metrics 段（v1 旧格式）。"
                "图表只读 v2 扁平点，请先用统一采集器迁移历史后重绘。"
            )
    return points


def main():
    parser = argparse.ArgumentParser(description="性能趋势三图（确定性 SVG）")
    parser.add_argument("--history", default=str(DEFAULT_HISTORY), help="history.jsonl 路径")
    parser.add_argument("--out", default=str(DEFAULT_OUT_DIR), help="输出目录")
    args = parser.parse_args()

    points = load_history(Path(args.history))
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    for spec in FIGURES:
        file_name, content = build_figure(spec, points)
        path = out_dir / file_name
        path.write_text(content, encoding="utf-8")
        print(f"图表已生成：{path}")


if __name__ == "__main__":
    main()
