#!/usr/bin/env bash
# check-commit-msg.sh 的单元测试（纯 bash 轻量断言，零依赖）
# 覆盖：合法 type、scope、非法 type、空 message、超长 header、正文空行等边界情况。
set -uo pipefail

# 定位脚本路径（与源文件同目录）
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CHECKER="$SCRIPT_DIR/check-commit-msg.sh"

PASS=0
FAIL=0

# 断言助手：$1=描述  $2=期望退出码(0=通过,非0=拒绝)  $3=输入的 commit message
assert_exit() {
  local desc="$1"
  local expected="$2"
  local msg="$3"

  local checker_err
  checker_err="$(printf '%s\n' "$msg" | "$CHECKER" - 2>&1 >/dev/null)"
  local actual=$?

  if [ "$actual" -eq "$expected" ]; then
    echo "  ✓ $desc"
    PASS=$((PASS + 1))
  else
    echo "  ✗ $desc  (期望退出码 $expected，实际 $actual)"
    echo "    输入字节: $(printf '%s' "$msg" | od -c | head -2)"
    echo "    checker 输出: $(printf '%s' "$checker_err" | head -2)"
    FAIL=$((FAIL + 1))
  fi
}

echo "== 测试 check-commit-msg.sh =="
echo ""
echo "-- 合法用例（应通过，退出码 0）--"
assert_exit "feat: 基础功能" 0 "feat: 添加翻译浮层"
assert_exit "fix: 基础修复" 0 "fix: 修复选区读取崩溃"
assert_exit "带 scope" 0 "fix(core): 修复选区读取崩溃"
assert_exit "docs: 文档" 0 "docs: 补充使用说明"
assert_exit "chore: 杂项" 0 "chore(deps): 升级 egui 到 0.31"
assert_exit "revert: 回滚" 0 "revert: 回滚上一次提交"
assert_exit "破坏性变更 !" 0 "feat!: 破坏性 API 变更"
assert_exit "scope 带数字和点" 0 "fix(ci.yml): 修复流水线配置"
# 多行用例：用 printf 命令构造换行符，避免 $'...' 引号在不同 bash 版本下的差异
MSG_BODY_BLANK="$(printf 'feat: 标题\n\n这里是正文内容')"
assert_exit "正文与标题间有空行" 0 "$MSG_BODY_BLANK"
MSG_MULTILINE="$(printf 'feat: 标题\n\n正文第一行\n正文第二行')"
assert_exit "多行正文" 0 "$MSG_MULTILINE"

echo ""
echo "-- 非法用例（应拒绝，退出码非 0）--"
assert_exit "无 type 前缀" 1 "添加了某个功能"
assert_exit "非法 type: update" 1 "update: 更新依赖"
assert_exit "非法 type: 大写" 1 "FEAT: 大写前缀"
assert_exit "type 后缺冒号" 1 "feat 缺少冒号"
assert_exit "type 后缺空格" 1 "feat:没有空格"
assert_exit "空 message" 1 ""
assert_exit "纯空白" 1 "   "
assert_exit "scope 含空格" 1 "fix(bad scope): 非法 scope"
assert_exit "scope 为空括号" 1 "fix(): 空 scope"

echo ""
echo "-- 边界用例 --"
# 恰好 72 字符的 header（应通过）
local_72="feat: $(printf 'a%.0s' {1..66})"
assert_exit "header 恰好 72 字符" 0 "$local_72"
# 73 字符（应拒绝）
local_73="feat: $(printf 'a%.0s' {1..67})"
assert_exit "header 超过 72 字符" 1 "$local_73"
# 标题与正文之间无空行（应拒绝）
MSG_NO_BLANK="$(printf 'feat: 标题\n直接正文无空行')"
assert_exit "标题后无空行直接跟正文" 1 "$MSG_NO_BLANK"
# subject 非空即可（中文 2 字也应通过）
assert_exit "subject 单字符" 0 "feat: a"
assert_exit "subject 两字符" 0 "fix: ab"
assert_exit "subject 中文两字" 0 "feat: 标题"
# subject 为空（冒号后无内容，应拒绝）
assert_exit "subject 为空" 1 "feat: "

echo ""
echo "== 测试结果 =="
echo "  通过: $PASS"
echo "  失败: $FAIL"

if [ "$FAIL" -gt 0 ]; then
  echo ""
  echo "✗ 有 $FAIL 个测试失败"
  exit 1
fi

echo "✓ 全部 $PASS 个测试通过"
exit 0
