#!/usr/bin/env python3
"""性能指标注册表：全部趋势指标的唯一登记处（id / 类别 / 单位 / 判定 / 数据源）。

perf-collect / perf-chart / perf-report 共用本模块：指标 id、中文标签、单位
与格式化、判定语义、对比噪声阈值都从这里取，不在消费方各存一份。预算门禁
指标只登记 Rust 常量名与所在文件，预算数值不走注册表——Rust 常量随自检
stderr 结构化行流出，collector 折算进历史点 budgets 段，图表与报告取最新
携带预算的点画参考线，任何环节都不手写数值。

单位词表固定五种：ms / kb / count / ns / bytes（kb 渲染时按 1024 进制换算
MB）。判定语义三档：预算门禁（L3 自检判退出码）、精确计数门禁（对照历史
最新点，整数上升即失败）、仅记录（趋势对照 + 噪声阈值启发式参考，输出须
标注「非门禁，仅参考」）。

用法：
  python3 scripts/perf/metrics.py    打印全部注册项与 fmt 示例（核对清单）
  from metrics import METRICS, BUDGET_CONSTS, fmt
  fmt(1498, "ms")      # "1,498 ms"
  fmt(104212, "kb")    # "101.8 MB"
"""

from dataclasses import dataclass

CATEGORIES = (
    ("latency", "响应性"),
    ("memory", "内存"),
    ("cpu", "处理器"),
    ("size", "体积"),
    ("hotpath", "热路径"),
    ("code", "代码健康"),
    ("external", "外部依赖"),
)
CATEGORY_LABELS = dict(CATEGORIES)

UNITS = frozenset({"ms", "kb", "count", "ns", "bytes"})
JUDGES = ("预算门禁", "精确计数门禁", "仅记录")
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

# 四组×用例与 benches/core.rs 一一对应，benches 增删用例时同步这里；
# id 取 criterion full_id 前缀 core.，即 core.<group>/<case>。
CORE_GROUPS = (
    ("cache_key", "缓存键"),
    ("complete", "结构化解析"),
    ("prompt_render", "提示词渲染"),
    ("task_cache", "任务缓存"),
)
CORE_CASES = {
    "cache_key": ("text/short", "text/long", "image/1mb", "image/4mb"),
    "complete": ("word_card/ok", "word_card/many_senses", "fence_missing", "json_broken", "ocr/ok"),
    "prompt_render": ("word/short", "code/long"),
    "task_cache": ("get/hit", "get/miss", "set/same_key", "set/new_key"),
}


def _core_metrics():
    """展开 criterion 热点墙钟指标（bench 墙钟随机器波动，恒为仅记录）。"""
    return {
        f"core.{group}/{case}": Metric("hotpath", f"{label} {case}", "ns", "仅记录", "criterion")
        for group, label in CORE_GROUPS
        for case in CORE_CASES[group]
    }


METRICS = {
    # ---- latency 响应性 ----
    "app.startup_ms": Metric("latency", "启动总时长 spawn→就绪里程碑", "ms", "仅记录", "app-metrics"),
    "startup.elapsed_ms": Metric(
        "latency", "启动自检墙钟（起点→预热帧完成）", "ms", "预算门禁", "startup-selftest"
    ),
    "overlay.show_first_ms": Metric("latency", "划词→首帧（最差）", "ms", "预算门禁", "overlay-selftest"),
    "overlay.show_p50_ms": Metric("latency", "显隐延迟 p50", "ms", "仅记录", "overlay-selftest"),
    "overlay.show_p95_ms": Metric("latency", "显隐延迟 p95", "ms", "仅记录", "overlay-selftest"),
    "overlay.show_max_ms": Metric("latency", "显隐延迟最差", "ms", "仅记录", "overlay-selftest"),
    "overlay.hide_p50_ms": Metric("latency", "浮窗隐藏延迟 p50", "ms", "仅记录", "overlay-selftest"),
    "overlay.hide_max_ms": Metric("latency", "浮窗隐藏延迟最差", "ms", "仅记录", "overlay-selftest"),
    "overlay.frame_p95_ms": Metric("latency", "显隐动画帧时间 p95", "ms", "仅记录", "overlay-selftest"),
    "overlay.frame_missed": Metric(
        "latency", "动画期 >32 ms 帧数", "count", "仅记录", "overlay-selftest", note="仅报告呈现"
    ),
    # ---- memory 内存 ----
    "app.rss_idle_kb": Metric("memory", "稳态 RSS（30s 空闲窗口末段）", "kb", "仅记录", "app-metrics"),
    "app.rss_peak_kb": Metric("memory", "启动峰值 RSS", "kb", "仅记录", "app-metrics"),
    "app.footprint_idle_kb": Metric(
        "memory", "phys_footprint 稳态（活动监视器口径）", "kb", "仅记录", "app-metrics"
    ),
    "app.footprint_peak_kb": Metric(
        "memory", "phys_footprint 峰值（活动监视器口径）", "kb", "仅记录", "app-metrics"
    ),
    "overlay.rss_growth_kb": Metric("memory", "显隐 RSS 净增长（全程）", "kb", "仅记录", "overlay-selftest"),
    "overlay.rss_tail_growth_kb": Metric(
        "memory", "显隐 RSS 净增长（尾段）", "kb", "预算门禁", "overlay-selftest"
    ),
    "overlay.window_handles": Metric("memory", "窗口句柄数（泄漏哨兵）", "count", "仅记录", "overlay-selftest"),
    # ---- cpu 处理器 ----
    "app.startup_cpu_ms": Metric("cpu", "启动 CPU 时间", "ms", "仅记录", "app-metrics"),
    "app.cpu_idle_ms": Metric("cpu", "空闲窗口 CPU（30s）", "ms", "仅记录", "app-metrics"),
    # ---- size 体积 ----
    "app.binary_x64_kb": Metric("size", "二进制体积 x64（release）", "kb", "仅记录", "app-metrics"),
    "app.binary_arm64_kb": Metric("size", "二进制体积 arm64（release）", "kb", "仅记录", "app-metrics"),
    # ---- hotpath 热路径 ----
    **_core_metrics(),
    "clone.allocs": Metric("hotpath", "热点分配合计（次数）", "count", "精确计数门禁", "criterion"),
    "clone.bytes": Metric("hotpath", "热点分配合计（字节）", "bytes", "精确计数门禁", "criterion"),
    # ---- code 代码健康 ----
    "clone.total": Metric("code", ".clone() 总数", "count", "精确计数门禁", "clone-stats"),
    "clone.density": Metric("code", ".clone() 密度（次/千行）", "count", "仅记录", "clone-stats"),
    # ---- external 外部依赖 ----
    "eval.task_latency_p50_ms": Metric(
        "external", "LLM 任务端到端延迟 p50", "ms", "仅记录", "eval-live", noise=0.20, note="外部服务波动"
    ),
    "eval.task_latency_p95_ms": Metric(
        "external", "LLM 任务端到端延迟 p95", "ms", "仅记录", "eval-live", noise=0.20, note="外部服务波动"
    ),
}

_KB_PER_MB = 1024
_KB_MB_SWITCH = 10 * _KB_PER_MB  # kb 值达到 10 MB 才换 MB 显示，以下保持 KB


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
        if value >= _KB_PER_MB ** 3:
            return f"{value / _KB_PER_MB ** 3:.1f} GB"
        if value >= _KB_PER_MB ** 2:
            return f"{value / _KB_PER_MB ** 2:.1f} MB"
        if value >= _KB_PER_MB:
            return f"{value / _KB_PER_MB:.1f} KB"
        return f"{value:,.0f} B"
    raise ValueError(f"未知单位 {unit!r}（词表：ms/kb/count/ns/bytes）")


def _validate():
    """注册表一致性自检，import 时执行：登记项漂移当场失败，不留到消费方。"""
    problems = []
    for mid, metric in METRICS.items():
        if metric.category not in CATEGORY_LABELS:
            problems.append(f"{mid}: 未知类别 {metric.category!r}")
        if metric.unit not in UNITS:
            problems.append(f"{mid}: 未知单位 {metric.unit!r}")
        if metric.judge not in JUDGES:
            problems.append(f"{mid}: 未知判定 {metric.judge!r}")
        if metric.source not in SOURCES:
            problems.append(f"{mid}: 未知数据源 {metric.source!r}")
        if metric.judge == "预算门禁" and mid not in BUDGET_CONSTS:
            problems.append(f"{mid}: 预算门禁指标缺 BUDGET_CONSTS 登记")
    for mid in BUDGET_CONSTS:
        metric = METRICS.get(mid)
        if metric is None or metric.judge != "预算门禁":
            problems.append(f"{mid}: BUDGET_CONSTS 指向非预算门禁指标")
    if problems:
        raise RuntimeError("指标注册表自检失败：\n" + "\n".join(problems))


_validate()


if __name__ == "__main__":
    for category_id, category_label in CATEGORIES:
        print(f"## {category_label}（{category_id}）")
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
    print(f"共 {len(METRICS)} 项（core.* {core_count} 项）")
    print("fmt 示例：")
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
