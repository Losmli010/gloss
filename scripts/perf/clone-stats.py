#!/usr/bin/env python3
"""统计生产代码里克隆方法调用的分布，并支持对照趋势基线。

生产代码口径：根包入口与各包源码目录，先剥离注释与字符串字面量，再剔除
测试条件标注的项后计数；测试目录、基准目录与测试桩不计。计数对象是实例
方法形态的克隆调用；静态路径形态的克隆调用与派生声明只入库展示，不参与
基线成败判定；静态形态如共享智能指针的克隆。

性能口径：克隆基准的分配计数（次数与字节数）由（criterion）自定义测
量。校验模式读取基准产物目录下最近一次克隆基准的结果，对照趋势历史最
新点的克隆门禁基线（总量、分配次数与字节数，由统一采集器随采集自然前
移）；最新点缺分配值即带指引失败——拒绝以零为基线放行分配回退。计时不
进判罚，归（criterion）自身的基线机制。
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
HISTORY_PATH = Path(__file__).resolve().parent / "baselines" / "history.jsonl"

CRATES = [
    ("gloss", "src/main.rs"),
    ("gloss-core", "crates/gloss-core/src"),
    ("gloss-platform", "crates/gloss-platform/src"),
    ("gloss-app", "crates/gloss-app/src"),
    ("gloss-eval", "crates/gloss-eval/src"),
]

CLONE_RE = re.compile(r"\.clone\s*\(")
PATH_CLONE_RE = re.compile(r"::clone\s*\(")
DERIVE_CLONE_RE = re.compile(r"#\s*\[\s*derive\s*\([^\)]*\bClone\b[^\)]*\)\s*\]")
CFG_TEST_RE = re.compile(r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]")

CRIT_ROOT = ROOT / "target" / "criterion"
PERF_GROUPS = {"clone_allocs": "allocs", "clone_bytes": "bytes"}


def read_perf() -> dict[str, dict[str, int]]:
    """读 criterion 最近一次 clone 基准结果：基准 id → {allocs, bytes}（均值四舍五入）。"""
    perf = {}
    if not CRIT_ROOT.is_dir():
        return perf
    for group, field in PERF_GROUPS.items():
        for estimates in sorted(CRIT_ROOT.glob(f"{group}/*/new/estimates.json")):
            bench_dir = estimates.parent.parent
            try:
                mean = json.loads(estimates.read_text(encoding="utf-8"))["mean"]
                value = round(mean["point_estimate"])
            except (OSError, KeyError, json.JSONDecodeError):
                continue
            try:
                full_id = json.loads(
                    (bench_dir / "new" / "benchmark.json").read_text(encoding="utf-8")
                )["full_id"]
            except (OSError, KeyError, json.JSONDecodeError):
                full_id = bench_dir.relative_to(CRIT_ROOT / group).as_posix()
            perf.setdefault(full_id, {})[field] = value
    return perf


def _consume_str(src: str, i: int) -> int:
    """从开引号处消费一个转义字符串，返回结束下标（不含）。"""
    n = len(src)
    j = i + 1
    while j < n:
        if src[j] == "\\":
            j += 2
        elif src[j] == '"':
            return j + 1
        else:
            j += 1
    return n


def _consume_raw(src: str, i: int) -> int | None:
    """i 处为 r 时尝试消费 r#"..."# 原始字符串；不是原始字符串返回 None。"""
    n = len(src)
    j = i + 1
    hashes = 0
    while j < n and src[j] == "#":
        hashes += 1
        j += 1
    if j >= n or src[j] != '"':
        return None
    terminator = '"' + "#" * hashes
    k = src.find(terminator, j + 1)
    return n if k == -1 else k + len(terminator)


def tokenize_segments(src: str) -> list[tuple[str, int, int]]:
    """把源码切分为 (kind, start, end)，kind ∈ code / comment / string。"""
    segs = []
    i, n = 0, len(src)
    code_start = 0

    def flush(end: int) -> None:
        if end > code_start:
            segs.append(("code", code_start, end))

    while i < n:
        c = src[i]
        if c == "/" and src[i + 1 : i + 2] == "/":
            flush(i)
            j = src.find("\n", i)
            j = n if j == -1 else j
            segs.append(("comment", i, j))
            i = code_start = j
        elif c == "/" and src[i + 1 : i + 2] == "*":
            flush(i)
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth += 1
                    j += 2
                elif src.startswith("*/", j):
                    depth -= 1
                    j += 2
                else:
                    j += 1
            segs.append(("comment", i, j))
            i = code_start = j
        elif c == '"':
            flush(i)
            end = _consume_str(src, i)
            segs.append(("string", i, end))
            i = code_start = end
        elif c in "bc" and src[i + 1 : i + 2] == '"':
            flush(i)
            end = _consume_str(src, i + 1)
            segs.append(("string", i, end))
            i = code_start = end
        elif c == "r" and src[i + 1 : i + 2] == '"':
            flush(i)
            end = _consume_raw(src, i)
            segs.append(("string", i, end))
            i = code_start = end
        elif c in "br" and src[i + 1 : i + 2] == "#":
            flush(i)
            end = _consume_raw(src, i)
            if end is not None:
                segs.append(("string", i, end))
                i = code_start = end
            else:
                i += 1
        elif c == "'":
            m = re.match(r"'(\\(?:.|x[0-9a-fA-F]{2}|u\{[^}]*\})|[^\\'])'", src[i:])
            if m:
                flush(i)
                segs.append(("string", i, i + m.end()))
                i = code_start = i + m.end()
            else:
                i += 1
        else:
            i += 1
    flush(n)
    return segs


def blank_non_code(src: str) -> str:
    """返回注释与字符串置空后的源码，偏移保持不变。"""
    view = list(src)
    for kind, s, e in tokenize_segments(src):
        if kind != "code":
            for i in range(s, e):
                if view[i] != "\n":
                    view[i] = " "
    return "".join(view)


def _match_brace(src: str, open_idx: int) -> int:
    depth = 0
    for i in range(open_idx, len(src)):
        if src[i] == "{":
            depth += 1
        elif src[i] == "}":
            depth -= 1
            if depth == 0:
                return i
    return len(src) - 1


def _skip_group(src: str, open_idx: int, open_ch: str, close_ch: str) -> int:
    depth = 0
    for i in range(open_idx, len(src)):
        if src[i] == open_ch:
            depth += 1
        elif src[i] == close_ch:
            depth -= 1
            if depth == 0:
                return i + 1
    return len(src)


def strip_cfg_test(codeview: str) -> str:
    """把 #[cfg(test)] 标注的项（含项体）整体置空，换行保留。"""
    src = codeview
    regions = []
    for m in CFG_TEST_RE.finditer(src):
        i, n = m.end(), len(src)
        while True:
            while i < n and src[i].isspace():
                i += 1
            if i < n and src[i] == "#":
                i = _skip_group(src, i, "[", "]")
            else:
                break
        j, body_start, paren = i, None, 0
        while j < n:
            ch = src[j]
            if ch == "{":
                body_start = j
                break
            elif ch == "(":
                paren += 1
            elif ch == ")":
                paren -= 1
            elif ch == ";" and paren == 0:
                break
            elif ch == "}" and paren == 0:
                break
            j += 1
        if body_start is not None:
            regions.append((m.start(), _match_brace(src, body_start) + 1))
    regions.sort()
    out = list(src)
    last_end = 0
    for s, e in regions:
        if s < last_end:
            continue
        for i in range(s, e):
            if out[i] != "\n":
                out[i] = " "
        last_end = e
    return "".join(out)


def analyze_file(path: Path) -> dict[str, int]:
    """返回单文件计数：clone / path_clone / derive_clone / loc。"""
    src = path.read_text(encoding="utf-8")
    stripped = strip_cfg_test(blank_non_code(src))
    return {
        "clone": len(CLONE_RE.findall(stripped)),
        "path_clone": len(PATH_CLONE_RE.findall(stripped)),
        "derive_clone": len(DERIVE_CLONE_RE.findall(stripped)),
        "loc": sum(1 for ln in stripped.splitlines() if ln.strip()),
    }


def collect() -> tuple[dict[str, dict[str, int]], dict[str, int]]:
    """返回（各 crate 聚合计数，文件 → clone 数）。"""
    crates, files = {}, {}
    for crate, rel in CRATES:
        root = ROOT / rel
        if not root.exists():
            sys.exit(f"error: path does not exist {root}")
        paths = [root] if root.is_file() else sorted(root.rglob("*.rs"))
        agg = {"files": 0, "loc": 0, "clone": 0, "path_clone": 0, "derive_clone": 0}
        for p in paths:
            m = analyze_file(p)
            agg["files"] += 1
            for k in ("loc", "clone", "path_clone", "derive_clone"):
                agg[k] += m[k]
            if m["clone"]:
                files[str(p.relative_to(ROOT))] = m["clone"]
        crates[crate] = agg
    return crates, files


def density(clone: int, loc: int) -> float:
    return clone / loc * 1000 if loc else 0.0


def print_stats(crates: dict[str, dict[str, int]], files: dict[str, int]) -> None:
    total = sum(c["clone"] for c in crates.values())
    total_loc = sum(c["loc"] for c in crates.values())
    print("| crate | prod LOC | .clone() | density(/kloc) | share | ::clone | derive(Clone) |")
    print("|---|---:|---:|---:|---:|---:|---:|")
    for crate, agg in crates.items():
        share = f"{agg['clone'] / total * 100:.1f}%" if total else "-"
        print(
            f"| {crate} | {agg['loc']} | {agg['clone']} | {density(agg['clone'], agg['loc']):.1f} "
            f"| {share} | {agg['path_clone']} | {agg['derive_clone']} |"
        )
    print(f"| total | {total_loc} | {total} | {density(total, total_loc):.1f} | 100% |  |  |")

    ranked = sorted(files.items(), key=lambda kv: (-kv[1], kv[0]))
    if ranked:
        print("\nhot files:")
        for path, count in ranked[:15]:
            print(f"  {count:>4}  {path}")
        rest = ranked[15:]
        if rest:
            print(f"  (remaining {len(rest)} files, {sum(c for _, c in rest)} total)")

    perf = read_perf()
    if perf:
        print("\nalloc hotspots (latest criterion clone bench, mean/iter):")
        for bench_id in sorted(perf):
            entry = perf[bench_id]
            parts = " / ".join(f"{entry[f]} {f}" for f in ("allocs", "bytes") if f in entry)
            print(f"  {bench_id}: {parts}")


def latest_clone_baseline() -> tuple[dict[str, Any], dict[str, Any]]:
    """history 最新点 → clone 门禁基线（total/allocs/bytes 整数 + density 展示）。

    基线随采集自然前移（perf-collect.py 写入最新点）；缺行或缺分配值即带指
    引失败——分配为 0 的假基线会把真回退放行，防护与旧 clone-stats.json
    perf 段缺失时一致，不依赖上游采集侧。
    """
    if not HISTORY_PATH.is_file():
        sys.exit(
            f"error: trend history missing {HISTORY_PATH}, run the collector first (just perf-baseline) to establish a baseline"
        )
    points = []
    for line in HISTORY_PATH.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            points.append(json.loads(line))
        except json.JSONDecodeError as exc:
            sys.exit(f"error: {HISTORY_PATH} has an unparseable line: {exc}")
    if not points:
        sys.exit(
            f"error: {HISTORY_PATH} has no data points, run the collector to establish a baseline"
        )
    latest = points[-1]
    metrics = latest.get("metrics")
    if not isinstance(metrics, dict) or "clone.total" not in metrics:
        sys.exit(
            f"error: history latest point lacks clone.total (last line of {HISTORY_PATH}), "
            "run the collector first (just perf-baseline) to update the baseline"
        )
    if "clone.allocs" not in metrics or "clone.bytes" not in metrics:
        sys.exit(
            "error: history latest point lacks clone alloc values (clone.allocs/clone.bytes), "
            "refusing to pass alloc regressions against a 0 baseline; run just clone-bench first, "
            "then the collector (just perf-baseline) to update the baseline."
        )
    return latest, metrics


def aggregate_perf(perf: dict[str, dict[str, int]], field: str) -> int | None:
    """当前 criterion 数据单字段合计（次数/字节）；无任何基准携带该字段返回 None。"""
    values = [entry[field] for entry in perf.values() if field in entry]
    return sum(values) if values else None


def check_baseline() -> None:
    latest, baseline = latest_clone_baseline()
    crates, _files = collect()
    cur_total = sum(crate["clone"] for crate in crates.values())
    perf = read_perf()

    print(
        f"== clone gate check: history latest point {latest.get('commit', '?')}"
        f" ({str(latest.get('date', ''))[:10]}) ==\n"
    )
    print("| metric | baseline | current | change |")
    print("|---|---:|---:|---|")
    rows = [("clone.total", baseline["clone.total"], cur_total)]
    for field in ("allocs", "bytes"):
        current = aggregate_perf(perf, field)
        base = baseline.get(f"clone.{field}")
        rows.append((f"clone.{field}", base, current))
    regressions = []
    for metric_id, base, current in rows:
        if current is None:
            print(f"| {metric_id} | {base} | — | no current data |")
            continue
        delta = current - base
        mark = {0: "flat", 1: "↑", -1: "↓"}.get((delta > 0) - (delta < 0), "")
        print(f"| {metric_id} | {base} | {current} | {delta:+d} {mark} |")
        if delta > 0:
            regressions.append(f"{metric_id} {delta:+d}")
    cur_loc = sum(crate["loc"] for crate in crates.values())
    base_density = baseline.get("clone.density")
    base_density_text = f"{base_density:.1f}" if isinstance(base_density, (int, float)) else "—"
    print(
        f"\ndensity (record only): {base_density_text} -> {density(cur_total, cur_loc):.1f} (/kloc)"
    )

    if perf:
        print("\nalloc hotspots (latest criterion clone bench, mean/iter):")
        for bench_id in sorted(perf):
            entry = perf[bench_id]
            parts = " / ".join(f"{entry[f]} {f}" for f in ("allocs", "bytes") if f in entry)
            print(f"  {bench_id}: {parts}")

    if regressions:
        print("\nfailed: regression against the baseline.")
        print(f"  clone increases: {', '.join(regressions)}.")
        print(
            "  if intentional, run just clone-bench then the collector (just perf-baseline)"
            " to update the baseline, and review it together with this change."
        )
        sys.exit(1)
    print(
        f"\nok: baseline flat or improved (clone count {baseline['clone.total']} -> {cur_total})."
    )


def main() -> None:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        "--check", action="store_true", help="report changes against the latest history point"
    )
    args = parser.parse_args()
    if args.check:
        check_baseline()
    else:
        crates, files = collect()
        print_stats(crates, files)


if __name__ == "__main__":
    main()
