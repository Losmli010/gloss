#!/usr/bin/env python3
"""二进制体积归因：段/节构成 + 符号级 Top N，只记录不判罚。

节级：`size -m` 读 Mach-O 段/节（不依赖符号表，strip 过的 release 产物
可用；数值是虚拟地址空间占用，__PAGEZERO 等非文件映射段会偏大，文件大小
以 stat 为准）。符号级：`otool -l` 取节地址区间、`nm -n` 取符号地址，
同节内相邻符号地址差即符号大小（macOS 的 nm -S 对 Mach-O 恒报 0），
Rust 旧式 `_ZN` 串解码成 `crate::模块::函数` 并按顶层 crate 聚合——需要
未 strip 的二进制（release 产物 strip=true 无符号表，用
`cargo build --profile profiling` 的 target/profiling/gloss，或 dev 产物）。

纯记录工具：数字落 JSON（--json 指定路径）或仅打印；趋势对照按 AGENTS.md
口径只认同机同环境历史，不做成败判定。

用法：
  scripts/perf/size_report.py                            # release 产物节级
  scripts/perf/size_report.py --bin target/profiling/gloss --top 40
  scripts/perf/size_report.py --json target/size/report.json
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

DEFAULT_BIN = "target/release/gloss"

SEGROW_RE = re.compile(r"^(?P<kind>Segment|Section)\s+(?P<name>\S+?):\s+(?P<size>\d+)\s*$")


def run(argv: list[str]) -> str:
    proc = subprocess.run(argv, capture_output=True, text=True)
    if proc.returncode != 0:
        sys.exit(f"{argv[0]} 失败（exit {proc.returncode}）：{proc.stderr.strip()}")
    return proc.stdout


def parse_segments(binary: Path) -> list[dict]:
    """size -m 输出：`Segment __TEXT: 311296` 顶格，节行制表符缩进。"""
    rows: list[dict] = []
    segment = None
    for line in run(["size", "-m", str(binary)]).splitlines():
        match = SEGROW_RE.match(line.strip())
        if not match:
            continue
        name = match.group("name")
        size = int(match.group("size"))
        indented = line.startswith(("\t", "    "))
        row = {
            "level": "section" if indented else "segment",
            "name": name,
            "bytes": size,
        }
        if indented:
            row["segment"] = segment
        else:
            segment = name
        rows.append(row)
    return rows


def decode_rust_symbol(name: str) -> str | None:
    """解码 Rust 符号路径；解不动返回 None（调用方回退原始名）。

    旧式 `_ZN...E` 与 v0 `_RN...` 通用子集：标识符 `<长度><名字>` 连成
    `crate::模块::函数`；v0 的 Cs 去混淆串、`s<数字>_` 实现路径消歧符与
    单字符标记透明跳过，下划线开头的标识符在长度后多一个转义 `_`。泛型
    实例化（I…E）与回引用（B…_）不解——退回原始名。
    """
    legacy = name.find("_ZN")
    if legacy >= 0:
        return decode_components(name[legacy + 3 :], "E", v0=False)
    v0 = name.find("_RN")
    if v0 >= 0:
        return decode_components(name[v0 + 3 :], "E", v0=True)
    return None


def decode_components(body: str, terminator: str, v0: bool) -> str | None:
    parts: list[str] = []
    while body and not body.startswith(terminator):
        if v0 and body.startswith("Cs"):
            skip = body.find("_")
            if skip < 0:
                return None
            body = body[skip + 1 :]
            continue
        if v0 and re.match(r"s\d*_", body):
            body = body[re.match(r"s\d*_", body).end() :]
            continue
        if body[0] in "IBE":
            return None
        if body[0].isalpha() or body[0] == "_":
            body = body[1:]
            continue
        digits = re.match(r"(\d+)", body)
        if not digits:
            return None
        length = int(digits.group(1))
        body = body[digits.end() :]
        if v0 and body.startswith("_"):
            body = body[1:]
        if len(body) < length:
            return None
        part = body[:length]
        body = body[length:]
        if len(part) == 17 and re.fullmatch(r"h[0-9a-f]+", part):
            continue
        parts.append(part)
    if not parts:
        return None
    return "::".join(parts)


V0_CRATE_RE = re.compile(r"Cs[0-9A-Za-z]*?_(\d+)([0-9A-Za-z_]*)")


def crate_of(decoded: str | None, raw: str) -> str:
    """顶层 crate 名：解码路径取首段；解不动时从 v0 去混淆标记里抠。"""
    if decoded:
        return decoded.split("::")[0]
    match = V0_CRATE_RE.search(raw)
    if match:
        length = int(match.group(1))
        name = match.group(2)
        if name.startswith("_"):
            name = name[1:]
        return name[:length] if length else "std"
    return "<未解码>"


def section_ranges(binary: Path) -> list[tuple[str, str, int, int]]:
    """otool -l 抽 (节名, 段名, 起始地址, 大小)，供符号落节与差分算大小。"""
    rows: list[tuple[str, str, int, int]] = []
    lines = run(["otool", "-l", str(binary)]).splitlines()
    for i, line in enumerate(lines):
        stripped = line.strip()
        if not stripped.startswith("sectname "):
            continue
        sect = stripped.split()[1]
        seg = lines[i + 1].strip().split()[1]
        addr = int(lines[i + 2].strip().split()[1], 16)
        size = int(lines[i + 3].strip().split()[1], 16)
        rows.append((sect, seg, addr, size))
    return rows


def parse_symbols(binary: Path, top: int) -> tuple[list[dict], list[dict], int]:
    """nm -S 在 macOS 对 Mach-O 恒报 0，改用同节相邻符号地址差算大小。"""
    secs = section_ranges(binary)
    buckets: dict[tuple[str, str], list[tuple[int, str]]] = {}
    for line in run(["nm", "-n", str(binary)]).splitlines():
        match = re.match(r"^(?P<addr>[0-9a-f]+)\s+(?P<kind>\w)\s+(?P<name>.+?)\s*$", line)
        if not match or match.group("kind") == "U":
            continue
        addr = int(match.group("addr"), 16)
        for sect, seg, start, size in secs:
            if start <= addr < start + size:
                buckets.setdefault((sect, seg), []).append((addr, match.group("name")))
                break
    entries: list[dict] = []
    for sect, seg in buckets:
        syms = sorted(buckets[(sect, seg)])
        end = next(a + z for s, g, a, z in secs if s == sect and g == seg)
        for i, (addr, name) in enumerate(syms):
            sym_end = syms[i + 1][0] if i + 1 < len(syms) else end
            if sym_end <= addr:
                continue
            decoded = decode_rust_symbol(name)
            entries.append({
                "raw": name,
                "decoded": decoded,
                "bytes": sym_end - addr,
                "section": f"{seg},{sect}",
            })
    entries.sort(key=lambda item: item["bytes"], reverse=True)
    by_crate: dict[str, int] = {}
    for item in entries:
        crate = crate_of(item["decoded"], item["raw"])
        by_crate[crate] = by_crate.get(crate, 0) + item["bytes"]
    crates = sorted(by_crate.items(), key=lambda kv: kv[1], reverse=True)[:top]
    total = sum(item["bytes"] for item in entries)
    return entries[:top], [{"crate": name, "bytes": size} for name, size in crates], total


def human(n: float) -> str:
    for unit in ("B", "KB", "MB", "GB"):
        if abs(n) < 1024:
            return f"{n:.1f} {unit}" if unit != "B" else f"{int(n)} B"
        n /= 1024
    return f"{n:.1f} TB"


def main() -> None:
    parser = argparse.ArgumentParser(description="二进制体积归因（记录性）")
    parser.add_argument("--bin", default=DEFAULT_BIN, help="目标 Mach-O 路径")
    parser.add_argument("--top", type=int, default=30, help="符号与 crate 各列前 N")
    parser.add_argument("--json", default="", help="结果落 JSON 的路径（缺省只打印）")
    args = parser.parse_args()

    binary = Path(args.bin)
    if not binary.is_file():
        sys.exit(f"找不到 {binary}：先用 just build-release（节级）或"
                 " cargo build --profile profiling（符号级）构建")
    file_size = binary.stat().st_size
    segments = parse_segments(binary)
    symbols: list[dict] = []
    crates: list[dict] = []
    symbol_total = None
    try:
        symbols, crates, symbol_total = parse_symbols(binary, args.top)
    except SystemExit:
        symbol_total = None
    if symbol_total == 0:
        symbols, crates, symbol_total = [], [], None
        print("（无符号表——strip 过的产物只做节级；符号级用 profiling/dev 产物重跑）")

    print(f"{binary}  文件大小 {human(file_size)}")
    if symbol_total is not None:
        print(f"符号映射合计 {human(symbol_total)}（nm 可见符号的体积和）\n")
    print("段/节构成（≥8KB 或全部段）：")
    for row in segments:
        if row["level"] == "segment" or row["bytes"] >= 8192:
            indent = "  " if row["level"] == "section" else ""
            note = f"  [{row['segment']}]" if row.get("segment") else ""
            print(f"  {indent}{row['name']:<28} {human(row['bytes']):>12}{note}")
    if symbols:
        print(f"\n符号 Top {len(symbols)}：")
        for item in symbols:
            path = item["decoded"] or item["raw"]
            print(f"  {human(item['bytes']):>12}  {path[:100]}")
        print(f"\ncrate 聚合 Top {len(crates)}：")
        for item in crates:
            print(f"  {human(item['bytes']):>12}  {item['crate']}")

    if args.json:
        out = Path(args.json)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps({
            "binary": str(binary),
            "file_size": file_size,
            "segments": segments,
            "symbol_total": symbol_total,
            "top_symbols": symbols,
            "top_crates": crates,
        }, ensure_ascii=False, indent=2))
        print(f"\nJSON 落盘：{out}")


if __name__ == "__main__":
    main()
