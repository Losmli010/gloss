#!/usr/bin/env python3
"""统计生产代码中 .clone() 调用的分布，并支持对照趋势基线。

生产代码口径：根包 src/main.rs 与各包 src/，先剥离注释与字符串字面量，
再剔除 #[cfg(test)] 标注的项后计数；tests/、benches/ 与测试桩不计。
计数对象是 `.clone(` 方法调用；`X::clone(`（如 Arc::clone）与 derive(Clone)
只入库展示，不参与基线成败判定。

性能口径：benches/clone.rs 的分配计数（次数/字节，criterion 自定义测量），
--check 读取 target/criterion 下最近一次 `just clone-bench` 的结果，对照
baselines/history.jsonl 最新点的 clone 门禁基线（total/allocs/bytes，由
perf-collect.py 随采集自然前移）；最新点缺分配值即带指引失败——拒绝以 0
为基线放行分配回退。计时不进判罚，归 criterion 自身的基线机制（just
bench-check / bench-summary）。

用法：
  scripts/perf/clone-stats.py            打印当前分布
  scripts/perf/clone-stats.py --check    对照 history 最新点报告变化；clone
                                         计数或热点分配上升即退出码 1
"""

import argparse
import json
import re
import sys
from pathlib import Path

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


def read_perf():
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
                full_id = json.loads((bench_dir / "new" / "benchmark.json").read_text(encoding="utf-8"))["full_id"]
            except (OSError, KeyError, json.JSONDecodeError):
                full_id = bench_dir.relative_to(CRIT_ROOT / group).as_posix()
            perf.setdefault(full_id, {})[field] = value
    return perf


def _consume_str(src, i):
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


def _consume_raw(src, i):
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


def tokenize_segments(src):
    """把源码切分为 (kind, start, end)，kind ∈ code / comment / string。"""
    segs = []
    i, n = 0, len(src)
    code_start = 0

    def flush(end):
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


def blank_non_code(src):
    """返回注释与字符串置空后的源码，偏移保持不变。"""
    view = list(src)
    for kind, s, e in tokenize_segments(src):
        if kind != "code":
            for i in range(s, e):
                if view[i] != "\n":
                    view[i] = " "
    return "".join(view)


def _match_brace(src, open_idx):
    depth = 0
    for i in range(open_idx, len(src)):
        if src[i] == "{":
            depth += 1
        elif src[i] == "}":
            depth -= 1
            if depth == 0:
                return i
    return len(src) - 1


def _skip_group(src, open_idx, open_ch, close_ch):
    depth = 0
    for i in range(open_idx, len(src)):
        if src[i] == open_ch:
            depth += 1
        elif src[i] == close_ch:
            depth -= 1
            if depth == 0:
                return i + 1
    return len(src)


def strip_cfg_test(codeview):
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


def analyze_file(path):
    """返回单文件计数：clone / path_clone / derive_clone / loc。"""
    src = path.read_text(encoding="utf-8")
    stripped = strip_cfg_test(blank_non_code(src))
    return {
        "clone": len(CLONE_RE.findall(stripped)),
        "path_clone": len(PATH_CLONE_RE.findall(stripped)),
        "derive_clone": len(DERIVE_CLONE_RE.findall(stripped)),
        "loc": sum(1 for ln in stripped.splitlines() if ln.strip()),
    }


def collect():
    """返回（各 crate 聚合计数，文件 → clone 数）。"""
    crates, files = {}, {}
    for crate, rel in CRATES:
        root = ROOT / rel
        if not root.exists():
            sys.exit(f"错误：路径不存在 {root}")
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


def density(clone, loc):
    return clone / loc * 1000 if loc else 0.0


def print_stats(crates, files):
    total = sum(c["clone"] for c in crates.values())
    total_loc = sum(c["loc"] for c in crates.values())
    print("| crate | 生产 LOC | .clone() | 密度(/千行) | 占比 | ::clone | derive(Clone) |")
    print("|---|---:|---:|---:|---:|---:|---:|")
    for crate, agg in crates.items():
        share = f"{agg['clone'] / total * 100:.1f}%" if total else "-"
        print(
            f"| {crate} | {agg['loc']} | {agg['clone']} | {density(agg['clone'], agg['loc']):.1f} "
            f"| {share} | {agg['path_clone']} | {agg['derive_clone']} |"
        )
    print(f"| 合计 | {total_loc} | {total} | {density(total, total_loc):.1f} | 100% |  |  |")

    ranked = sorted(files.items(), key=lambda kv: (-kv[1], kv[0]))
    if ranked:
        print("\n热点文件：")
        for path, count in ranked[:15]:
            print(f"  {count:>4}  {path}")
        rest = ranked[15:]
        if rest:
            print(f"  （其余 {len(rest)} 个文件共 {sum(c for _, c in rest)} 次）")

    perf = read_perf()
    if perf:
        print("\n分配热点（criterion 最近一次 clone 基准，均值/迭代）：")
        for bench_id in sorted(perf):
            entry = perf[bench_id]
            parts = " / ".join(f"{entry[f]} {f}" for f in ("allocs", "bytes") if f in entry)
            print(f"  {bench_id}: {parts}")


def latest_clone_baseline():
    """history 最新点 → clone 门禁基线（total/allocs/bytes 整数 + density 展示）。

    基线随采集自然前移（perf-collect.py 写入最新点）；缺行或缺分配值即带指
    引失败——分配为 0 的假基线会把真回退放行，防护与旧 clone-stats.json
    perf 段缺失时一致，不依赖上游采集侧。
    """
    if not HISTORY_PATH.is_file():
        sys.exit(f"错误：趋势历史不存在 {HISTORY_PATH}，先运行采集（just perf-baseline）建立基线")
    points = []
    for line in HISTORY_PATH.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            points.append(json.loads(line))
        except json.JSONDecodeError as exc:
            sys.exit(f"错误：{HISTORY_PATH} 存在无法解析的行：{exc}")
    if not points:
        sys.exit(f"错误：{HISTORY_PATH} 没有数据点，先运行采集建立基线")
    latest = points[-1]
    metrics = latest.get("metrics")
    if not isinstance(metrics, dict) or "clone.total" not in metrics:
        sys.exit(
            f"错误：history 最新点缺 clone.total（{HISTORY_PATH} 最后一行），"
            "先运行采集（just perf-baseline）更新基线"
        )
    if "clone.allocs" not in metrics or "clone.bytes" not in metrics:
        sys.exit(
            "错误：history 最新点缺 clone 分配值（clone.allocs/clone.bytes），"
            "拒绝以 0 为基线放行分配回退；先运行 just clone-bench，"
            "再运行采集（just perf-baseline）更新基线。"
        )
    return latest, metrics


def aggregate_perf(perf, field):
    """当前 criterion 数据单字段合计（次数/字节）；无任何基准携带该字段返回 None。"""
    values = [entry[field] for entry in perf.values() if field in entry]
    return sum(values) if values else None


def check_baseline():
    latest, baseline = latest_clone_baseline()
    crates, _files = collect()
    cur_total = sum(crate["clone"] for crate in crates.values())
    perf = read_perf()

    print(
        f"== clone 门禁对照：history 最新点 {latest.get('commit', '?')}"
        f"（{str(latest.get('date', ''))[:10]}）==\n"
    )
    print("| 指标 | 基线 | 当前 | 变化 |")
    print("|---|---:|---:|---|")
    rows = [("clone.total", baseline["clone.total"], cur_total)]
    for field in ("allocs", "bytes"):
        current = aggregate_perf(perf, field)
        base = baseline.get(f"clone.{field}")
        rows.append((f"clone.{field}", base, current))
    regressions = []
    for metric_id, base, current in rows:
        if current is None:
            print(f"| {metric_id} | {base} | — | 当前无数据 |")
            continue
        delta = current - base
        mark = {0: "持平", 1: "↑", -1: "↓"}.get((delta > 0) - (delta < 0), "")
        print(f"| {metric_id} | {base} | {current} | {delta:+d} {mark} |")
        if delta > 0:
            regressions.append(f"{metric_id} {delta:+d}")
    cur_loc = sum(crate["loc"] for crate in crates.values())
    base_density = baseline.get("clone.density")
    base_density_text = f"{base_density:.1f}" if isinstance(base_density, (int, float)) else "—"
    print(
        f"\n密度（仅记录）：{base_density_text} → "
        f"{density(cur_total, cur_loc):.1f}（次/千行）"
    )

    if perf:
        print("\n分配热点（criterion 最近一次 clone 基准，均值/迭代）：")
        for bench_id in sorted(perf):
            entry = perf[bench_id]
            parts = " / ".join(f"{entry[f]} {f}" for f in ("allocs", "bytes") if f in entry)
            print(f"  {bench_id}: {parts}")

    if regressions:
        print("\n✗ 对照基线回退。")
        print(f"  clone 上升：{'、'.join(regressions)}。")
        print("  如属有意，运行 just clone-bench 后采集（just perf-baseline）更新基线，并随本次改动一并审查。")
        sys.exit(1)
    print(f"\n✓ 对照基线持平或改善（clone 计数 {baseline['clone.total']} → {cur_total}）。")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--check", action="store_true", help="对照 history 最新点报告变化")
    args = parser.parse_args()
    if args.check:
        check_baseline()
    else:
        crates, files = collect()
        print_stats(crates, files)


if __name__ == "__main__":
    main()
