#!/usr/bin/env bash
# check-agents-doc.sh 的单元测试（纯 bash 轻量断言，零依赖）
# 覆盖：存在的引用、失效的配方、justfile 变量误判为配方、配方名前缀冲突、
# 缺失路径、缺失裸文件名、缺失测试目标、占位符与仓库外路径的跳过规则。
set -uo pipefail

# 定位脚本路径（与源文件同目录）
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CHECKER="$SCRIPT_DIR/check-agents-doc.sh"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

PASS=0
FAIL=0
CASE_NO=0

# 建一个最小可信的仓库夹具：justfile + AGENTS.md 可能引用的文件。
# 含 justfile 变量（coverage_min）、前缀冲突的配方名（logs / logs-dir）。
new_fixture() {
  local dir="$1"
  mkdir -p "$dir/crates/demo/tests" "$dir/tests" "$dir/docs"
  : >"$dir/Cargo.toml"
  : >"$dir/kittest.toml"
  : >"$dir/tests/overlay_selftest.rs"
  : >"$dir/crates/demo/tests/pipeline.rs"
  printf '%s\n' '# 说明文档' >"$dir/docs/note.md"
  cat >"$dir/justfile" <<'EOF'
coverage_min := "70"

run:
    cargo run

logs:
    @echo logs

logs-dir:
    @echo logs dir

test: run
    cargo test --workspace
EOF
}

# 断言助手：$1=描述  $2=期望退出码(0=通过,非0=拒绝)  $3=AGENTS.md 正文
assert_doc() {
  local desc="$1"
  local expected="$2"
  local body="$3"

  CASE_NO=$((CASE_NO + 1))
  local dir="$WORK/case${CASE_NO}"
  new_fixture "$dir"
  printf '%s\n' "$body" >"$dir/AGENTS.md"

  local checker_out
  checker_out="$("$CHECKER" "$dir/AGENTS.md" 2>&1)"
  local actual=$?

  if [ "$actual" -eq "$expected" ]; then
    echo "  ✓ $desc"
    PASS=$((PASS + 1))
  else
    # 变量一律加花括号：$var 后紧跟全角标点时，部分 bash 会把标点并进变量名，
    # set -u 下失败分支自己就报 unbound variable，反而盖掉真正的失败信息。
    echo "  ✗ $desc  (期望退出码 ${expected}，实际 ${actual})"
    echo "    checker 输出: $(printf '%s' "${checker_out}" | head -3)"
    FAIL=$((FAIL + 1))
  fi
}

echo "== 测试 check-agents-doc.sh =="
echo ""
echo "-- 合法用例（应通过，退出码 0）--"
assert_doc "引用全部存在" 0 "见 \`just test\`、\`just logs\`、\`just logs-dir\`、\`docs/note.md\`、\`Cargo.toml\`"
assert_doc "仓库路径（文件与目录）" 0 "产物在 \`crates/demo/tests/\`，配置见 \`kittest.toml\`"
assert_doc "cargo test 目标存在" 0 "跑 \`cargo test --test overlay_selftest\` 与 \`cargo test --test pipeline\`"
assert_doc "占位符与仓库外路径跳过校验" 0 "分支名 \`feat/<主题>\`，日志在 \`~/.gloss/logs/\`，模式 \`*.workspace = true\`"
assert_doc "注释语法与 URL 不算路径" 0 "每个 unsafe 块带 \`// SAFETY:\` 注释，文档见 \`https://example.com/a/b\`"
assert_doc "justfile 与 just --list 不算配方引用" 0 "全部 recipe 见 \`justfile\`，用 \`just --list\` 查看"
assert_doc "代码块内的配方引用同样校验（用户最常照抄的地方）" 0 "命令清单：

    just run
    just test"

echo ""
echo "-- 非法用例（应拒绝，退出码非 0）--"
assert_doc "配方不存在" 1 "跑 \`just deploy\` 发布"
assert_doc "纯文本里的配方不存在" 1 "先执行 just build-release 再打包"
assert_doc "justfile 变量被当成配方" 1 "阈值见 \`just coverage_min\`"
assert_doc "路径不存在" 1 "实现见 \`crates/demo/src/lib.rs\`"
assert_doc "裸文件名不存在" 1 "配置见 \`nonexistent.toml\`"
assert_doc "测试目标不存在" 1 "跑 \`cargo test --test smoke\`"
assert_doc "路径存在但写错了目录层级" 1 "见 \`crates/demo/tests/overlay_selftest.rs\`"
assert_doc "代码块里的配方也不放过" 1 "命令清单：

    just run
    just deploy"

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
