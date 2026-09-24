#!/usr/bin/env python3
"""把 JSONL 日志渲染成人读形态：`时间 级别 目标 消息 (字段…)`。

日志落盘是 `~/.gloss/logs/gloss-<UTC 日期>.jsonl`（一行一个 JSON 对象），
本脚本是它的读侧：默认跟随最新一份文件（等价 tail -f），可只挑一条任务
链路（`--generation`）或只看某一档以上（`--level`）。

用法：
  scripts/dev/gloss-logs.py                  # 跟随最新日志
  scripts/dev/gloss-logs.py --generation 2   # 只看代数 2 的链路
  scripts/dev/gloss-logs.py --level warn     # 只看 warn 及以上
  scripts/dev/gloss-logs.py --raw            # 原样输出 JSONL（给 jq/别的工具）
  scripts/dev/gloss-logs.py --no-follow      # 打完现有内容就退出
"""

from __future__ import annotations

import argparse
import datetime
import json
import signal
import sys
import time
from pathlib import Path

DEFAULT_DIR = Path.home() / ".gloss" / "logs"
FILE_PREFIX = "gloss-"
FILE_SUFFIX = ".jsonl"
POLL_SECONDS = 0.3

LEVELS = {"TRACE": 0, "DEBUG": 1, "INFO": 2, "WARN": 3, "ERROR": 4}
LEVEL_COLORS = {
    "TRACE": "\033[90m",
    "DEBUG": "\033[90m",
    "INFO": "\033[32m",
    "WARN": "\033[33m",
    "ERROR": "\033[31m",
}
RESET = "\033[0m"
DIM = "\033[2m"


def newest_log(directory: Path) -> Path | None:
    """目录里最新的日志文件（名字里的 UTC 日期可字典序比较）。"""
    candidates = sorted(directory.glob(f"{FILE_PREFIX}*{FILE_SUFFIX}"))
    return candidates[-1] if candidates else None


def local_clock(timestamp: str) -> str:
    """RFC3339（UTC）时间戳 → 本地 `HH:MM:SS`；解析不了就原样截取。"""
    try:
        parsed = datetime.datetime.fromisoformat(timestamp.replace("Z", "+00:00"))
        return parsed.astimezone().strftime("%H:%M:%S")
    except ValueError:
        return timestamp[11:19] if len(timestamp) >= 19 else timestamp


def format_line(line: str, color: bool) -> str | None:
    """一条 JSONL → 人读行；解析失败返回 None（由调用方决定怎么处理）。"""
    record = json.loads(line)
    if not isinstance(record, dict):
        return None

    clock = local_clock(str(record.pop("timestamp", "")))
    level = str(record.pop("level", ""))
    target = str(record.pop("target", ""))
    message = str(record.pop("message", ""))
    # 短模块名（`gloss_app::app::channels` → `app::channels`）省横向空间。
    short_target = target.split("::", 1)[1] if "::" in target else target

    fields = " ".join(f"{key}={value}" for key, value in record.items())
    head = f"{clock} {level:<5} {short_target} {message}"
    if color:
        paint = LEVEL_COLORS.get(level, "")
        head = f"{DIM}{clock}{RESET} {paint}{level:<5}{RESET} {DIM}{short_target}{RESET} {message}"
        return f"{head} {DIM}{fields}{RESET}" if fields else head
    return f"{head} {fields}" if fields else head


def matches(record_line: str, generation: int | None, level: str | None) -> bool:
    """按代数与最低级别过滤一条原始行。"""
    if generation is None and level is None:
        return True
    try:
        record = json.loads(record_line)
    except json.JSONDecodeError:
        return True
    if not isinstance(record, dict):
        return True
    if generation is not None and record.get("generation") != generation:
        return False
    if level is not None:
        current = LEVELS.get(str(record.get("level", "")).upper(), -1)
        if current < LEVELS[level]:
            return False
    return True


def render(line: str, args: argparse.Namespace, color: bool) -> None:
    """输出一行：过滤、渲染、按需原样透传。"""
    if not line.strip():
        return
    if not matches(line, args.generation, args.level):
        return
    if args.raw:
        print(line, flush=True)
        return
    try:
        rendered = format_line(line, color)
    except json.JSONDecodeError:
        rendered = f"{DIM}{line}{RESET}" if color else line
    print(rendered if rendered is not None else line, flush=True)


def follow(path: Path, args: argparse.Namespace, color: bool) -> None:
    """打完现有内容；跟随模式下继续读新行，跨天换了文件就跟着切。"""
    handle = path.open(encoding="utf-8", errors="replace")
    current = path
    while True:
        line = handle.readline()
        if line:
            render(line, args, color)
            continue
        if not args.follow:
            return
        time.sleep(POLL_SECONDS)
        newer = newest_log(path.parent)
        if newer is not None and newer != current:
            handle.close()
            current = newer
            handle = current.open(encoding="utf-8", errors="replace")
        else:
            handle.seek(handle.tell())


def main() -> int:
    parser = argparse.ArgumentParser(description="渲染 gloss 的 JSONL 日志")
    parser.add_argument("--dir", type=Path, default=DEFAULT_DIR, help="日志目录")
    parser.add_argument("--file", type=Path, help="指定日志文件（默认取最新一份）")
    parser.add_argument("--generation", type=int, help="只看这条代数的任务链路")
    parser.add_argument("--level", choices=[level.lower() for level in LEVELS], help="最低级别")
    parser.add_argument("--raw", action="store_true", help="原样输出 JSONL")
    parser.add_argument("--follow", dest="follow", action="store_true", default=True, help="跟随新行（默认）")
    parser.add_argument("--no-follow", dest="follow", action="store_false", help="打完现有内容就退出")
    parser.add_argument("--no-color", action="store_true", help="不输出颜色")
    args = parser.parse_args()
    if args.level is not None:
        args.level = args.level.upper()

    path = args.file if args.file else newest_log(args.dir)
    if path is None or not path.exists():
        print(f"no log file under {args.dir} yet (run the app first)", file=sys.stderr)
        return 1
    color = sys.stdout.isatty() and not args.no_color and not args.raw
    if color:
        print(f"{DIM}tailing {path}{RESET}", file=sys.stderr)
    else:
        print(f"tailing {path}", file=sys.stderr)
    follow(path, args, color)
    return 0


if __name__ == "__main__":
    # 管道下游（head/less）提前关闭时安静退出，不要在刷屏里抛栈。
    signal.signal(signal.SIGPIPE, signal.SIG_DFL)
    sys.exit(main())
