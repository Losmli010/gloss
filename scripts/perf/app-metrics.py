#!/usr/bin/env python3
"""量化应用运行时资源：启动里程碑分段、RSS/CPU 采样、磁盘占用清单。

每次运行：GLOSS_LOG_DIR 指向独立临时目录 spawn release 二进制，轮询日志
解析里程碑行（milestone / elapsed_ms），并行按固定间隔采样 ps 的 RSS 与
累计 CPU 时间；m5_ready（窗口+GPU+预热帧就绪）后再采一段空闲窗口，
SIGTERM 收尾。共跑 --runs 次，各指标取中位数，连同磁盘清单与环境快照
写入 baselines/app-runtime-baseline.json。

启动总时长取 spawn→m5_ready 的观测墙钟（含进程加载，精度受轮询粒度限
制），进程内分段归因看 milestones_ms。空闲 CPU 是空闲窗口的 CPU 时间增
量，长驻进程预算口径 ≈0——winit 循环阻塞、tap 线程睡 CFRunLoop、tokio
空闲，任何子系统空转都在这里显形。墙钟与 RSS 跨机器不可比：本基线只记
录与展示，不做成败判定（趋势只认同机同环境历史）。

前置：target/release/gloss 已构建（just app-metrics 配方先构建）。已有
gloss 实例在跑时建议先退出——测量第二实例短暂共存，采样按 pid 隔离不
受影响，但划词手势会弹两张卡。

用法：
  scripts/perf/app-metrics.py                        # 3 次，空闲窗口 30s
  scripts/perf/app-metrics.py --runs 5 --idle-secs 60
"""

import argparse
import json
import os
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
ROOT = SCRIPT_DIR.parents[1]
BINARY = ROOT / "target" / "release" / "gloss"
BASELINE_PATH = SCRIPT_DIR / "baselines" / "app-runtime-baseline.json"

READY_MILESTONE = "m5_ready"


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


def parse_cpu_seconds(text):
    """ps 的 time 列（MM:SS.ss / HH:MM:SS.ss）→ 秒。"""
    parts = text.strip().split(":")
    try:
        seconds = float(parts[-1])
        minutes = float(parts[-2]) if len(parts) > 1 else 0.0
        hours = float(parts[-3]) if len(parts) > 2 else 0.0
    except ValueError:
        return None
    return hours * 3600 + minutes * 60 + seconds


def sample_process(pid):
    """ps 采样一个进程：RSS(kb) 与累计 CPU 秒；进程已退出返回 None。"""
    try:
        out = subprocess.run(
            ["ps", "-o", "rss=,time=", "-p", str(pid)],
            capture_output=True, text=True, check=True, timeout=10,
        ).stdout
    except (OSError, subprocess.SubprocessError):
        return None
    fields = out.split()
    if len(fields) < 2:
        return None
    try:
        rss_kb = int(fields[0])
    except ValueError:
        return None
    cpu = parse_cpu_seconds(" ".join(fields[1:]))
    if cpu is None:
        return None
    return rss_kb, cpu


def find_log_file(log_dir):
    files = sorted(log_dir.glob("gloss-*.jsonl"))
    return files[-1] if files else None


def parse_milestones(log_dir):
    """解析日志里的里程碑行：id → elapsed_ms（进程内单调时钟，相对入口累计）。"""
    log_file = find_log_file(log_dir)
    if log_file is None:
        return {}
    milestones = {}
    for line in log_file.read_text(encoding="utf-8").splitlines():
        if '"milestone"' not in line:
            continue
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(event.get("milestone"), str) and isinstance(event.get("elapsed_ms"), int):
            milestones[event["milestone"]] = event["elapsed_ms"]
    return milestones


def one_run(idle_secs, ready_timeout):
    """spawn 一次并采到空闲窗口结束；失败返回 None（应用侧异常不外泄）。"""
    log_dir = Path(tempfile.mkdtemp(prefix="gloss-metrics-"))
    env = dict(os.environ, GLOSS_LOG_DIR=str(log_dir))
    spawn_wall = time.monotonic()
    proc = subprocess.Popen(
        [str(BINARY)],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    rss_samples = []
    cpu_samples = []
    milestones = {}
    ready_wall_ms = None
    ready_cpu = None
    idle_wall_ms = None
    try:
        deadline = spawn_wall + ready_timeout
        idle_start = None
        while True:
            now = time.monotonic()
            sample = sample_process(proc.pid)
            if sample is not None:
                rss_samples.append(sample[0])
                cpu_samples.append((now, sample[1]))
            milestones.update(parse_milestones(log_dir))
            if ready_wall_ms is None and READY_MILESTONE in milestones:
                ready_wall_ms = (now - spawn_wall) * 1000
                ready_cpu = cpu_samples[-1][1] if cpu_samples else None
                idle_start = now
            if ready_wall_ms is not None and now - idle_start >= idle_secs:
                idle_wall_ms = (now - idle_start) * 1000
                break
            if ready_wall_ms is None and now > deadline:
                print(
                    f"警告：{READY_MILESTONE} 在 {ready_timeout}s 内未出现，本次运行作废。",
                    file=sys.stderr,
                )
                return None
            if proc.poll() is not None:
                print("警告：应用提前退出，本次运行作废。", file=sys.stderr)
                return None
            time.sleep(0.05 if ready_wall_ms is None else 1.0)
        if ready_cpu is None or len(cpu_samples) < 2:
            print("警告：CPU 采样不足，本次运行作废。", file=sys.stderr)
            return None
        return {
            "wall_ms": round(ready_wall_ms),
            "cpu_ms": round((ready_cpu - cpu_samples[0][1]) * 1000),
            "milestones": milestones,
            "rss_peak_kb": max(rss_samples),
            "rss_idle_kb": round(statistics.median(rss_samples[-5:])),
            "idle_window_ms": round(idle_wall_ms),
            "idle_cpu_ms": round((cpu_samples[-1][1] - ready_cpu) * 1000),
        }
    finally:
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=10)
        else:
            proc.wait()
        if ready_wall_ms is not None:
            shutil.rmtree(log_dir, ignore_errors=True)


def aggregate(runs):
    def med(key):
        values = [r[key] for r in runs]
        return round(statistics.median(values))

    milestone_ids = sorted({k for r in runs for k in r["milestones"]})
    milestones_ms = {}
    for key in milestone_ids:
        values = [r["milestones"][key] for r in runs if key in r["milestones"]]
        milestones_ms[key] = round(statistics.median(values))
    return {
        "startup": {
            "wall_ms": med("wall_ms"),
            "cpu_ms": med("cpu_ms"),
            "milestones_ms": milestones_ms,
        },
        "memory": {
            "rss_peak_kb": med("rss_peak_kb"),
            "rss_idle_kb": med("rss_idle_kb"),
        },
        "cpu": {
            "idle_window_ms": med("idle_window_ms"),
            "idle_cpu_ms": med("idle_cpu_ms"),
        },
    }


def config_path():
    """配置文件路径，与 storage 的 dirs 惯例一致（macOS 标准目录 / XDG）。"""
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / "gloss" / "config.toml"
    xdg = os.environ.get("XDG_CONFIG_HOME")
    base = Path(xdg) if xdg else Path.home() / ".config"
    return base / "gloss" / "config.toml"


def dir_usage(path):
    """目录总字节与文件数；目录不存在返回 (0, 0)。"""
    total, count = 0, 0
    if path.is_dir():
        for item in path.rglob("*"):
            if item.is_file():
                total += item.stat().st_size
                count += 1
    return total, count


def main():
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--runs", type=int, default=3, help="运行次数，各指标取中位数")
    parser.add_argument("--idle-secs", type=int, default=30, help="就绪后的空闲采样窗口秒数")
    parser.add_argument("--ready-timeout", type=int, default=60, help="等待 m5_ready 的超时秒数")
    args = parser.parse_args()

    if not BINARY.is_file():
        sys.exit(
            f"错误：找不到 {BINARY.relative_to(ROOT)}，先构建 release 产物"
            "（just app-metrics 配方会先跑 cargo build --release）。"
        )
    if subprocess.run(["pgrep", "-x", "gloss"], capture_output=True).returncode == 0:
        print(
            "提示：已有 gloss 实例在运行；测量实例会短暂共存（采样按 pid 隔离），建议先退出再测。",
            file=sys.stderr,
        )

    runs = []
    for i in range(args.runs):
        print(f"运行 {i + 1}/{args.runs}（就绪后空闲采样 {args.idle_secs}s）…")
        result = one_run(args.idle_secs, args.ready_timeout)
        if result is not None:
            runs.append(result)
    if not runs:
        sys.exit("错误：没有一次运行成功，无法写基线（见上方各次警告定位原因）。")

    config = config_path()
    logs_bytes, logs_files = dir_usage(Path.home() / ".gloss" / "logs")
    payload = {
        "env": collect_env(),
        "runs": {"requested": args.runs, "succeeded": len(runs)},
        **aggregate(runs),
        "disk": {
            "binary_bytes": BINARY.stat().st_size,
            "config_bytes": config.stat().st_size if config.is_file() else 0,
            "logs_bytes": logs_bytes,
            "logs_files": logs_files,
        },
    }
    BASELINE_PATH.parent.mkdir(parents=True, exist_ok=True)
    BASELINE_PATH.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    rel = BASELINE_PATH.relative_to(ROOT)
    print(
        f"应用运行时基线已写入：{rel}（{len(runs)}/{args.runs} 次成功，"
        f"启动 {payload['startup']['wall_ms']} ms，稳态 RSS {payload['memory']['rss_idle_kb']} kb，"
        f"空闲 CPU {payload['cpu']['idle_cpu_ms']} ms/{payload['cpu']['idle_window_ms']} ms）"
    )


if __name__ == "__main__":
    main()
