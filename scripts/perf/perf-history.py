#!/usr/bin/env python3
"""把当前基线快照折算成聚合点，追加进 baselines/history.jsonl（趋势曲线的数据源）。

clone 取整体聚合口径：`.clone()` 总数、密度（次/千行）、热点分配次数合计、
分配字节合计；core 记录各组各基准的均值（ns）。clone-stats.json 缺 perf 段时
拒绝记录，避免 0 值进入趋势线。同一 commit 重跑原位覆盖旧点，不重复膨胀也
不挪动既有时序位置。明细数据仍在 clone-stats.json / core-baseline.json，
历史文件只承载曲线需要的量。

用法：
  scripts/perf/perf-history.py    依据 baselines/ 下已有 JSON 追加快照点
"""

import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
BASELINE_DIR = SCRIPT_DIR / "baselines"
HISTORY_PATH = BASELINE_DIR / "history.jsonl"


def run_text(cmd):
    try:
        return subprocess.run(
            cmd, capture_output=True, text=True, check=True, timeout=10
        ).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        return "unknown"


def load_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def clone_aggregates():
    baseline = load_json(BASELINE_DIR / "clone-stats.json")
    crates = baseline.get("crates", {})
    perf = baseline.get("perf")
    if not perf:
        sys.exit(
            "错误：clone-stats.json 缺 perf 段（无 clone_allocs/clone_bytes 基准数据），"
            "拒绝记录分配为 0 的历史点；先运行 just clone-bench 再 just perf-baseline"
        )
    total = sum(agg["clone"] for agg in crates.values())
    total_loc = sum(agg["loc"] for agg in crates.values())
    return {
        "total": total,
        "density": round(total / total_loc * 1000, 1) if total_loc else 0.0,
        "allocs": sum(e["allocs"] for e in perf.values() if "allocs" in e),
        "bytes": sum(e["bytes"] for e in perf.values() if "bytes" in e),
    }


def main():
    if not (BASELINE_DIR / "clone-stats.json").is_file():
        sys.exit("错误：缺少 clone-stats.json，先运行 just perf-baseline")
    entry = {
        "commit": run_text(["git", "rev-parse", "--short", "HEAD"]),
        "date": datetime.now(timezone.utc).astimezone().isoformat(timespec="seconds"),
        "clone": clone_aggregates(),
        "core": None,
    }
    core_path = BASELINE_DIR / "core-baseline.json"
    if core_path.is_file():
        benches = load_json(core_path).get("benches") or {}
        entry["core"] = {full_id: bench["mean_ns"] for full_id, bench in benches.items()}

    entries = []
    replaced = False
    if HISTORY_PATH.is_file():
        for line in HISTORY_PATH.read_text(encoding="utf-8").splitlines():
            if line.strip():
                old = json.loads(line)
                if old.get("commit") == entry["commit"]:
                    entries.append(entry)
                    replaced = True
                else:
                    entries.append(old)
    if not replaced:
        entries.append(entry)
    HISTORY_PATH.write_text(
        "".join(json.dumps(e, ensure_ascii=False, sort_keys=True) + "\n" for e in entries),
        encoding="utf-8",
    )
    print(f"历史快照已记录：{HISTORY_PATH.relative_to(HISTORY_PATH.parents[2])}（共 {len(entries)} 点，HEAD {entry['commit']}）")


if __name__ == "__main__":
    main()
