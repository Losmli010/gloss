#!/usr/bin/env bash
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CHECKER="$SCRIPT_DIR/check-secrets.sh"

PASS=0
FAIL=0

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
git init -q "$TMP"
mkdir -p "$TMP/scripts/hooks"
cp "$CHECKER" "$TMP/scripts/hooks/"
chmod +x "$TMP/scripts/hooks/check-secrets.sh"

TAIL12="$(printf '0%.0s' {1..12})"
TAIL16="$(printf '0%.0s' {1..16})"
TAIL20="$(printf '0%.0s' {1..20})"
TAIL35="$(printf 'a%.0s' {1..35})"
TAIL36="$(printf 'a%.0s' {1..36})"
TAIL40="$(printf 'a%.0s' {1..40})"
AWS_TAIL16="$(printf 'A%.0s' {1..16})"

MARKER="secrets:allow"

assert_scan() {
  local desc="$1"
  local expected="$2"
  local rel="$3"
  local content="$4"

  mkdir -p "$TMP/$(dirname "$rel")"
  printf '%s\n' "$content" > "$TMP/$rel"
  git -C "$TMP" add "$rel" >/dev/null 2>&1

  local out actual
  out="$(bash "$TMP/scripts/hooks/check-secrets.sh" 2>&1 >/dev/null)"
  actual=$?

  if [ "$actual" -eq "$expected" ]; then
    echo "  ✓ $desc"
    PASS=$((PASS + 1))
  else
    echo "  ✗ $desc  (期望退出码 ${expected}，实际 ${actual})"
    echo "    夹具: ${rel}"
    echo "    checker 输出: $(printf '%s' "${out}" | head -3)"
    FAIL=$((FAIL + 1))
  fi

  git -C "$TMP" rm --cached -q "$rel" >/dev/null 2>&1
  rm -f "$TMP/$rel"
}

echo "== 测试 check-secrets.sh =="
echo ""
echo "-- 干净文件（应放行，退出码 0）--"
assert_scan "纯代码无密钥" 0 "clean.rs" 'let x = compute(input);'
assert_scan "提及 CancellationToken 不误报" 0 "clean2.rs" \
  "$(printf 'use tokio_util::sync::CancellationToken;\nlet cancel = CancellationToken::new();')"
assert_scan "task- 前缀不触发 sk- 规则" 0 "task-prefix.rs" \
  "$(printf 'let s = "task-%s";' "$TAIL40")"
assert_scan "裸 token 关键字不在通用规则内" 0 "bare-token.rs" \
  "$(printf 'let token = "cmd+shift+%s";' "$TAIL12")"
assert_scan "带引号短值（<12 字符）放行" 0 "short-quoted.toml" \
  "$(printf 'password = "%s"\n' "123456")"
assert_scan "未加引号短值（<20 字符）放行" 0 "short-unquoted.env" \
  "$(printf 'API_KEY=%s\n' "abc123")"

echo ""
echo "-- provider 密钥格式（应拦截，退出码 1）--"
assert_scan "OpenAI sk-" 1 "leak-openai.txt" \
  "$(printf 'key = "sk-%s"' "$TAIL40")"
assert_scan "Anthropic sk-ant-" 1 "leak-anthropic.txt" \
  "$(printf 'key = "sk-ant-%s"' "$TAIL40")"
assert_scan "AWS AKIA" 1 "leak-aws.txt" \
  "$(printf 'key = "AKIA%s"' "$AWS_TAIL16")"
assert_scan "GitHub classic PAT ghp_" 1 "leak-github.txt" \
  "$(printf 'key = "ghp_%s"' "$TAIL36")"
assert_scan "GitHub fine-grained github_pat_" 1 "leak-github-pat.txt" \
  "$(printf 'key = "github_pat_%s"' "$TAIL20")"
assert_scan "GitLab glpat-" 1 "leak-gitlab.txt" \
  "$(printf 'key = "glpat-%s"' "$TAIL20")"
assert_scan "Slack xoxb-" 1 "leak-slack.txt" \
  "$(printf 'key = "xoxb-%s"' "$TAIL20")"
assert_scan "Google AIza" 1 "leak-google.txt" \
  "$(printf 'key = "AIza%s"' "$TAIL35")"
assert_scan "私钥块" 1 "leak-key.pem" \
  "$(printf -- '-----BEGIN %s-----\nabcdef\n-----END %s-----\n' \
    'RSA PRIVATE KEY' 'RSA PRIVATE KEY')"

echo ""
echo "-- 通用凭据赋值（应拦截，退出码 1）--"
assert_scan "带引号 api_key = \"…\"" 1 "leak-generic.toml" \
  "$(printf 'api_key = "%s"\n' "$TAIL16")"
assert_scan "未加引号 API_KEY=…（20 字符起）" 1 "leak-generic.env" \
  "$(printf 'API_KEY=%s\n' "$TAIL20")"
assert_scan "credential 冒号形态（JSON/YAML）" 1 "leak-generic.json" \
  "$(printf '"credential": "%s"\n' "$TAIL16")"
assert_scan "Authorization Bearer 头" 1 "leak-bearer.txt" \
  "$(printf 'authorization: Bearer %s\n' "$TAIL20")"

echo ""
echo "-- secrets:allow 放行标记 --"
assert_scan "命中行带标记则放行" 0 "allow-ok.toml" \
  "$(printf 'api_key = "%s" # %s\n' "$TAIL16" "$MARKER")"
assert_scan "标记只放行所在行：同文件其他命中行仍拦截" 1 "allow-partial.toml" \
  "$(printf 'api_key = "%s" # %s\napi_key = "%s"\n' \
    "$TAIL16" "$MARKER" "$TAIL20")"
assert_scan "干净行加标记属多余但无害" 0 "allow-noise.txt" \
  "$(printf 'let x = 1; # %s\n' "$MARKER")"

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
