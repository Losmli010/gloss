#!/usr/bin/env bash
# 发布冒烟：把打包出的 .app 真正跑起来，验证「能启动、活得住、日志无崩溃」，
# 防「发了却跑不起来」。判定三条，全部基于可观察行为：
#   1. 进程在预算时间内起来（窗口出现、事件循环在转）；
#   2. 再观察一段窗口后仍存活（排除启动即崩的慢炸弹）；
#   3. 日志出现启动标记，且没有任何 panic。
# 辅助功能未授权只会让划词手势降级（设计如此），不算冒烟失败——冒烟只管
# 「这个产物能不能跑」。因此本脚本可在本机与 macOS CI runner 上直接运行。
# 用法：./scripts/smoke-app.sh <path/to/Gloss.app>
set -uo pipefail

START_TIMEOUT="${SMOKE_START_TIMEOUT:-30}"   # 等进程起来并写出启动日志的预算（秒）
OBSERVE_SECONDS="${SMOKE_OBSERVE_SECONDS:-5}" # 起来后再观察多久（秒）
POLL_INTERVAL="0.5"

if [ "$#" -ne 1 ]; then
  echo "用法: $0 <path/to/Gloss.app>" >&2
  exit 2
fi

APP="$1"
BIN="$APP/Contents/MacOS/gloss"
if [ ! -x "$BIN" ]; then
  echo "错误：找不到可执行文件：${BIN}（.app 完整吗？）" >&2
  exit 1
fi

# 日志隔离：应用把日志写到 $HOME/.gloss/logs，这里换一个一次性 HOME，
# 冒烟只认本次启动写出的日志，不碰开发机的真实日志目录。
# 直接拉二进制而不是 open：LaunchServices 不透传 HOME，隔离需要继承环境。
SMOKE_HOME="$(mktemp -d)"
trap 'rm -rf "$SMOKE_HOME"; [ -n "${PID:-}" ] && kill "$PID" 2>/dev/null || true' EXIT
LAUNCH_LOG="$SMOKE_HOME/launch-stdout.log"

HOME="$SMOKE_HOME" "$BIN" >>"$LAUNCH_LOG" 2>&1 &
PID=$!
echo "已启动 pid=${PID}（日志隔离在 ${SMOKE_HOME}）"

# ---- 判定 1：预算时间内进程活着，且日志写出启动标记 ----
# 启动标记与 src/main.rs 的 info!("gloss starting") 对应；应用把日志落盘后才有。
deadline=$((SECONDS + START_TIMEOUT))
started=""
while [ "$SECONDS" -lt "$deadline" ]; do
  if ! kill -0 "$PID" 2>/dev/null; then
    echo "错误：进程在启动阶段退出（等了 $((deadline - SECONDS)) 秒内）" >&2
    echo "---- stdout/stderr ----" >&2
    cat "$LAUNCH_LOG" >&2
    exit 1
  fi
  if grep -rq "gloss starting" "$SMOKE_HOME/.gloss/logs" 2>/dev/null; then
    started="yes"
    break
  fi
  sleep "$POLL_INTERVAL"
done

if [ -z "$started" ]; then
  echo "错误：${START_TIMEOUT}s 内没等到启动日志（进程状态：$(kill -0 "$PID" 2>/dev/null && echo alive || echo dead)）" >&2
  echo "  没有窗口服务的环境起不来——冒烟需要本机或 macOS runner。" >&2
  echo "---- stdout/stderr ----" >&2
  cat "$LAUNCH_LOG" >&2
  echo "---- 应用日志目录 ----" >&2
  ls -la "$SMOKE_HOME/.gloss/logs" 2>/dev/null || echo "（目录未创建）" >&2
  exit 1
fi
echo "✓ 启动成功并写出日志（${SECONDS}s）"

# ---- 判定 2：观察窗口内持续存活 ----
deadline=$((SECONDS + OBSERVE_SECONDS))
while [ "$SECONDS" -lt "$deadline" ]; do
  if ! kill -0 "$PID" 2>/dev/null; then
    echo "错误：启动成功后 $OBSERVE_SECONDS 秒观察窗口内退出（慢启动崩溃）" >&2
    echo "---- 应用日志 ----" >&2
    cat "$SMOKE_HOME"/.gloss/logs/gloss.log.* 2>/dev/null >&2
    exit 1
  fi
  sleep "$POLL_INTERVAL"
done
echo "✓ 观察窗口 ${OBSERVE_SECONDS}s 内持续存活"

# ---- 判定 3：日志无 panic ----
if grep -rqi "panic" "$SMOKE_HOME/.gloss/logs" 2>/dev/null; then
  echo "错误：日志里出现 panic：" >&2
  grep -ri "panic" "$SMOKE_HOME/.gloss/logs" >&2
  exit 1
fi
echo "✓ 日志无 panic"

kill "$PID" 2>/dev/null
wait "$PID" 2>/dev/null
echo "✓ 冒烟通过：产物能启动、能存活、日志干净"
