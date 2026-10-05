#!/usr/bin/env python3
"""把 benches/core.rs（墙钟基准）最近一次 criterion 结果快照进基线 JSON。

采集 target/criterion 下 core 四组（cache_key / complete / prompt_render /
task_cache）的均值与 95% 区间，连同运行环境（系统、CPU、rustc、commit、
时间）写入 baselines/core-baseline.json。墙钟数字与机器强相关，本基线只
作记录与展示，不做成败判定——趋势对照用 criterion 自身机制（just
bench-check / bench-summary），仅认同机同环境的历史数据。clone 分配组
（clone_allocs/clone_bytes）不在本快照内：它们随 clone-stats.json 判罚，
单一事实来源。

用法：
  scripts/perf/bench-baseline.py    快照并覆盖 core-baseline.json（需先跑 just bench）
"""

import json
import platform
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
ROOT = SCRIPT_DIR.parents[1]
CRIT_ROOT = ROOT / "target" / "criterion"
BASELINE_PATH = SCRIPT_DIR / "baselines" / "core-baseline.json"

GROUPS = ("cache_key", "complete", "prompt_render", "task_cache")


def run_text(cmd):
    try:
        return subprocess.run(
            cmd, capture_output=True, text=True, check=True, timeout=10
        ).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        return "unknown"


def collect_env():
    cpu = "unknown"
    if sys.platform == "darwin":
        cpu = run_text(["sysctl", "-n", "machdep.cpu.brand_string"])
    elif Path("/proc/cpuinfo").is_file():
        for line in Path("/proc/cpuinfo").read_text(encoding="utf-8").splitlines():
            if line.startswith("model name"):
                cpu = line.split(":", 1)[1].strip()
                break
    return {
        "generated_at": datetime.now(timezone.utc).astimezone().isoformat(timespec="seconds"),
        "os": platform.platform(),
        "machine": platform.machine(),
        "cpu": cpu,
        "rustc": run_text(["rustc", "--version"]),
        "commit": run_text(["git", "rev-parse", "--short", "HEAD"]),
    }


def load_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def snapshot():
    benches = {}
    for group in GROUPS:
        for estimates in sorted(CRIT_ROOT.glob(f"{group}/*/new/estimates.json")):
            bench_dir = estimates.parent.parent
            try:
                mean = load_json(estimates)["mean"]
                ci = mean["confidence_interval"]
                entry = {
                    "mean_ns": round(mean["point_estimate"]),
                    "ci_lo_ns": round(ci["lower_bound"]),
                    "ci_hi_ns": round(ci["upper_bound"]),
                }
            except (OSError, KeyError, json.JSONDecodeError):
                print(f"警告：跳过 {bench_dir.name}（estimates.json 不可读）", file=sys.stderr)
                continue
            try:
                full_id = load_json(bench_dir / "new" / "benchmark.json")["full_id"]
            except (OSError, KeyError, json.JSONDecodeError):
                full_id = bench_dir.relative_to(CRIT_ROOT).as_posix()
            benches[full_id] = entry
    return benches


def main():
    if not CRIT_ROOT.is_dir():
        print("提示：target/criterion 不存在，跳过 core 快照（先跑 just bench）。", file=sys.stderr)
        return
    benches = snapshot()
    if not benches:
        print("提示：target/criterion 下没有 core 基准数据，跳过快照（先跑 just bench）。", file=sys.stderr)
        return
    payload = {"env": collect_env(), "benches": benches}
    BASELINE_PATH.parent.mkdir(parents=True, exist_ok=True)
    BASELINE_PATH.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(f"core 基线已快照：{BASELINE_PATH.relative_to(ROOT)}（{len(benches)} 项，环境 {payload['env']['os']}）")


if __name__ == "__main__":
    main()
