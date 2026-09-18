#!/usr/bin/env bash
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CHECKER="$SCRIPT_DIR/check-commit-msg.sh"

PASS=0
FAIL=0

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
    echo "  ✗ $desc  (期望退出码 ${expected}，实际 ${actual})"
    echo "    输入字节: $(printf '%s' "${msg}" | od -c | head -2)"
    echo "    checker 输出: $(printf '%s' "${checker_err}" | head -2)"
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
assert_exit "scope 含逗号" 1 "fix(bad,scope): scope 不允许逗号"
assert_exit "scope 为空括号" 1 "fix(): 空 scope"

echo ""
echo "-- 边界用例 --"
local_81="feat: $(printf 'a%.0s' {1..75})"
assert_exit "header 恰好 81 字节" 0 "$local_81"
local_82="feat: $(printf 'a%.0s' {1..76})"
assert_exit "header 超过 81 字节" 1 "$local_82"
MSG_NO_BLANK="$(printf 'feat: 标题\n直接正文无空行')"
assert_exit "标题后无空行直接跟正文" 1 "$MSG_NO_BLANK"
assert_exit "subject 单字符" 0 "feat: a"
assert_exit "subject 两字符" 0 "fix: ab"
assert_exit "subject 中文两字" 0 "feat: 标题"
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
