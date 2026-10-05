#!/usr/bin/env python3
"""汇总最近一次 criterion 基准运行为 Markdown 表。

每个基准一行：本次均值、95% 置信区间、相对对照的变化与判定。对照对象由
最近一次的运行方式决定（just bench 对照上一次运行，just bench-check 对照
命名基线），本脚本只读取数据，不运行基准。

用法：scripts/bench-summary.py [对照标签] [criterion 目录]
"""

import json
import sys
from pathlib import Path

NS_PER_MS = 1_000_000
NS_PER_US = 1_000


def human_ns(ns):
    if ns >= NS_PER_MS:
        return f"{ns / NS_PER_MS:.3f} ms"
    if ns >= NS_PER_US:
        return f"{ns / NS_PER_US:.3f} µs"
    return f"{ns:.1f} ns"


def human_ratio(frac):
    return f"{frac * 100:+.2f}%"


def load_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def load_mean(path):
    """读取 estimates.json，返回（均值点估计，区间下界，区间上界）。"""
    mean = load_json(path)["mean"]
    ci = mean["confidence_interval"]
    return mean["point_estimate"], ci["lower_bound"], ci["upper_bound"]


def display_name(bench_dir, dir_name):
    """优先取 benchmark.json 里的 full_id（含原始 / 分隔），退回目录名。"""
    try:
        return load_json(bench_dir / "new" / "benchmark.json")["full_id"]
    except (OSError, KeyError, json.JSONDecodeError):
        return dir_name


def main():
    label = sys.argv[1] if len(sys.argv) > 1 else "上一次运行"
    crit_dir = Path(sys.argv[2]) if len(sys.argv) > 2 else Path("target/criterion")
    if not crit_dir.is_dir():
        sys.exit(f"错误：找不到 {crit_dir}——先跑 just bench 生成数据。")

    rows = []
    for estimates in sorted(crit_dir.rglob("new/estimates.json")):
        bench_dir = estimates.parent.parent
        dir_name = bench_dir.relative_to(crit_dir).as_posix()
        try:
            point, lower, upper = load_mean(estimates)
            change_path = bench_dir / "change" / "estimates.json"
            if change_path.is_file():
                delta, ci_lo, ci_hi = load_mean(change_path)
                change = f"{human_ratio(delta)} [{human_ratio(ci_lo)}, {human_ratio(ci_hi)}]"
                if ci_lo > 0:
                    state = "回归"
                elif ci_hi < 0:
                    state = "改善"
                else:
                    state = "无显著变化"
            else:
                change = state = "—"
        except (OSError, KeyError, json.JSONDecodeError) as err:
            print(f"警告：跳过 {dir_name}（{err}）", file=sys.stderr)
            continue
        bench = display_name(bench_dir, dir_name)
        rows.append(
            f"| `{bench}` | {human_ns(point)} | [{human_ns(lower)}, {human_ns(upper)}] "
            f"| {change} | {state} |"
        )

    if not rows:
        sys.exit(f"错误：{crit_dir} 下没有可读取的基准数据（new/estimates.json）。")

    print(f"对照：{label}，共 {len(rows)} 个基准")
    print()
    print("| 基准 | 均值 | 95% 区间 | 相对变化 | 判定 |")
    print("| --- | --- | --- | --- | --- |")
    for row in rows:
        print(row)


if __name__ == "__main__":
    main()
