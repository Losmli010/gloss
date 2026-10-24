#!/usr/bin/env python3
"""量化应用运行时资源：启动里程碑分段、常驻内存（RSS）与处理器占用（CPU）
采样、二进制体积。

每次运行把日志目录指向独立临时目录后拉起发行版二进制，轮询日志解析启动
里程碑行，并行按固定间隔采样进程的常驻内存与累计处理器时间；窗口、图形
（GPU）与预热帧就绪之后再采一段空闲窗口——空闲起点取就绪里程碑与后台
字体补装完成信号两者较晚者，保证空闲是真空闲；以终止信号收尾。共跑指定
轮数，各指标取中位数，连同环境快照写入本地产物片段（不入版本库，由统一
采集器折算进趋势点）。

启动总时长取拉起到就绪的观测墙钟（含进程加载，精度受轮询粒度限制），进
程内分段归因看生产日志里的启动里程碑。内存双口径：常驻内存按进程表采样
（压缩内存与交换区不计，低估真实占用），物理足迹按内存映像工具（vmmap）
采样（活动监视器口径，空闲窗口末采样一次并带进程生命期峰值），稳态与峰
值都进片段。空闲处理器占用是空闲窗口内的处理器时间增量，长驻进程预算口
径趋近于零——界面循环（winit）阻塞、监听线程（tap）睡在（CFRunLoop）、
后台运行时（tokio）空闲，任何子系统空转都在这里显形。体积按本机构建架
构分两栏登记（零新增采集）。墙钟与常驻内存跨机器不可比：片段只作趋势折
算，不做成败判定（趋势只认同机同环境历史）。

前置：发行版二进制已构建（对应配方先构建）。已有应用实例在跑时建议先退
出——测量会与第二实例短暂共存，采样按进程号隔离不受影响，但划词手势会
弹两张卡。
"""

import argparse
import json
import os
import platform
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
ROOT = SCRIPT_DIR.parents[1]
BINARY = ROOT / "target" / "release" / "gloss"
OUT_PATH = ROOT / "target" / "perf" / "app-runtime.json"

READY_MILESTONE = "06_ready"

# 本机构建架构 → 体积指标编号（编号按双架构分列，见指标注册表）
ARCH_BINARY_FIELDS = {"x86_64": "binary_x64_kb", "arm64": "binary_arm64_kb"}

# 平台 → 趋势点环境段的系统值（与自检信号行的系统常量同词表）
OS_NAMES = {"Darwin": "macos", "Linux": "linux", "Windows": "windows"}


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
    system = platform.system()
    return {
        "os": OS_NAMES.get(system, system.lower()),
        "arch": platform.machine(),
        "cpu": cpu,
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
            capture_output=True,
            text=True,
            check=True,
            timeout=10,
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


FOOTPRINT_VALUE = re.compile(r"^(\d+(?:\.\d+)?)([KMG]?)$")


def sample_footprint_kb(pid):
    """vmmap 采活动监视器口径的物理占用：空闲值与进程生命期峰值（kb）。

    进程已退出或 vmmap 不可用返回 None；只在空闲窗口末调用一次（vmmap
    单次数百毫秒，不宜进高频采样循环）。
    """
    try:
        out = subprocess.run(
            ["vmmap", "--summary", str(pid)],
            capture_output=True,
            text=True,
            check=True,
            timeout=30,
        ).stdout
    except (OSError, subprocess.SubprocessError):
        return None
    values = {}
    for line in out.splitlines():
        stripped = line.strip()
        for key in ("Physical footprint:", "Physical footprint (peak):"):
            if not stripped.startswith(key):
                continue
            # 数值与单位连写，正则一并吃下
            match = FOOTPRINT_VALUE.match(stripped[len(key) :].strip())
            if match is None:
                continue
            multiplier = {"": 1.0, "K": 1.0, "M": 1024.0, "G": 1024.0 * 1024.0}[match.group(2)]
            values[key] = round(float(match.group(1)) * multiplier)
    if "Physical footprint:" not in values:
        return None
    return values


def find_log_file(log_dir):
    files = sorted(log_dir.glob("gloss-*.jsonl"))
    return files[-1] if files else None


def parse_milestones(log_dir):
    """解析日志：里程碑 id → elapsed_ms，外加系统字体补装是否完成。"""
    log_file = find_log_file(log_dir)
    if log_file is None:
        return {}, False
    milestones = {}
    fonts_applied = False
    for line in log_file.read_text(encoding="utf-8").splitlines():
        if '"milestone"' in line:
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(event.get("milestone"), str) and isinstance(event.get("elapsed_ms"), int):
                milestones[event["milestone"]] = event["elapsed_ms"]
        elif '"system fonts applied"' in line:
            fonts_applied = True
    return milestones, fonts_applied


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
    fonts_seen_mono = None
    idle_start = None
    idle_cpu_start = None
    idle_wall_ms = None
    try:
        deadline = spawn_wall + ready_timeout
        fonts_deadline = spawn_wall + ready_timeout * 2
        while True:
            now = time.monotonic()
            sample = sample_process(proc.pid)
            if sample is not None:
                rss_samples.append(sample[0])
                cpu_samples.append((now, sample[1]))
            parsed, fonts_applied = parse_milestones(log_dir)
            milestones.update(parsed)
            if fonts_applied and fonts_seen_mono is None:
                fonts_seen_mono = now
            if ready_wall_ms is None and READY_MILESTONE in milestones:
                ready_wall_ms = (now - spawn_wall) * 1000
                ready_cpu = cpu_samples[-1][1] if cpu_samples else None
            # 空闲窗口起点：就绪与字体补装两个锚点都到齐，或按超时兜底起窗
            if (
                idle_start is None
                and ready_wall_ms is not None
                and (fonts_seen_mono is not None or now > fonts_deadline)
            ):
                idle_start = now
                if cpu_samples:
                    idle_cpu_start = cpu_samples[-1][1]
            if idle_start is not None and now - idle_start >= idle_secs:
                if proc.poll() is not None:
                    print("warning: app exited early, run discarded.", file=sys.stderr)
                    return None
                idle_wall_ms = (now - idle_start) * 1000
                footprint = sample_footprint_kb(proc.pid)
                break
            if ready_wall_ms is None and now > deadline:
                print(
                    f"warning: {READY_MILESTONE} not seen within {ready_timeout}s, run discarded.",
                    file=sys.stderr,
                )
                return None
            if proc.poll() is not None:
                print("warning: app exited early, run discarded.", file=sys.stderr)
                return None
            time.sleep(0.05 if ready_wall_ms is None else 1.0)
        if ready_cpu is None or idle_cpu_start is None or len(cpu_samples) < 2:
            print("warning: insufficient CPU samples, run discarded.", file=sys.stderr)
            return None
        fonts_applied_ms = (
            round((fonts_seen_mono - spawn_wall) * 1000) if fonts_seen_mono is not None else None
        )
        return {
            "wall_ms": round(ready_wall_ms),
            "cpu_ms": round((ready_cpu - cpu_samples[0][1]) * 1000),
            "fonts_applied_ms": fonts_applied_ms,
            "milestones": milestones,
            "rss_peak_kb": max(rss_samples),
            "rss_idle_kb": round(statistics.median(rss_samples[-5:])),
            "idle_window_ms": round(idle_wall_ms),
            "idle_cpu_ms": round((cpu_samples[-1][1] - idle_cpu_start) * 1000),
            "footprint_idle_kb": footprint and footprint.get("Physical footprint:"),
            "footprint_peak_kb": footprint and footprint.get("Physical footprint (peak):"),
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
    """各指标取中位数，平铺成 v2 片段字段（id 见 metrics.py 注册表；无值不造 0）。"""

    def med(key):
        values = [r[key] for r in runs if r.get(key) is not None]
        return round(statistics.median(values)) if values else None

    fragment = {}
    for field, key in (
        ("startup_ms", "wall_ms"),
        ("startup_cpu_ms", "cpu_ms"),
        ("rss_idle_kb", "rss_idle_kb"),
        ("rss_peak_kb", "rss_peak_kb"),
        ("cpu_idle_ms", "idle_cpu_ms"),
        ("footprint_idle_kb", "footprint_idle_kb"),
        ("footprint_peak_kb", "footprint_peak_kb"),
    ):
        value = med(key)
        if value is not None:
            fragment[field] = value
    return fragment


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
    parser.add_argument("--runs", type=int, default=3, help="number of runs, median per metric")
    parser.add_argument(
        "--idle-secs", type=int, default=30, help="idle sampling window seconds after ready"
    )
    parser.add_argument(
        "--ready-timeout", type=int, default=60, help="timeout seconds waiting for 06_ready"
    )
    args = parser.parse_args()

    if not BINARY.is_file():
        sys.exit(
            f"error: {BINARY.relative_to(ROOT)} not found, build the release binary first"
            " (the just app-metrics recipe runs cargo build --release first)."
        )
    if subprocess.run(["pgrep", "-x", "gloss"], capture_output=True).returncode == 0:
        print(
            "note: a gloss instance is already running; the measured instance will coexist briefly"
            " (sampling is pid-isolated), quit it before measuring.",
            file=sys.stderr,
        )

    runs = []
    for i in range(args.runs):
        print(f"run {i + 1}/{args.runs} (idle sampling {args.idle_secs}s after ready) ...")
        result = one_run(args.idle_secs, args.ready_timeout)
        if result is not None:
            runs.append(result)
    if not runs:
        sys.exit(
            "error: no run succeeded, cannot write the fragment (see the warnings above for causes)."
        )

    logs_bytes, logs_files = dir_usage(Path.home() / ".gloss" / "logs")
    arch_field = ARCH_BINARY_FIELDS.get(platform.machine())
    if arch_field is None:
        sys.exit(
            f"error: unrecognized host build arch {platform.machine()!r}, "
            f"cannot pick one of {'/'.join(ARCH_BINARY_FIELDS.values())}"
        )
    payload = aggregate(runs)
    payload[arch_field] = BINARY.stat().st_size // 1024
    payload["env"] = collect_env()
    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUT_PATH.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    rel = OUT_PATH.relative_to(ROOT)
    print(
        f"app runtime fragment written: {rel} ({len(runs)}/{args.runs} runs succeeded, "
        f"startup {payload['startup_ms']} ms, steady-state RSS {payload['rss_idle_kb']} kb, "
        f"idle CPU {payload['cpu_idle_ms']} ms; {logs_files} log files, {logs_bytes} B total, "
        f"to be folded into trend points by perf-collect.py)"
    )


if __name__ == "__main__":
    main()
