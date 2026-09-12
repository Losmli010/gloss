#!/usr/bin/env bash
# 硬编码密钥扫描门禁：扫描全部 git 跟踪文件，命中任一模式即失败。
# 本地 `just secrets`（pre-commit 的一部分）与 CI 的 quality job 共用此脚本，
# 保证本地与 CI 判定一致。
#
# 规则取向：只收高信号格式（provider 密钥前缀、私钥块、带引号的凭据赋值），
# 宁可漏报也不误报——漏报由 review 与轮换密钥兜底，误报会让门禁被习惯性绕过。
# 确属合法字面量（文档示例、测试夹具）时，在该行尾加 `secrets:allow` 放行，
# 并在旁边注明缘由。
#
# 注意：裸 `token` 关键字不进通用规则——热键解析（events/hotkey.rs）拿它当
# 变量名，误报率太高；真实 token 泄漏由 provider 前缀模式兜底。
set -euo pipefail

cd "$(dirname "$0")/.."

# ---- 配置区 ----
# provider 专属格式，大小写敏感（这些前缀本身就是大小写敏感的）
CASE_SENSITIVE_PATTERNS=(
  # OpenAI / Anthropic：sk- 左侧必须是非标识符字符，避免 task- / risk- 误报
  '(^|[^A-Za-z0-9_-])sk-ant-[A-Za-z0-9_-]{20,}'
  '(^|[^A-Za-z0-9_-])sk-[A-Za-z0-9_-]{20,}'
  # AWS（AKIA 长期密钥 / ASIA 临时凭证）
  'A[SK]IA[0-9A-Z]{16}'
  # GitHub（classic PAT / OAuth / fine-grained）
  'gh[pousr]_[A-Za-z0-9]{36,}'
  'github_pat_[A-Za-z0-9_]{20,}'
  # GitLab PAT
  'glpat-[A-Za-z0-9_-]{20,}'
  # Slack
  'xox[baprs]-[A-Za-z0-9-]{10,}'
  # Google API key
  'AIza[0-9A-Za-z_-]{35}'
  # 私钥块（任何算法）
  '-----BEGIN [A-Z ]*PRIVATE KEY-----'
)

# 通用凭据赋值，大小写不敏感：
# 1) 带引号的值（TOML/JSON/YAML 配置的常见形态），12 字符起；
# 2) 不带引号的值（shell 导出形态），20 字符起压误报。
# 不含裸 token：见文件头说明。
CASE_INSENSITIVE_PATTERNS=(
  '(api[_-]?key|secret|password|passwd|credential|access[_-]?key)["'"'"']?[[:space:]]*[:=][[:space:]]*["'"'"'][^"'"'"']{12,}["'"'"']'
  '(api[_-]?key|secret|password|passwd|credential|access[_-]?key)["'"'"']?[[:space:]]*[:=][[:space:]]*[A-Za-z0-9+/_-]{20,}'
  'authorization["'"'"']?[[:space:]]*[:=][[:space:]]*["'"'"']?bearer[[:space:]]+[A-Za-z0-9._-]{16,}'
)

ALLOW_MARKER="secrets:allow"

# ---- 扫描 ----
# git ls-files 只看跟踪文件：.gitignore 掉的本地文件不扫（也不该进仓库）。
# -I 跳过二进制文件（BSD/GNU grep 通用）；-z 处理带空格的路径。
matched=0
while IFS= read -r -d '' file; do
  for pat in "${CASE_SENSITIVE_PATTERNS[@]}"; do
    if grep -nE -I -- "$pat" "$file" 2>/dev/null | grep -vF "$ALLOW_MARKER" >/dev/null; then
      echo "疑似硬编码密钥: $file" >&2
      grep -nE -I -- "$pat" "$file" 2>/dev/null | grep -vF "$ALLOW_MARKER" | sed 's/^/  /' >&2
      matched=1
    fi
  done
  for pat in "${CASE_INSENSITIVE_PATTERNS[@]}"; do
    if grep -niE -I -- "$pat" "$file" 2>/dev/null | grep -vF "$ALLOW_MARKER" >/dev/null; then
      echo "疑似硬编码凭据: $file" >&2
      grep -niE -I -- "$pat" "$file" 2>/dev/null | grep -vF "$ALLOW_MARKER" | sed 's/^/  /' >&2
      matched=1
    fi
  done
done < <(git ls-files -z)

if [ "$matched" -ne 0 ]; then
  echo "" >&2
  echo "错误：检出疑似硬编码密钥/凭据（见上）。" >&2
  echo "  - 真实泄漏：立即吊销并轮换该密钥，改从配置存储读取；" >&2
  echo "  - 合法字面量（文档示例 / 测试夹具）：在该行尾加 ${ALLOW_MARKER} 并注明缘由。" >&2
  exit 1
fi

echo "✓ 密钥扫描通过（无硬编码密钥/凭据）"
exit 0
