#!/usr/bin/env bash
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CHECKER="$SCRIPT_DIR/check-i18n.sh"

PASS=0
FAIL=0

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
git init -q "$TMP"
mkdir -p "$TMP/scripts/hooks"
cp "$CHECKER" "$TMP/scripts/hooks/"
chmod +x "$TMP/scripts/hooks/check-i18n.sh"

MARKER="i18n:allow"

# 基线：文案表装入点（owner）与词条表各就位，全部用例共享
mkdir -p "$TMP/crates/gloss-app/src/ui" "$TMP/crates/gloss-app/i18n"
printf '%s\n' 'const ZH: &str = include_str!("../../i18n/zh.toml");' > "$TMP/crates/gloss-app/src/ui/i18n.rs"
printf '%s\n' 'gloss_app_title = "Gloss"' > "$TMP/crates/gloss-app/i18n/zh.toml"

assert_scan() {
  local desc="$1"
  local expected="$2"
  local rel="$3"
  local content="$4"
  local expect="${5:-}"

  mkdir -p "$TMP/$(dirname "$rel")"
  printf '%s\n' "$content" > "$TMP/$rel"

  local out actual
  out="$(bash "$TMP/scripts/hooks/check-i18n.sh" 2>&1 >/dev/null)"
  actual=$?

  if [ "$actual" -eq "$expected" ] &&
    { [ -z "$expect" ] || printf '%s' "${out}" | grep -qF "$expect"; }; then
    echo "  ✓ $desc"
    PASS=$((PASS + 1))
  else
    echo "  ✗ $desc  (期望退出码 ${expected}，实际 ${actual}${expect:+；期望输出含「${expect}」})"
    echo "    夹具: ${rel}"
    echo "    checker 输出: $(printf '%s' "${out}" | head -3)"
    FAIL=$((FAIL + 1))
  fi

  rm -f "$TMP/$rel"
}

echo "== 测试 check-i18n.sh =="
echo ""
echo "-- 基线（应放行，退出码 0）--"
assert_scan "owner 装入点 + 词条表就位" 0 "crates/gloss-app/src/.keep" ""

echo ""
echo "-- 非判罚面（应放行，退出码 0）--"
assert_scan "中文行注释与文档注释" 0 "crates/gloss-app/src/comments.rs" \
  "$(printf '// 中文行注释\n/// 中文文档注释\nlet x = 1;')"
assert_scan "嵌套块注释中文" 0 "crates/gloss-app/src/block.rs" \
  "$(printf '/* 外层注释 /* 内层中文 */ 仍是注释 */\nfn main() { let x = 1; }')"
assert_scan "多行属性内的中文 reason" 0 "crates/gloss-app/src/attr.rs" \
  "$(printf '#[allow(\n    clippy::too_many_arguments,\n    reason = "组装点的主入口：字段袋说明"\n)]\npub fn run() {}')"
assert_scan "cfg(test) mod tests 内的中文断言" 0 "crates/gloss-app/src/with_tests.rs" \
  "$(printf 'pub fn add(a: u32, b: u32) -> u32 { a + b }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {\n        assert_eq!(1, 1, "中文断言");\n    }\n}')"
assert_scan "cfg(test) pub(crate) mod 内的中文" 0 "crates/gloss-app/src/support.rs" \
  "$(printf 'pub fn visible() -> u32 { 1 }\n\n#[cfg(test)]\npub(crate) mod support {\n    pub fn need() {\n        panic!("中文指引");\n    }\n}')"
assert_scan "嵌套 cfg(test) mod：内层闭合后外层仍豁免" 0 "crates/gloss-app/src/nested_tests.rs" \
  "$(printf 'pub fn visible() -> u32 { 1 }\n\n#[cfg(test)]\nmod outer {\n    use super::*;\n\n    #[cfg(test)]\n    mod inner {\n        #[test]\n        fn t() {\n            assert_eq!(1, 1, "内层中文断言");\n        }\n    }\n\n    #[test]\n    fn u() {\n        assert!(true, "外层中文断言");\n    }\n}')"
assert_scan "文件级 #![...] 内联属性内的中文" 0 "crates/gloss-app/src/inner_attr.rs" \
  "$(printf '#![allow(clippy::too_many_arguments, reason = "内联属性理由：字段袋说明")]\nfn main() {}')"
assert_scan "生命周期与 ASCII char 不误报" 0 "crates/gloss-app/src/lifetimes.rs" \
  "$(printf "fn pick<'a>(a: &'a str, b: &'a str) -> &'a str { if a.len() > b.len() { a } else { b } }\nlet c = 'x';\nlet s: &'static str = \"ok\";")"
assert_scan "i18n:allow 放行所在行" 0 "crates/gloss-app/src/allow_ok.rs" \
  "$(printf 'let gear = icon_button("⚙"); // %s 图标字形，非 locale 文案\n' "$MARKER")"

echo ""
echo "-- 文案字面量（应拦截，退出码 1）--"
assert_scan "生产代码内嵌中文文案" 1 "crates/gloss-app/src/hardcoded.rs" \
  "$(printf 'fn label() -> String {\n    "打开设置".to_owned()\n}')"
assert_scan "raw string 内嵌中文" 1 "crates/gloss-app/src/raw.rs" \
  "$(printf 'let s = r#"中文正文"#;')"
assert_scan "char 字面量内嵌中文" 1 "crates/gloss-app/src/charlit.rs" \
  "$(printf "let seal = '疏';")"
assert_scan "标记只放行所在行：其他命中行仍拦截" 1 "crates/gloss-app/src/allow_partial.rs" \
  "$(printf 'let a = "放大"; // %s 合法字形\nlet b = "打开设置";\n' "$MARKER")"
assert_scan "根包 src 同样受管" 1 "src/main.rs" \
  "$(printf 'fn main() {\n    let _ = "入口文案";\n}')"

echo ""
echo "-- 词法边界 --"
assert_scan "cfg(test) mod 闭合后的生产代码仍受管" 1 "crates/gloss-app/src/late.rs" \
  "$(printf '#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {\n        assert_eq!(1, 1, "中文断言");\n    }\n}\n\nlet _ = "模块之后的生产文案";')"
assert_scan "字符串里的 // 不当注释吞掉后续命中" 1 "crates/gloss-app/src/url.rs" \
  "$(printf 'let url = "https://example.com/a";\nlet s = "中文";')"
assert_scan "字符串续行不使行号失步（报告行=真实行）" 1 "crates/gloss-app/src/continuation.rs" \
  "$(printf 'let a = "no chinese \\\nstill ascii";\n\nlet s = "打开设置";\n')" \
  "continuation.rs 第 4 行"
assert_scan "字符串里的 ] 不破坏属性配对" 0 "crates/gloss-app/src/bracket.rs" \
  "$(printf '#[allow(clippy::doc_markdown, reason = "数组 [0] 说明")]\nlet x = 1;')"

echo ""
echo "-- 扩展扫描面：gloss-core 与 gloss-platform --"
assert_scan "gloss-core 生产代码内嵌中文拦截" 1 "crates/gloss-core/src/hardcoded.rs" \
  "$(printf 'fn label() -> String {\n    "未能读取选区".to_owned()\n}')"
assert_scan "gloss-platform 生产代码内嵌中文拦截" 1 "crates/gloss-platform/src/hardcoded.rs" \
  "$(printf 'pub fn hint() -> &\x27static str {\n    "打开设置"\n}')"
assert_scan "prompt.rs 作为模型面向文案处理点整文件豁免" 0 "crates/gloss-core/src/prompt.rs" \
  "$(printf 'fn lang_name() -> &\x27static str {\n    "中文"\n}')"
assert_scan "core 生产代码的 i18n:allow 放行" 0 "crates/gloss-core/src/allowed.rs" \
  "$(printf "const DOTS: &str = \"…\"; // %s 诊断省略号，非 locale 文案\n" "$MARKER")"

echo ""
echo "-- 装入点集中与词条表唯一（应拦截，退出码 1）--"
assert_scan "owner 之外 include_str 引用 i18n 资源" 1 "crates/gloss-core/src/alias.rs" \
  "$(printf 'const P: &str = include_str!("../i18n/zh.toml");')" \
  "alias.rs"
assert_scan "owner 之外 include_bytes 引用 i18n 资源" 1 "crates/gloss-core/src/blob.rs" \
  "$(printf 'const B: &[u8] = include_bytes!("../i18n/zh.toml");')" \
  "blob.rs"
assert_scan "i18n 目录之外声明词条键的 TOML" 1 "extra.toml" \
  "$(printf 'gloss_extra_foo = "x"\n')"
assert_scan "i18n 目录之外的普通 TOML 键放行" 0 "plain.toml" \
  "$(printf 'app_title = "Gloss"\n')"

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
