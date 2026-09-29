#!/usr/bin/env bash
# 生成 GitHub Pages 部署用的 manifest.json（输出到 stdout，日志走 stderr）。
# 版本单点复用 check-release-tag.sh：tag 格式与 Cargo.toml 版本一致性校验失败即失败；
# size 与 sha256 从产物文件实测计算，产物缺文件即失败，不得发出残缺清单。
# 用法: gen-manifest.sh <tag> <artifacts-dir>
#   artifacts-dir 内须有 gloss-<version>-<target>.zip 与 .dmg 共 4 个文件
#   （target = aarch64-apple-darwin / x86_64-apple-darwin）。
set -euo pipefail

SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ "$#" -ne 2 ]; then
  echo "用法: $0 <tag> <artifacts-dir>（例：$0 v0.1.0 artifacts）" >&2
  exit 2
fi
TAG="$1"
ARTDIR="$2"

"$SELF_DIR/check-release-tag.sh" "$TAG" >&2
VERSION="${TAG#v}"

PUBLISHED_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
BASE_URL="https://losmli010.github.io/gloss/latest"
NOTES_URL="https://github.com/Losmli010/gloss/releases/tag/${TAG}"

# 产物缺文件在主 shell 里失败（函数只在文件齐备后拼装，避免子 shell 吞掉退出）
for target in aarch64-apple-darwin x86_64-apple-darwin; do
  for ext in zip dmg; do
    file="${ARTDIR}/gloss-${VERSION}-${target}.${ext}"
    if [ ! -f "$file" ]; then
      echo "错误：缺产物文件 ${file}" >&2
      exit 1
    fi
  done
done

# 单架构条目：zip 取 size/sha256（客户端校验对象），dmg 只出直链供站点使用。
# 结果写入 ENTRY_JSON；主 shell 调用（非命令替换），失败即退出。
compute_entry() {
  local target="$1" stem size sha
  stem="gloss-${VERSION}-${target}"
  size="$(wc -c < "${ARTDIR}/${stem}.zip" | tr -d '[:space:]')"
  sha="$(shasum -a 256 "${ARTDIR}/${stem}.zip" | awk '{print $1}')"
  if ! printf '%s' "$sha" | grep -qE '^[0-9a-f]{64}$'; then
    echo "错误：sha256 计算结果异常：${sha}" >&2
    exit 1
  fi
  ENTRY_JSON="      \"${target}\": {
        \"url\": \"${BASE_URL}/${stem}.zip\",
        \"dmg_url\": \"${BASE_URL}/${stem}.dmg\",
        \"size\": ${size},
        \"sha256\": \"${sha}\"
      }"
}

compute_entry aarch64-apple-darwin
ENTRY_ARM="$ENTRY_JSON"
compute_entry x86_64-apple-darwin
ENTRY_X64="$ENTRY_JSON"

printf '%s\n' "{
  \"schema\": 1,
  \"version\": \"${VERSION}\",
  \"published_at\": \"${PUBLISHED_AT}\",
  \"notes_url\": \"${NOTES_URL}\",
  \"channels\": {
    \"stable\": {
${ENTRY_ARM},
${ENTRY_X64}
    }
  }
}"
