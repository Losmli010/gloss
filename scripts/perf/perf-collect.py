#!/usr/bin/env python3
"""统一采集器：聚合全部本地测量源为一个 v2 历史点，写回 baselines/history.jsonl。

数据源（全部 target/ 本地产物，某源缺席 → 对应 id 整体缺席，不造 0）：
  criterion core 墙钟   target/criterion/<组>/<用例>/new/estimates.json 的
                        mean.point_estimate → core.<full_id>；只收注册表
                        CORE_GROUPS 内且已登记的 id，陈旧基准目录不入档
  criterion clone 分配  target/criterion/clone_allocs/* 与 clone_bytes/* →
                        clone.allocs / clone.bytes（逐基准均值取整后合计，
                        与原门禁链同口径，基线数值可跨管线衔接）
  clone 静态计数        复用 clone_stats 的计数函数 → clone.total / clone.density
  L3 自检信号           target/perf/selftest.jsonl（selftest_wrapper 产物，
                        取各源最后一条信号行）→ overlay.* 延迟分位/帧时间/
                        尾段/句柄与 startup.elapsed_ms；行内 budget_ms /
                        rss_tail_budget_kb 折算进该点 budgets 段
  app 运行时片段        target/perf/app-runtime.json（app-metrics 产物）→
                        app.*（含 env.cpu）
  eval live 报告        target/eval/live.json → eval.task_latency_p50/p95_ms，
                        仅 mode=="live" 且字段在场时并入

点模型 {commit, date, env, budgets?, metrics}：env 各源尽力合并（缺失留空
对象）；budgets 仅在该点折算了自检信号行时写入，数值随行流出（源头是 Rust
常量，metrics.py 只记常量名）；同 commit 原位覆盖（位置即时序，不因重跑挪
位），新点按 date 升序插入（手动补采历史区间不打乱时序）。全部源都缺席时
拒绝写出空指标点。

写回时把 v1 嵌套点（clone/core/app 分组键）就地规范化为 v2 扁平点——首次
运行即完成一次性迁移，保序，迁移点 env 为空对象；v1 的 app.binary_kb 按
其采集机架构（x86_64）折算为 app.binary_x64_kb。

用法：
  scripts/perf/perf-collect.py
  scripts/perf/perf-collect.py --history /tmp/h.jsonl    覆盖 history 路径（相对路径按仓库根解析）
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

from metrics import CORE_GROUPS, METRICS


def _load_sibling_script(module_name, file_name):
    """按路径加载同目录脚本（文件名含连字符，import 语句加载不了）。"""
    spec = importlib.util.spec_from_file_location(module_name, SCRIPT_DIR / file_name)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


clone_stats = _load_sibling_script("clone_stats", "clone-stats.py")

ROOT = SCRIPT_DIR.parents[1]
CRIT_ROOT = ROOT / "target" / "criterion"
SELFTEST_PATH = ROOT / "target" / "perf" / "selftest.jsonl"
APP_PATH = ROOT / "target" / "perf" / "app-runtime.json"
EVAL_PATH = ROOT / "target" / "eval" / "live.json"
HISTORY_PATH = SCRIPT_DIR / "baselines" / "history.jsonl"

STARTUP_SIGNAL_MESSAGE = "startup self-test finished"
OVERLAY_SIGNAL_KIND = "overlay_perf"

# overlay_perf 行字段 → 指标 id；行内字段缺席（null）即对应 id 缺席，不造 0
OVERLAY_METRIC_FIELDS = {
    "first_ms": "overlay.show_first_ms",
    "p50_ms": "overlay.show_p50_ms",
    "p95_ms": "overlay.show_p95_ms",
    "max_ms": "overlay.show_max_ms",
    "hide_p50_ms": "overlay.hide_p50_ms",
    "hide_max_ms": "overlay.hide_max_ms",
    "frame_p95_ms": "overlay.frame_p95_ms",
    "frame_missed": "overlay.frame_missed",
    "rss_growth_kb": "overlay.rss_growth_kb",
    "rss_tail_growth_kb": "overlay.rss_tail_growth_kb",
    "window_handles": "overlay.window_handles",
}
# overlay_perf 行的预算字段 → 预算 id（数值源头是 Rust 常量随行流出）
OVERLAY_BUDGET_FIELDS = {
    "budget_ms": "overlay.show_first_ms",
    "rss_tail_budget_kb": "overlay.rss_tail_growth_kb",
}
# app-metrics v2 片段字段 → 指标 id
APP_METRIC_FIELDS = {
    "startup_ms": "app.startup_ms",
    "startup_cpu_ms": "app.startup_cpu_ms",
    "cpu_idle_ms": "app.cpu_idle_ms",
    "rss_idle_kb": "app.rss_idle_kb",
    "rss_peak_kb": "app.rss_peak_kb",
    "footprint_idle_kb": "app.footprint_idle_kb",
    "footprint_peak_kb": "app.footprint_peak_kb",
    "binary_x64_kb": "app.binary_x64_kb",
    "binary_arm64_kb": "app.binary_arm64_kb",
}
# v1 app 段字段 → v2 指标 id；binary_kb 是单值字段，按采集机（x86_64）折算
V1_APP_FIELDS = {
    "startup_ms": "app.startup_ms",
    "startup_cpu_ms": "app.startup_cpu_ms",
    "rss_idle_kb": "app.rss_idle_kb",
    "binary_kb": "app.binary_x64_kb",
}


def fail(message):
    sys.exit(f"错误：{message}")


def load_json_file(path):
    """读 JSON 文件为对象；文件缺席返回 None（源缺席），在场但坏行当场失败。"""
    if not path.is_file():
        return None
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        fail(f"{path} 无法解析：{exc}")


def read_mean_ns(estimates_path):
    """estimates.json → mean.point_estimate（ns，取整）；不可读警告后跳过。

    target/criterion 是滚动状态，个别目录残缺不应挡住整点采集；跳过留
    stderr 痕迹。
    """
    try:
        mean = json.loads(estimates_path.read_text(encoding="utf-8"))["mean"]
        return round(mean["point_estimate"])
    except (OSError, KeyError, TypeError, ValueError, json.JSONDecodeError):
        print(f"警告：跳过 {estimates_path}（mean.point_estimate 不可读）", file=sys.stderr)
        return None


def read_full_id(bench_dir, fallback):
    """基准目录 → criterion full_id；benchmark.json 不可读时退回目录相对路径。"""
    try:
        return json.loads((bench_dir / "new" / "benchmark.json").read_text(encoding="utf-8"))["full_id"]
    except (OSError, KeyError, json.JSONDecodeError):
        return fallback


def collect_core():
    """criterion core 墙钟 → {core.<full_id>: mean_ns}，只收注册表已登记 id。"""
    metrics = {}
    for group, _label in CORE_GROUPS:
        for estimates in sorted(CRIT_ROOT.glob(f"{group}/*/new/estimates.json")):
            bench_dir = estimates.parent.parent
            full_id = read_full_id(bench_dir, bench_dir.relative_to(CRIT_ROOT).as_posix())
            metric_id = f"core.{full_id}"
            if metric_id not in METRICS:
                print(
                    f"警告：跳过未登记的基准 {metric_id}（benches 与 metrics.py 注册表失同步？）",
                    file=sys.stderr,
                )
                continue
            mean_ns = read_mean_ns(estimates)
            if mean_ns is not None:
                metrics[metric_id] = mean_ns
    return metrics


def collect_clone_allocs():
    """clone_allocs/clone_bytes 两组 → 分配次数/字节合计（无数据即 id 缺席）。"""
    metrics = {}
    for group, metric_id in (("clone_allocs", "clone.allocs"), ("clone_bytes", "clone.bytes")):
        values = []
        for estimates in sorted(CRIT_ROOT.glob(f"{group}/*/new/estimates.json")):
            mean = read_mean_ns(estimates)
            if mean is not None:
                values.append(mean)
        if values:
            metrics[metric_id] = sum(values)
    return metrics


def collect_clone_static():
    """clone-stats 静态计数 → clone.total / clone.density（聚合口径同原管线）。"""
    crates, _files = clone_stats.collect()
    total = sum(crate["clone"] for crate in crates.values())
    total_loc = sum(crate["loc"] for crate in crates.values())
    return {
        "clone.total": total,
        "clone.density": round(clone_stats.density(total, total_loc), 1),
    }


def load_selftest_signals(path):
    """selftest.jsonl → 各源最后一条信号行（wrapper 每次整文件覆写落盘）。

    overlay 信号按 kind 识别，startup 信号按日志行 message 识别（verdict 由
    退出码承载，不进行内字段）。文件缺席返回空表；在场但有坏行当场失败——
    信号行是这些 id 的唯一来源，静默跳过等于无痕丢数据。
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
        except json.JSONDecodeError as exc:
            fail(f"{path} 存在无法解析的行：{exc}")
        if not isinstance(record, dict):
            continue
        if record.get("kind") == OVERLAY_SIGNAL_KIND:
            signals["overlay"] = record
        elif record.get("message") == STARTUP_SIGNAL_MESSAGE:
            signals["startup"] = record
    return signals


def collect_selftest(signals):
    """自检信号行 → (metrics, budgets, env)；行内字段缺席即对应 id 缺席。"""
    metrics, budgets, env = {}, {}, {}
    overlay = signals.get("overlay")
    if overlay:
        for field, metric_id in OVERLAY_METRIC_FIELDS.items():
            value = overlay.get(field)
            if value is not None:
                metrics[metric_id] = value
        for field, metric_id in OVERLAY_BUDGET_FIELDS.items():
            value = overlay.get(field)
            if value is not None:
                budgets[metric_id] = value
        line_env = overlay.get("env")
        if isinstance(line_env, dict):
            env.update(line_env)
    startup = signals.get("startup")
    if startup:
        elapsed = startup.get("elapsed_ms")
        if elapsed is not None:
            metrics["startup.elapsed_ms"] = elapsed
        budget = startup.get("budget_ms")
        if budget is not None:
            budgets["startup.elapsed_ms"] = budget
    return metrics, budgets, env


def collect_app():
    """app-metrics v2 片段 → (metrics, env)；片段缺席或字段 null 即 id 缺席。"""
    data = load_json_file(APP_PATH)
    if data is None:
        return {}, {}
    if not isinstance(data, dict):
        fail(f"{APP_PATH} 不是 JSON 对象")
    metrics = {}
    for field, metric_id in APP_METRIC_FIELDS.items():
        value = data.get(field)
        if value is not None:
            metrics[metric_id] = value
    env = data.get("env")
    return metrics, (env if isinstance(env, dict) else {})


def collect_eval():
    """eval live 报告 → 端到端延迟分位；非 live 模式或字段缺席即 id 缺席。"""
    data = load_json_file(EVAL_PATH)
    if data is None or data.get("mode") != "live":
        return {}
    latency = data.get("latency")
    metrics = {}
    if isinstance(latency, dict):
        p50 = latency.get("p50")
        p95 = latency.get("p95")
        if p50 is not None:
            metrics["eval.task_latency_p50_ms"] = p50
        if p95 is not None:
            metrics["eval.task_latency_p95_ms"] = p95
    return metrics


def migrate_point(point):
    """v1 嵌套点 → v2 扁平点（就地规范化）；已带 metrics 段的 v2 点原样返回。"""
    if "metrics" in point:
        return point
    metrics = {}
    clone = point.get("clone") or {}
    for key in ("total", "density", "allocs", "bytes"):
        if clone.get(key) is not None:
            metrics[f"clone.{key}"] = clone[key]
    core = point.get("core") or {}
    for full_id, mean_ns in core.items():
        metrics[f"core.{full_id}"] = mean_ns
    app = point.get("app") or {}
    for field, metric_id in V1_APP_FIELDS.items():
        if app.get(field) is not None:
            metrics[metric_id] = app[field]
    return {"commit": point.get("commit"), "date": point.get("date"), "env": {}, "metrics": metrics}


def parse_date(raw):
    try:
        return datetime.fromisoformat(raw)
    except (TypeError, ValueError):
        fail(f"history 中存在无法解析的 date：{raw!r}")


def insert_point(points, entry):
    """同 commit 原位覆盖（位置即时序）；新点按 date 升序插入，不打乱既有顺序。"""
    replaced = False
    merged = []
    for old in points:
        if old.get("commit") == entry["commit"]:
            merged.append(entry)
            replaced = True
        else:
            merged.append(old)
    if replaced:
        return merged
    new_date = parse_date(entry["date"])
    index = len(merged)
    for i, old in enumerate(merged):
        if parse_date(old.get("date")) > new_date:
            index = i
            break
    return merged[:index] + [entry] + merged[index:]


def load_history(path):
    points = []
    if not path.is_file():
        return points
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            point = json.loads(line)
        except json.JSONDecodeError as exc:
            fail(f"{path} 存在无法解析的行：{exc}")
        if not isinstance(point, dict):
            fail(f"{path} 存在非对象行")
        points.append(migrate_point(point))
    return points


def current_commit():
    proc = subprocess.run(
        ["git", "rev-parse", "--short", "HEAD"], cwd=ROOT, capture_output=True, text=True
    )
    if proc.returncode != 0:
        fail(f"无法取得 HEAD 短 sha：{proc.stderr.strip()}")
    return proc.stdout.strip()


def main():
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        "--history",
        metavar="PATH",
        help=f"history.jsonl 路径（缺省 {HISTORY_PATH}，相对路径按仓库根解析）",
    )
    args = parser.parse_args()
    history_path = Path(args.history) if args.history else HISTORY_PATH
    if not history_path.is_absolute():
        history_path = ROOT / history_path

    metrics = {}
    env = {}
    metrics.update(collect_core())
    metrics.update(collect_clone_allocs())
    metrics.update(collect_clone_static())
    selftest_metrics, budgets, selftest_env = collect_selftest(load_selftest_signals(SELFTEST_PATH))
    metrics.update(selftest_metrics)
    env.update(selftest_env)
    app_metrics, app_env = collect_app()
    metrics.update(app_metrics)
    env.update(app_env)
    metrics.update(collect_eval())
    if not metrics:
        fail(
            "全部数据源缺席，拒绝写出空指标点；先采集至少一个源"
            "（just bench / just clone-bench / just selftest-report / just app-metrics / just eval）"
        )

    entry = {
        "commit": current_commit(),
        "date": datetime.now(timezone.utc).astimezone().isoformat(timespec="seconds"),
        "env": env,
        "metrics": metrics,
    }
    if budgets:
        entry["budgets"] = budgets

    points = insert_point(load_history(history_path), entry)
    history_path.parent.mkdir(parents=True, exist_ok=True)
    history_path.write_text(
        "".join(json.dumps(point, ensure_ascii=False, sort_keys=True) + "\n" for point in points),
        encoding="utf-8",
    )
    try:
        rel = history_path.relative_to(ROOT)
    except ValueError:
        rel = history_path
    print(
        f"历史点已写回：{rel}（共 {len(points)} 点，HEAD {entry['commit']}，"
        f"本点 {len(metrics)} 项指标、{len(budgets)} 项预算）"
    )


if __name__ == "__main__":
    main()
