#!/usr/bin/env python3
"""性能总览与对比的 Markdown 报告（stdout）：趋势数据的人读静态视图。

数据来自 baselines/history.jsonl（v2 扁平点）与 target/perf/selftest.jsonl
（L3 自检信号 handoff）。两个视图：

  总览（缺省）        最新点的全部注册指标：最新值、vs 上一点、vs 首点、判定。
                      预算门禁指标转述自检信号（预算数值 + PASS/FAIL），其余
                      行呈现注册表的判定语义（judge 与 note 括注）。
  对比（--from/--to）  两期点按指标 id 对齐：改善/回归/持平/无数据摘要 + 明细表。
                      精确计数门禁类新值上升即回归（与 clone-check 同向），其余
                      按注册表 noise 阈值启发式判定；判定只作参考，表头与摘要
                      均标注「非门禁」。

预算判定转述口径：PASS ⟺ 自检信号行携带的该指标值 ≤ 行内携带的预算字段（与
自检退出码的判定是同一比较），信号文件、信号行或任一字段缺失即「—」；门禁
判定权始终在 L3 自检退出码，本报告不判罚。

指标 id、中文类别、单位格式化（fmt）、noise 阈值全部取自 metrics.py 注册表，
点里不在注册表的 id 不呈现。缺源即缺行：最新点（或对比两点任一）缺某 id 时
不出行也不造 0；对比摘要的无数据 = 注册表 id 在两点恰缺其一的个数。v1 旧点
无 metrics 段按缺源处理，就地迁移是 collector（perf-collect.py）的职责。

用法：
  scripts/perf/perf-report.py                          总览最新点
  scripts/perf/perf-report.py --from a1b2c3d --to e4f5a6b   两期对比
  scripts/perf/perf-report.py --history /tmp/h.jsonl --selftest /tmp/s.jsonl
                                                       覆盖数据路径（相对路径按仓库根解析）
"""

import argparse
import json
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

from metrics import CATEGORY_LABELS, METRICS, fmt

ROOT = SCRIPT_DIR.parents[1]
DEFAULT_HISTORY = SCRIPT_DIR / "baselines" / "history.jsonl"
DEFAULT_SELFTEST = Path("target/perf/selftest.jsonl")

STARTUP_SIGNAL_MESSAGE = "startup self-test finished"
OVERLAY_SIGNAL_KIND = "overlay_perf"

# 预算指标 id → 自检信号行上的（指标值字段, 预算字段）；行内字段即自检判定的
# 同源数据。与注册表预算门禁指标的对应关系由 _validate_budget_signal_fields
# 在 import 期校验，漂移当场带指引失败。
BUDGET_SIGNAL_FIELDS = {
    "overlay.show_first_ms": ("first_ms", "budget_ms"),
    "overlay.rss_tail_growth_kb": ("rss_tail_growth_kb", "rss_tail_budget_kb"),
    "startup.elapsed_ms": ("elapsed_ms", "budget_ms"),
}


def _validate_budget_signal_fields():
    """import 期校验：信号字段映射与注册表预算门禁指标一一对应。

    登记项漂移当场失败（与 metrics.py 的 import 期自检同模式），不留到带
    信号文件的运行才以裸 KeyError 暴露。
    """
    budget_ids = {mid for mid, metric in METRICS.items() if metric.judge == "预算门禁"}
    mapped = set(BUDGET_SIGNAL_FIELDS)
    if budget_ids != mapped:
        missing = "、".join(sorted(budget_ids - mapped)) or "无"
        extra = "、".join(sorted(mapped - budget_ids)) or "无"
        raise SystemExit(
            "错误：BUDGET_SIGNAL_FIELDS 与注册表预算门禁指标不一致"
            f"（缺映射：{missing}；多余映射：{extra}）；"
            "新增预算指标时同步此映射（对照 metrics.py 的 BUDGET_CONSTS）"
        )


_validate_budget_signal_fields()


def resolve_path(raw):
    path = Path(raw)
    return path if path.is_absolute() else ROOT / path


def load_points(path):
    """读 history.jsonl 为点列表，保文件序（collector 按 date 排序写入即时间序）。"""
    if not path.is_file():
        raise SystemExit(f"错误：历史文件不存在：{path}（先运行采集，如 just perf-baseline）")
    points = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            point = json.loads(line)
        except json.JSONDecodeError as exc:
            raise SystemExit(f"错误：{path} 存在无法解析的行：{exc}")
        if not isinstance(point, dict):
            raise SystemExit(f"错误：{path} 存在非对象行")
        points.append(point)
    if not points:
        raise SystemExit(f"错误：{path} 没有数据点")
    return points


def point_metrics(point):
    """取点的 metrics 段；缺段（v1 旧点）按缺源返回空。"""
    metrics = point.get("metrics")
    return metrics if isinstance(metrics, dict) else {}


def find_point(points, commit):
    for point in points:
        if point.get("commit") == commit:
            return point
    known = "、".join(str(point.get("commit", "?")) for point in points)
    raise SystemExit(f"错误：历史中没有提交 {commit}（可用：{known}）")


def earlier_value(points, metric_id, latest_first):
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


def latest_budget(points, metric_id):
    """「当前预算」＝历史里最新携带该预算的点上的数值（预算变更自然形成时间线）。"""
    for point in reversed(points):
        budgets = point.get("budgets")
        if isinstance(budgets, dict) and metric_id in budgets:
            return budgets[metric_id]
    return None


def latest_env(points):
    """页头环境行取最新点的 env；最新点为空（v1 迁移点）回退最新非空 env。"""
    for point in reversed(points):
        env = point.get("env")
        if isinstance(env, dict) and env:
            return env
    return {}


def env_line(env):
    left = " ".join(part for part in (env.get("os"), env.get("arch")) if part)
    cpu = env.get("cpu", "")
    if cpu and left:
        return f"{left} / {cpu}"
    return left or cpu


def change_cell(new, old):
    """相对变化百分比单元格；旧值为 0 无法定义百分比，标「—」。"""
    if old == 0:
        return "—"
    return f"{(new - old) / old * 100:.1f}%"


def load_selftest_signals(path):
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


def signal_verdict(metric_id, metric, signals):
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


def judge_cell(metric):
    """总览判定列的非预算行：注册表的 judge + note 括注（与设计 4.4 样例一致）。

    这类行没有可转述的判定信号（判定权在 clone-check 与自检退出码），只
    呈现判定语义本身，note 单源在注册表。
    """
    return f"{metric.judge}（{metric.note}）" if metric.note else metric.judge


def verdict_cell(points, metric_id, metric, signals):
    """总览判定列：预算指标转述 PASS/FAIL 并标注预算数值，其余行呈现 judge+note。

    预算数值取最新携带预算的历史点；预算与判定单独缺席时只展示在场的部分，
    都不缺才组合成完整判定。
    """
    if metric.judge != "预算门禁":
        return judge_cell(metric)
    verdict = signal_verdict(metric_id, metric, signals)
    budget = latest_budget(points, metric_id)
    if budget is None:
        return verdict if verdict is not None else "—"
    cell = f"预算 {fmt(budget, metric.unit)}"
    return f"{cell} · {verdict}" if verdict is not None else cell


def render_overview(points, signals):
    latest = points[-1]
    metrics = point_metrics(latest)
    lines = ["## 总览", ""]
    head = f"Gloss 性能总览 · {latest.get('commit', '?')}（{str(latest.get('date', ''))[:10] or '?'}）"
    env_text = env_line(latest_env(points))
    if env_text:
        head += f"· {env_text}"
    lines += [head, ""]
    lines += [
        "| 类别 | 指标 | 最新 | vs 上一点 | vs 首点 | 判定 |",
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


def compare_verdict(metric, old, new):
    """对比参考列：精确计数类整数对照（与 clone-check 同向），其余按 noise 阈值。"""
    if metric.judge == "精确计数门禁":
        if new > old:
            return "回归"
        if new < old:
            return "改善"
        return "持平（精确计数）"
    if old == 0:
        return "持平" if new == 0 else "回归"
    relative = (new - old) / old
    if abs(relative) <= metric.noise:
        return "持平"
    return "回归" if relative > 0 else "改善"


def render_compare(old_point, new_point, from_commit, to_commit):
    old_metrics, new_metrics = point_metrics(old_point), point_metrics(new_point)
    improved = regressed = flat = 0
    nodata = sum(1 for metric_id in METRICS if (metric_id in old_metrics) != (metric_id in new_metrics))
    rows = []
    for metric_id, metric in METRICS.items():
        if metric_id not in old_metrics or metric_id not in new_metrics:
            continue
        old, new = old_metrics[metric_id], new_metrics[metric_id]
        verdict = compare_verdict(metric, old, new)
        if verdict == "改善":
            improved += 1
        elif verdict == "回归":
            regressed += 1
        else:
            flat += 1
        rows.append(
            f"| {CATEGORY_LABELS[metric.category]} | {metric_id} | {fmt(old, metric.unit)} "
            f"| {fmt(new, metric.unit)} | {fmt(new - old, metric.unit)} "
            f"| {change_cell(new, old)} | {verdict} |"
        )
    lines = [
        f"## 对比（--from {from_commit} --to {to_commit}，判定为启发式参考，非门禁）",
        "",
        f"改善 {improved} · 回归 {regressed} · 持平 {flat} · 无数据 {nodata}（非门禁，仅参考）",
        "",
        "| 类别 | 指标 | 旧 | 新 | Δ | Δ% | 参考 |",
        "| --- | --- | ---: | ---: | ---: | ---: | --- |",
        *rows,
    ]
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(
        description="性能总览与对比的 Markdown 报告（stdout，判定仅参考非门禁）"
    )
    parser.add_argument("--from", dest="from_commit", metavar="SHA", help="对比起点提交短 sha")
    parser.add_argument("--to", dest="to_commit", metavar="SHA", help="对比终点提交短 sha")
    parser.add_argument(
        "--history",
        metavar="PATH",
        help=f"history.jsonl 路径（缺省 {DEFAULT_HISTORY}）",
    )
    parser.add_argument(
        "--selftest",
        metavar="PATH",
        help=f"自检信号文件路径（缺省 {DEFAULT_SELFTEST}，相对路径按仓库根解析）",
    )
    args = parser.parse_args()
    if (args.from_commit is None) != (args.to_commit is None):
        parser.error("对比模式需要 --from 与 --to 成对提供")

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
