#!/usr/bin/env python3
"""性能总览与对比的（Markdown）报告，输出到标准输出：趋势数据的人读静态视
图。

数据来自基线历史文件（现行扁平点）与（L3）自检信号交接文件。两个视图：
总览视图（缺省）呈现最新点的全部注册指标——最新值、相对上一点与相对首
点的变化、判定；预算门禁指标转述自检信号（预算数值与通过与否），其余行
呈现注册表的判定语义（判定档位与补充说明括注）。对比视图把两期点按指标
编号对齐，给出改善、回归、持平与无数据的摘要加明细表；精确计数门禁类新
值上升即回归（与克隆校验同向），其余按注册表噪声阈值启发式判定；判定只
作参考，表头与摘要均标注“非门禁”。

预算判定转述口径：自检信号行携带的该指标值不大于行内携带的预算字段即通
过（与自检退出码的判定是同一比较），信号文件、信号行或任一字段缺失即留
空占位；门禁判定权始终在（L3）自检退出码，本报告不判罚。

指标编号、中文类别、单位格式化与噪声阈值全部取自注册表，点里不在注册表
的编号不呈现。缺源即缺行：最新点（或对比两点任一）缺某编号时不出行也不
造零；对比摘要的无数据数即注册表编号在两点恰缺其一的个数。旧版点无指标
段按缺源处理，就地迁移是统一采集器的职责。
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

from metrics import CATEGORY_LABELS, METRICS, Metric, fmt  # noqa: E402 —— 路径引导先于导入

ROOT = SCRIPT_DIR.parents[1]
DEFAULT_HISTORY = SCRIPT_DIR / "baselines" / "history.jsonl"
DEFAULT_SELFTEST = Path("target/perf/selftest.jsonl")

STARTUP_SIGNAL_MESSAGE = "startup self-test finished"
OVERLAY_SIGNAL_KIND = "overlay_perf"

# 预算指标编号到自检信号行上指标值字段与预算字段的映射；行内字段即自检判定的
# 同源数据。与注册表预算门禁指标的对应关系在导入期校验，漂移当场带指引失败。
BUDGET_SIGNAL_FIELDS = {
    "overlay.show_first_ms": ("first_ms", "budget_ms"),
    "overlay.rss_tail_growth_kb": ("rss_tail_growth_kb", "rss_tail_budget_kb"),
    "startup.elapsed_ms": ("elapsed_ms", "budget_ms"),
}


def _validate_budget_signal_fields() -> None:
    """import 期校验：信号字段映射与注册表预算门禁指标一一对应。

    登记项漂移当场失败（与 metrics.py 的 import 期自检同模式），不留到带
    信号文件的运行才以裸 KeyError 暴露。
    """
    budget_ids = {mid for mid, metric in METRICS.items() if metric.judge == "budget gate"}
    mapped = set(BUDGET_SIGNAL_FIELDS)
    if budget_ids != mapped:
        missing = ", ".join(sorted(budget_ids - mapped)) or "none"
        extra = ", ".join(sorted(mapped - budget_ids)) or "none"
        raise SystemExit(
            "error: BUDGET_SIGNAL_FIELDS inconsistent with the registry budget-gate metrics "
            f"(missing mappings: {missing}; extra mappings: {extra}); "
            "update this mapping when adding budget metrics (see metrics.py BUDGET_CONSTS)"
        )


_validate_budget_signal_fields()


def resolve_path(raw: str) -> Path:
    path = Path(raw)
    return path if path.is_absolute() else ROOT / path


def load_points(path: Path) -> list[dict[str, Any]]:
    """读 history.jsonl 为点列表，保文件序（collector 按 date 排序写入即时间序）。"""
    if not path.is_file():
        raise SystemExit(
            f"error: history file missing: {path} (run the collector first, e.g. just perf-baseline)"
        )
    points = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            point = json.loads(line)
        except json.JSONDecodeError as exc:
            raise SystemExit(f"error: {path} has an unparseable line: {exc}") from None
        if not isinstance(point, dict):
            raise SystemExit(f"error: {path} has a non-object line")
        points.append(point)
    if not points:
        raise SystemExit(f"error: {path} has no data points")
    return points


def point_metrics(point: dict[str, Any]) -> dict[str, Any]:
    """取点的 metrics 段；缺段（v1 旧点）按缺源返回空。"""
    metrics = point.get("metrics")
    return metrics if isinstance(metrics, dict) else {}


def find_point(points: list[dict[str, Any]], commit: str) -> dict[str, Any]:
    for point in points:
        if point.get("commit") == commit:
            return point
    known = ", ".join(str(point.get("commit", "?")) for point in points)
    raise SystemExit(f"error: commit {commit} not in history (available: {known})")


def earlier_value(
    points: list[dict[str, Any]], metric_id: str, latest_first: bool
) -> int | float | None:
    """最新点之前该指标最近/最早的点值；之前无点携带该 id 则 None（vs 列标「—」）。

    对比锚点是该 id 自身的上一/首个数据点（与图表数值槽同口径），不是文件里
    相邻的点——某源中途缺席不制造伪变化。
    """
    scope = points[:-1]
    for point in reversed(scope) if latest_first else scope:
        value = point_metrics(point).get(metric_id)
        if value is not None:
            return value
    return None


def latest_budget(points: list[dict[str, Any]], metric_id: str) -> int | float | None:
    """「当前预算」＝历史里最新携带该预算的点上的数值（预算变更自然形成时间线）。"""
    for point in reversed(points):
        budgets = point.get("budgets")
        if isinstance(budgets, dict) and metric_id in budgets:
            return budgets[metric_id]
    return None


def latest_env(points: list[dict[str, Any]]) -> dict[str, Any]:
    """页头环境行取最新点的 env；最新点为空（v1 迁移点）回退最新非空 env。"""
    for point in reversed(points):
        env = point.get("env")
        if isinstance(env, dict) and env:
            return env
    return {}


def env_line(env: dict[str, Any]) -> str:
    left = " ".join(part for part in (env.get("os"), env.get("arch")) if part)
    cpu = env.get("cpu", "")
    if cpu and left:
        return f"{left} / {cpu}"
    return left or cpu


def change_cell(new: int | float, old: int | float) -> str:
    """相对变化百分比单元格；旧值为 0 无法定义百分比，标「—」。"""
    if old == 0:
        return "—"
    return f"{(new - old) / old * 100:.1f}%"


def load_selftest_signals(path: Path) -> dict[str, Any]:
    """提取自检信号文件里各源最新的一条信号行，缺文件/坏行容忍（总览判「—」）。

    wrapper 按采集顺序整文件覆写落盘；这里对同源信号取最后一条，兼容将来
    改为追加的落盘语义。overlay 信号按 kind 识别，startup 信号按日志行的
    message 识别（verdict 由退出码承载，不进行内字段）。
    """
    signals = {}
    if not path.is_file():
        return signals
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(record, dict):
            continue
        if record.get("kind") == OVERLAY_SIGNAL_KIND:
            signals["overlay-selftest"] = record
        elif record.get("message") == STARTUP_SIGNAL_MESSAGE:
            signals["startup-selftest"] = record
    return signals


def signal_verdict(metric_id: str, metric: Metric, signals: dict[str, Any]) -> str | None:
    """从自检信号行转述单指标 PASS/FAIL（行内值 ≤ 行内预算字段）。

    信号行未携带该指标的值或预算字段（含整个信号文件缺席）即 None，总览标
    「—」；判定权在自检退出码，这里只转述同一比较的结果。
    """
    signal = signals.get(metric.source)
    if signal is None:
        return None
    value_field, budget_field = BUDGET_SIGNAL_FIELDS[metric_id]
    value, budget = signal.get(value_field), signal.get(budget_field)
    if value is None or budget is None:
        return None
    return "PASS" if value <= budget else "FAIL"


def judge_cell(metric: Metric) -> str:
    """总览判定列的非预算行：注册表的 judge + note 括注（与设计 4.4 样例一致）。

    这类行没有可转述的判定信号（判定权在 clone-check 与自检退出码），只
    呈现判定语义本身，note 单源在注册表。
    """
    return f"{metric.judge} ({metric.note})" if metric.note else metric.judge


def verdict_cell(
    points: list[dict[str, Any]], metric_id: str, metric: Metric, signals: dict[str, Any]
) -> str:
    """总览判定列：预算指标转述 PASS/FAIL 并标注预算数值，其余行呈现 judge+note。

    预算数值取最新携带预算的历史点；预算与判定单独缺席时只展示在场的部分，
    都不缺才组合成完整判定。
    """
    if metric.judge != "budget gate":
        return judge_cell(metric)
    verdict = signal_verdict(metric_id, metric, signals)
    budget = latest_budget(points, metric_id)
    if budget is None:
        return verdict if verdict is not None else "—"
    cell = f"budget {fmt(budget, metric.unit)}"
    return f"{cell} · {verdict}" if verdict is not None else cell


def render_overview(points: list[dict[str, Any]], signals: dict[str, Any]) -> str:
    latest = points[-1]
    metrics = point_metrics(latest)
    lines = ["## overview", ""]
    head = f"Gloss perf overview · {latest.get('commit', '?')} ({str(latest.get('date', ''))[:10] or '?'})"
    env_text = env_line(latest_env(points))
    if env_text:
        head += f"· {env_text}"
    lines += [head, ""]
    lines += [
        "| category | metric | latest | vs previous | vs first | verdict |",
        "| --- | --- | ---: | ---: | ---: | --- |",
    ]
    for metric_id, metric in METRICS.items():
        if metric_id not in metrics:
            continue
        value = metrics[metric_id]
        previous = earlier_value(points, metric_id, latest_first=True)
        initial = earlier_value(points, metric_id, latest_first=False)
        lines.append(
            f"| {CATEGORY_LABELS[metric.category]} | {metric_id} | {fmt(value, metric.unit)} "
            f"| {change_cell(value, previous) if previous is not None else '—'} "
            f"| {change_cell(value, initial) if initial is not None else '—'} "
            f"| {verdict_cell(points, metric_id, metric, signals)} |"
        )
    return "\n".join(lines)


def compare_verdict(metric: Metric, old: int | float, new: int | float) -> str:
    """对比参考列：精确计数类整数对照（与 clone-check 同向），其余按 noise 阈值。"""
    if metric.judge == "exact count gate":
        if new > old:
            return "regressed"
        if new < old:
            return "improved"
        return "flat (exact count)"
    if old == 0:
        return "flat" if new == 0 else "regressed"
    relative = (new - old) / old
    if abs(relative) <= metric.noise:
        return "flat"
    return "regressed" if relative > 0 else "improved"


def render_compare(
    old_point: dict[str, Any],
    new_point: dict[str, Any],
    from_commit: str,
    to_commit: str,
) -> str:
    old_metrics, new_metrics = point_metrics(old_point), point_metrics(new_point)
    improved = regressed = flat = 0
    nodata = sum(
        1 for metric_id in METRICS if (metric_id in old_metrics) != (metric_id in new_metrics)
    )
    rows = []
    for metric_id, metric in METRICS.items():
        if metric_id not in old_metrics or metric_id not in new_metrics:
            continue
        old, new = old_metrics[metric_id], new_metrics[metric_id]
        verdict = compare_verdict(metric, old, new)
        if verdict == "improved":
            improved += 1
        elif verdict == "regressed":
            regressed += 1
        else:
            flat += 1
        rows.append(
            f"| {CATEGORY_LABELS[metric.category]} | {metric_id} | {fmt(old, metric.unit)} "
            f"| {fmt(new, metric.unit)} | {fmt(new - old, metric.unit)} "
            f"| {change_cell(new, old)} | {verdict} |"
        )
    lines = [
        f"## compare (--from {from_commit} --to {to_commit}, verdicts are heuristic reference, not gates)",
        "",
        f"improved {improved} · regressed {regressed} · flat {flat} · no data {nodata} (not a gate, reference only)",
        "",
        "| category | metric | old | new | delta | delta% | reference |",
        "| --- | --- | ---: | ---: | ---: | ---: | --- |",
        *rows,
    ]
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="perf overview and compare Markdown report (stdout, verdicts are reference only, not gates)"
    )
    parser.add_argument(
        "--from", dest="from_commit", metavar="SHA", help="compare-from commit short sha"
    )
    parser.add_argument("--to", dest="to_commit", metavar="SHA", help="compare-to commit short sha")
    parser.add_argument(
        "--history",
        metavar="PATH",
        help=f"history.jsonl path (default {DEFAULT_HISTORY})",
    )
    parser.add_argument(
        "--selftest",
        metavar="PATH",
        help=f"selftest signal file path (default {DEFAULT_SELFTEST}, relative paths resolve against the repo root)",
    )
    args = parser.parse_args()
    if (args.from_commit is None) != (args.to_commit is None):
        parser.error("compare mode needs --from and --to together")

    points = load_points(resolve_path(args.history) if args.history else DEFAULT_HISTORY)
    if args.from_commit:
        old_point = find_point(points, args.from_commit)
        new_point = find_point(points, args.to_commit)
        print(render_compare(old_point, new_point, args.from_commit, args.to_commit))
    else:
        selftest_path = resolve_path(args.selftest) if args.selftest else ROOT / DEFAULT_SELFTEST
        print(render_overview(points, load_selftest_signals(selftest_path)))


if __name__ == "__main__":
    main()
