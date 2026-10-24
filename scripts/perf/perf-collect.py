#!/usr/bin/env python3
"""统一采集器：聚合全部本地测量源为一个趋势历史点，写回基线目录下的历史文
件。

数据源全部是构建产物目录下的本地产物，某源缺席则对应指标整体缺席，不造
零。核心基准墙钟：基准产物里各用例的均值点估计折成核心指标，只收注册表
核心分组内且已登记的编号，陈旧基准目录不入档。克隆分配基准：逐基准均值
取整后合计成克隆分配次数与字节数，与原门禁链同口径，基线数值可跨管线衔
接。克隆静态计数：复用统计脚本的计数函数得克隆总量与密度。显隐与启动自
检信号：自检信号文件取各源最后一条信号行（包装器产物），得显隐延迟的
（p50）与（p95）分位、帧时间、尾段、句柄数与启动耗时；行内预算字段折算
进该点预算段。应用运行时片段：应用度量脚本的产物得应用各项指标（含环境
的处理器信息）。评测实时报告：仅实时模式且字段在场时并入任务延迟分位。

点模型由提交号、日期、环境、可选预算段与指标段组成。环境各源尽力合并
（缺失留空对象）；预算段仅在该点折算了自检信号行时写入，数值随行流出，
源头是（Rust）常量，注册表只记常量名；同一提交原位覆盖（位置即时序，不
因重跑挪位），新点按日期升序插入（手动补采历史区间不打乱时序）。全部源
都缺席时拒绝写出空指标点。

写回时把旧版嵌套点（按分组键分段的点）就地规范化为现行扁平点——首次运
行即完成一次性迁移，保序，迁移点环境为空对象；旧点的应用二进制体积按其
采集机架构（x86_64）折算为对应架构栏位。
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

from metrics import CORE_GROUPS, METRICS  # noqa: E402 —— 路径引导先于导入


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

# 自检信号行字段 → 指标编号；行内字段缺席（空值）即对应指标缺席，不造零
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
# 自检信号行的预算字段到预算编号的映射，数值源头是本语言（Rust）常量随行流出
OVERLAY_BUDGET_FIELDS = {
    "budget_ms": "overlay.show_first_ms",
    "rss_tail_budget_kb": "overlay.rss_tail_growth_kb",
}
# 应用度量片段字段 → 指标编号
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
# 旧版应用段字段 → 新版指标编号；体积是单值字段，按采集机架构折算
V1_APP_FIELDS = {
    "startup_ms": "app.startup_ms",
    "startup_cpu_ms": "app.startup_cpu_ms",
    "rss_idle_kb": "app.rss_idle_kb",
    "binary_kb": "app.binary_x64_kb",
}


def fail(message):
    sys.exit(f"error: {message}")


def load_json_file(path):
    """读 JSON 文件为对象；文件缺席返回 None（源缺席），在场但坏行当场失败。"""
    if not path.is_file():
        return None
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        fail(f"{path} unparseable: {exc}")


def read_mean_ns(estimates_path):
    """estimates.json → mean.point_estimate（ns，取整）；不可读警告后跳过。

    target/criterion 是滚动状态，个别目录残缺不应挡住整点采集；跳过留
    stderr 痕迹。
    """
    try:
        mean = json.loads(estimates_path.read_text(encoding="utf-8"))["mean"]
        return round(mean["point_estimate"])
    except (OSError, KeyError, TypeError, ValueError, json.JSONDecodeError):
        print(
            f"warning: skipping {estimates_path} (mean.point_estimate unreadable)", file=sys.stderr
        )
        return None


def read_full_id(bench_dir, fallback):
    """基准目录 → criterion full_id；benchmark.json 不可读时退回目录相对路径。"""
    try:
        return json.loads((bench_dir / "new" / "benchmark.json").read_text(encoding="utf-8"))[
            "full_id"
        ]
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
                    f"warning: skipping unregistered benchmark {metric_id} (benches out of sync with the metrics.py registry?)",
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
            fail(f"{path} has an unparseable line: {exc}")
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
        fail(f"{APP_PATH} is not a JSON object")
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
        fail(f"history has an unparseable date: {raw!r}")


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
            fail(f"{path} has an unparseable line: {exc}")
        if not isinstance(point, dict):
            fail(f"{path} has a non-object line")
        points.append(migrate_point(point))
    return points


def current_commit():
    proc = subprocess.run(
        ["git", "rev-parse", "--short", "HEAD"], cwd=ROOT, capture_output=True, text=True
    )
    if proc.returncode != 0:
        fail(f"cannot get the HEAD short sha: {proc.stderr.strip()}")
    return proc.stdout.strip()


def main():
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        "--history",
        metavar="PATH",
        help=f"history.jsonl path (default {HISTORY_PATH}, relative paths resolve against the repo root)",
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
            "all data sources absent, refusing to write an empty metrics point; collect at least one source first "
            "(just bench / just clone-bench / just selftest-report / just app-metrics / just eval)"
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
        f"history point written: {rel} ({len(points)} points total, HEAD {entry['commit']}, "
        f"this point {len(metrics)} metrics, {len(budgets)} budgets)"
    )


if __name__ == "__main__":
    main()
