#!/usr/bin/env python3
# 发布冒烟：把打包出的应用包真正跑起来，验证「能启动、活得住、日志无崩溃」，
# 防「发了却跑不起来」。判定三条，全部基于可观察行为：
#   一、进程在预算时间内起来（窗口出现、事件循环在转）；
#   二、再观察一段窗口后仍存活（排除启动即崩的慢炸弹）；
#   三、日志出现启动标记，且没有任何崩溃（panic）。
# 辅助功能未授权只会让划词手势降级，不算冒烟失败——冒烟只管「能不能跑」。
# 用法：以解释器运行，首参为应用包路径。

import glob
import os
import shutil
import subprocess
import sys
import tempfile
import time

START_TIMEOUT = int(
    os.environ.get("SMOKE_START_TIMEOUT", "30")
)  # 等进程起来并写出启动日志的预算（秒）
OBSERVE_SECONDS = int(os.environ.get("SMOKE_OBSERVE_SECONDS", "5"))  # 起来后再观察多久（秒）
POLL_INTERVAL = 0.5


def alive(proc):
    return proc.poll() is None


def logs_contain(smoke_home, needle, ignore_case=False):
    logs_dir = os.path.join(smoke_home, ".gloss", "logs")
    if not os.path.isdir(logs_dir):
        return False
    for path in glob.glob(os.path.join(logs_dir, "**"), recursive=True):
        if not os.path.isfile(path):
            continue
        with open(path, encoding="utf-8", errors="replace") as fh:
            for line in fh:
                if ignore_case:
                    if needle.lower() in line.lower():
                        return True
                elif needle in line:
                    return True
    return False


def dump_logs(smoke_home, pattern):
    for path in sorted(glob.glob(os.path.join(smoke_home, pattern))):
        with open(path, encoding="utf-8", errors="replace") as fh:
            sys.stderr.write(fh.read())


def fail(msg):
    print(msg, file=sys.stderr)
    return 1


def main():
    if len(sys.argv) != 2:
        print("用法: %s <path/to/Gloss.app>" % sys.argv[0], file=sys.stderr)
        return 2

    app = sys.argv[1]
    bin_path = os.path.join(app, "Contents", "MacOS", "gloss")
    if not (os.path.isfile(bin_path) and os.access(bin_path, os.X_OK)):
        return fail("错误：找不到可执行文件：%s（.app 完整吗？）" % bin_path)

    smoke_home = tempfile.mkdtemp(prefix="gloss-smoke-")
    proc = None
    t0 = time.monotonic()
    try:
        # 日志隔离：换一个一次性用户目录，冒烟只认本次启动写出的日志，
        # 不碰开发机的真实日志目录。
        launch_log = os.path.join(smoke_home, "launch-stdout.log")
        env = os.environ.copy()
        env["HOME"] = smoke_home
        with open(launch_log, "ab") as log:
            proc = subprocess.Popen([bin_path], stdout=log, stderr=subprocess.STDOUT, env=env)
        print("已启动 pid=%d（日志隔离在 %s）" % (proc.pid, smoke_home))

        # ---- 判定 1：预算时间内进程活着，且日志写出启动标记 ----
        # 启动标记与入口日志宏的标记行对应；应用把日志落盘后才有。
        deadline = time.monotonic() + START_TIMEOUT
        started = False
        while time.monotonic() < deadline:
            if not alive(proc):
                print("错误：进程在启动阶段退出（等了 %d 秒内）" % START_TIMEOUT, file=sys.stderr)
                print("---- stdout/stderr ----", file=sys.stderr)
                with open(launch_log, encoding="utf-8", errors="replace") as fh:
                    sys.stderr.write(fh.read())
                return 1
            if logs_contain(smoke_home, "gloss starting"):
                started = True
                break
            time.sleep(POLL_INTERVAL)
        if not started:
            state = "alive" if alive(proc) else "dead"
            print(
                "错误：%ds 内没等到启动日志（进程状态：%s）" % (START_TIMEOUT, state),
                file=sys.stderr,
            )
            print("  没有窗口服务的环境起不来——冒烟需要本机或 macOS runner。", file=sys.stderr)
            print("---- stdout/stderr ----", file=sys.stderr)
            with open(launch_log, encoding="utf-8", errors="replace") as fh:
                sys.stderr.write(fh.read())
            print("---- 应用日志目录 ----", file=sys.stderr)
            logs_dir = os.path.join(smoke_home, ".gloss", "logs")
            if os.path.isdir(logs_dir):
                for entry in sorted(os.listdir(logs_dir)):
                    sys.stderr.write("  %s\n" % entry)
            else:
                print("（目录未创建）", file=sys.stderr)
            return 1
        print("✓ 启动成功并写出日志（%ds）" % int(time.monotonic() - t0))

        # ---- 判定 2：观察窗口内持续存活 ----
        deadline = time.monotonic() + OBSERVE_SECONDS
        while time.monotonic() < deadline:
            if not alive(proc):
                print(
                    "错误：启动成功后 %d 秒观察窗口内退出（慢启动崩溃）" % OBSERVE_SECONDS,
                    file=sys.stderr,
                )
                print("---- 应用日志 ----", file=sys.stderr)
                dump_logs(smoke_home, os.path.join(".gloss", "logs", "gloss.log.*"))
                return 1
            time.sleep(POLL_INTERVAL)
        print("✓ 观察窗口 %ds 内持续存活" % OBSERVE_SECONDS)

        # ---- 判定三：日志无崩溃（panic） ----
        if logs_contain(smoke_home, "panic", ignore_case=True):
            print("错误：日志里出现 panic：", file=sys.stderr)
            dump_logs(smoke_home, os.path.join(".gloss", "logs", "*"))
            return 1
        print("✓ 日志无 panic")
    finally:
        if proc is not None:
            try:
                proc.terminate()
            except OSError:
                pass
            proc.wait()
        shutil.rmtree(smoke_home, ignore_errors=True)
    print("✓ 冒烟通过：产物能启动、能存活、日志干净")
    return 0


if __name__ == "__main__":
    sys.exit(main())
