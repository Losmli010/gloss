#!/usr/bin/env python3
"""性能指标注册表：全部趋势指标的唯一登记处，编号、类别、单位、判定与数据
源都在这里。

统一采集器、趋势图表与趋势报告三个消费方共用本模块：指标编号、中文标
签、单位与格式化、判定语义、对比噪声阈值都从这里取，不在消费方各存一
份。预算门禁指标只登记（Rust）常量名与所在文件，预算数值不走注册表
——常量随自检标准错误上的结构化行流出，采集器折算进历史点的预算段，图
表与报告取最新携带预算的点画参考线，任何环节都不手写数值。

单位词表固定五种：毫秒、千字节、计数、纳秒与字节数（千字节渲染时按
1024 进制换算兆字节）。判定语义三档：预算门禁由（L3）自检判退出码；精
确计数门禁对照历史最新点，整数上升即失败；仅记录作趋势对照加噪声阈值启
发式参考，输出须标注“非门禁，仅参考”。
"""

from dataclasses import dataclass

CATEGORIES = (
    ("latency", "latency"),
    ("memory", "memory"),
    ("cpu", "cpu"),
    ("size", "size"),
    ("hotpath", "hotpath"),
    ("code", "code health"),
    ("external", "external"),
)
CATEGORY_LABELS = dict(CATEGORIES)

UNITS = frozenset({"ms", "kb", "count", "ns", "bytes"})
JUDGES = ("budget gate", "exact count gate", "record only")
SOURCES = frozenset(
    {"app-metrics", "overlay-selftest", "startup-selftest", "clone-stats", "criterion", "eval-live"}
)


@dataclass(frozen=True)
class Metric:
    """单条指标的注册信息。

    label 供图表图例与报告表格使用；noise 是仅记录类对比的启发式阈值
    （相对变化超过该比例才标改善/回归，非门禁），门禁类指标不消费该
    字段；note 是判定语义的补充说明（报告判定列随 judge 一并显示）。
    """

    category: str
    label: str
    unit: str
    judge: str
    source: str
    noise: float = 0.05
    note: str = ""


@dataclass(frozen=True)
class BudgetConst:
    """预算门禁指标对应的 Rust 常量：file 为相对仓库根的源文件，name 为常量名。"""

    file: str
    name: str


BUDGET_CONSTS = {
    "startup.elapsed_ms": BudgetConst("tests/startup.rs", "STARTUP_BUDGET"),
    "overlay.show_first_ms": BudgetConst("tests/overlay.rs", "SHOW_BUDGET"),
    "overlay.rss_tail_growth_kb": BudgetConst("tests/overlay.rs", "RSS_TAIL_GROWTH_BUDGET_KB"),
}

# 四组用例与基准文件一一对应，基准增删用例时同步这里；
# 编号取基准框架的全名前缀，即组名点用例名。
CORE_GROUPS = (
    ("cache_key", "cache key"),
    ("complete", "structured parse"),
    ("prompt_render", "prompt render"),
    ("task_cache", "task cache"),
)
CORE_CASES = {
    "cache_key": ("text/short", "text/long", "image/1mb", "image/4mb"),
    "complete": ("word_card/ok", "word_card/many_examples", "long_note/ok", "prose/degraded"),
    "prompt_render": ("word/short", "code/long"),
    "task_cache": ("get/hit", "get/miss", "set/same_key", "set/new_key"),
}


def _core_metrics():
    """展开 criterion 热点墙钟指标（bench 墙钟随机器波动，恒为仅记录）。"""
    return {
        f"core.{group}/{case}": Metric(
            "hotpath", f"{label} {case}", "ns", "record only", "criterion"
        )
        for group, label in CORE_GROUPS
        for case in CORE_CASES[group]
    }


METRICS = {
    # ---- 响应性 ----
    "app.startup_ms": Metric(
        "latency", "startup total spawn→ready milestone", "ms", "record only", "app-metrics"
    ),
    "startup.elapsed_ms": Metric(
        "latency",
        "startup selftest wall clock (start→warmup frame done)",
        "ms",
        "budget gate",
        "startup-selftest",
    ),
    "overlay.show_first_ms": Metric(
        "latency", "selection→first frame (worst)", "ms", "budget gate", "overlay-selftest"
    ),
    "overlay.show_p50_ms": Metric(
        "latency", "show/hide latency p50", "ms", "record only", "overlay-selftest"
    ),
    "overlay.show_p95_ms": Metric(
        "latency", "show/hide latency p95", "ms", "record only", "overlay-selftest"
    ),
    "overlay.show_max_ms": Metric(
        "latency", "show/hide latency worst", "ms", "record only", "overlay-selftest"
    ),
    "overlay.hide_p50_ms": Metric(
        "latency", "overlay hide latency p50", "ms", "record only", "overlay-selftest"
    ),
    "overlay.hide_max_ms": Metric(
        "latency", "overlay hide latency worst", "ms", "record only", "overlay-selftest"
    ),
    "overlay.frame_p95_ms": Metric(
        "latency", "show/hide animation frame time p95", "ms", "record only", "overlay-selftest"
    ),
    "overlay.frame_missed": Metric(
        "latency",
        "frames >32 ms during animation",
        "count",
        "record only",
        "overlay-selftest",
        note="report only",
    ),
    # ---- 内存 ----
    "app.rss_idle_kb": Metric(
        "memory", "steady-state RSS (30s idle window tail)", "kb", "record only", "app-metrics"
    ),
    "app.rss_peak_kb": Metric("memory", "startup peak RSS", "kb", "record only", "app-metrics"),
    "app.footprint_idle_kb": Metric(
        "memory",
        "phys_footprint steady (activity monitor basis)",
        "kb",
        "record only",
        "app-metrics",
    ),
    "app.footprint_peak_kb": Metric(
        "memory", "phys_footprint peak (activity monitor basis)", "kb", "record only", "app-metrics"
    ),
    "overlay.rss_growth_kb": Metric(
        "memory", "show/hide RSS net growth (full run)", "kb", "record only", "overlay-selftest"
    ),
    "overlay.rss_tail_growth_kb": Metric(
        "memory", "show/hide RSS net growth (tail)", "kb", "budget gate", "overlay-selftest"
    ),
    "overlay.window_handles": Metric(
        "memory", "window handles (leak sentinel)", "count", "record only", "overlay-selftest"
    ),
    # ---- 处理器 ----
    "app.startup_cpu_ms": Metric("cpu", "startup CPU time", "ms", "record only", "app-metrics"),
    "app.cpu_idle_ms": Metric("cpu", "idle window CPU (30s)", "ms", "record only", "app-metrics"),
    # ---- 体积 ----
    "app.binary_x64_kb": Metric(
        "size", "binary size x64 (release)", "kb", "record only", "app-metrics"
    ),
    "app.binary_arm64_kb": Metric(
        "size", "binary size arm64 (release)", "kb", "record only", "app-metrics"
    ),
    # ---- 热路径 ----
    **_core_metrics(),
    "clone.allocs": Metric(
        "hotpath", "hotpath allocs total (count)", "count", "exact count gate", "criterion"
    ),
    "clone.bytes": Metric(
        "hotpath", "hotpath allocs total (bytes)", "bytes", "exact count gate", "criterion"
    ),
    # ---- 代码健康 ----
    "clone.total": Metric("code", ".clone() total", "count", "exact count gate", "clone-stats"),
    "clone.density": Metric(
        "code", ".clone() density (/kloc)", "count", "record only", "clone-stats"
    ),
    # ---- 外部依赖 ----
    "eval.task_latency_p50_ms": Metric(
        "external",
        "LLM task end-to-end latency p50",
        "ms",
        "record only",
        "eval-live",
        noise=0.20,
        note="external service variance",
    ),
    "eval.task_latency_p95_ms": Metric(
        "external",
        "LLM task end-to-end latency p95",
        "ms",
        "record only",
        "eval-live",
        noise=0.20,
        note="external service variance",
    ),
}

_KB_PER_MB = 1024
_KB_MB_SWITCH = 10 * _KB_PER_MB  # 千字节值过万才换兆显示，以下保持千字节


def fmt(value, unit):
    """把指标数值格式化为人读字符串（图表数值槽与报告表格共用同一出口）。

    ms 取整数千位分隔；kb ≥10 MB 换算 MB 一位小数（1024 进制），以下 KB
    千位分隔；count 整数千位分隔、非整数保留一位小数；ns 按量级落在
    ns/µs/ms 档；bytes 按 B/KB/MB/GB 换算。单位不在词表内直接抛错，
    防拼错单位后静默输出错误格式。
    """
    if unit == "ms":
        return f"{round(value):,} ms"
    if unit == "kb":
        if value >= _KB_MB_SWITCH:
            return f"{value / _KB_PER_MB:.1f} MB"
        return f"{round(value):,} KB"
    if unit == "count":
        if value == int(value):
            return f"{int(value):,}"
        return f"{value:,.1f}"
    if unit == "ns":
        if value >= 1_000_000:
            return f"{value / 1_000_000:,.1f} ms"
        if value >= 1_000:
            return f"{value / 1_000:,.1f} µs"
        return f"{value:,.0f} ns"
    if unit == "bytes":
        if value >= _KB_PER_MB**3:
            return f"{value / _KB_PER_MB**3:.1f} GB"
        if value >= _KB_PER_MB**2:
            return f"{value / _KB_PER_MB**2:.1f} MB"
        if value >= _KB_PER_MB:
            return f"{value / _KB_PER_MB:.1f} KB"
        return f"{value:,.0f} B"
    raise ValueError(f"unknown unit {unit!r} (vocabulary: ms/kb/count/ns/bytes)")


def _validate():
    """注册表一致性自检，import 时执行：登记项漂移当场失败，不留到消费方。"""
    problems = []
    for mid, metric in METRICS.items():
        if metric.category not in CATEGORY_LABELS:
            problems.append(f"{mid}: unknown category {metric.category!r}")
        if metric.unit not in UNITS:
            problems.append(f"{mid}: unknown unit {metric.unit!r}")
        if metric.judge not in JUDGES:
            problems.append(f"{mid}: unknown judge {metric.judge!r}")
        if metric.source not in SOURCES:
            problems.append(f"{mid}: unknown source {metric.source!r}")
        if metric.judge == "budget gate" and mid not in BUDGET_CONSTS:
            problems.append(f"{mid}: budget gate metric missing a BUDGET_CONSTS entry")
    for mid in BUDGET_CONSTS:
        metric = METRICS.get(mid)
        if metric is None or metric.judge != "budget gate":
            problems.append(f"{mid}: BUDGET_CONSTS points to a non-budget-gate metric")
    if problems:
        raise RuntimeError("metric registry self-check failed:\n" + "\n".join(problems))


_validate()


if __name__ == "__main__":
    for category_id, category_label in CATEGORIES:
        print(f"## {category_label} ({category_id})")
        for mid, metric in METRICS.items():
            if metric.category != category_id:
                continue
            fields = [
                f"unit={metric.unit}",
                f"judge={metric.judge}",
                f"source={metric.source}",
                f"noise={metric.noise:g}",
            ]
            const = BUDGET_CONSTS.get(mid)
            if const:
                fields.append(f"budget={const.file} :: {const.name}")
            if metric.note:
                fields.append(f"note={metric.note}")
            print(f"  {mid:<30} {metric.label}  " + "  ".join(fields))
        print()
    core_count = sum(1 for mid in METRICS if mid.startswith("core."))
    print(f"{len(METRICS)} metrics total (core.* {core_count})")
    print("fmt examples:")
    for value, unit in [
        (1498, "ms"),
        (38.2, "ms"),
        (104212, "kb"),
        (259800, "kb"),
        (2048, "kb"),
        (340, "kb"),
        (410, "count"),
        (5.4, "count"),
        (0, "count"),
        (239, "ns"),
        (5929, "ns"),
        (23900000, "ns"),
        (4657000000, "ns"),
        (109198, "bytes"),
    ]:
        print(f"  fmt({value}, {unit!r})".ljust(28) + f"-> {fmt(value, unit)}")
