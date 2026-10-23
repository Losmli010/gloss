#!/usr/bin/env python3
# 硬编码密钥扫描门禁：扫描全部 git 跟踪文件，命中任一模式即失败。
# 本地 `just secrets`（pre-commit 的一部分）与 CI 的 Secrets check job 共用此脚本，
# 保证本地与 CI 判定一致。
#
# 规则取向：只收高信号格式（provider 密钥前缀、私钥块、带引号的凭据赋值）。
# 确属合法字面量（文档示例、测试夹具）时，在该行尾加 `secrets:allow` 放行，
# 并在旁边注明缘由。

import os
import re
import subprocess
import sys

# provider 专属格式，大小写敏感（这些前缀本身就是大小写敏感的）
CASE_SENSITIVE_PATTERNS = [
    # OpenAI / Anthropic：sk- 左侧必须是非标识符字符，避免 task- / risk- 误报
    re.compile(r"(^|[^A-Za-z0-9_-])sk-ant-[A-Za-z0-9_-]{20,}"),
    re.compile(r"(^|[^A-Za-z0-9_-])sk-[A-Za-z0-9_-]{20,}"),
    # AWS（AKIA 长期密钥 / ASIA 临时凭证）
    re.compile(r"A[SK]IA[0-9A-Z]{16}"),
    # GitHub（classic PAT / OAuth / fine-grained）
    re.compile(r"gh[pousr]_[A-Za-z0-9]{36,}"),
    re.compile(r"github_pat_[A-Za-z0-9_]{20,}"),
    # GitLab PAT
    re.compile(r"glpat-[A-Za-z0-9_-]{20,}"),
    # Slack
    re.compile(r"xox[baprs]-[A-Za-z0-9-]{10,}"),
    # Google API key
    re.compile(r"AIza[0-9A-Za-z_-]{35}"),
    # 私钥块（任何算法）
    re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----"),
]

# 通用凭据赋值，大小写不敏感：
# 1) 带引号的值（TOML/JSON/YAML 配置的常见形态），12 字符起；
# 2) 不带引号的值（shell 导出形态），20 字符起压误报。
# 不含裸 token 关键字。
CASE_INSENSITIVE_PATTERNS = [
    re.compile(
        r'(api[_-]?key|secret|password|passwd|credential|access[_-]?key)["\']?'
        r"\s*[:=]\s*[\"'][^\"']{12,}[\"']",
        re.IGNORECASE,
    ),
    re.compile(
        r"(api[_-]?key|secret|password|passwd|credential|access[_-]?key)[\"']?"
        r"\s*[:=]\s*[A-Za-z0-9+/_-]{20,}",
        re.IGNORECASE,
    ),
    re.compile(
        r'authorization["\']?\s*[:=]\s*["\']?bearer\s+[A-Za-z0-9._-]{16,}',
        re.IGNORECASE,
    ),
]

ALLOW_MARKER = "secrets:allow"


def tracked_files(root):
    # 只看跟踪文件：.gitignore 掉的本地文件不扫（也不该进仓库）
    out = subprocess.run(
        ["git", "-C", root, "ls-files", "-z"],
        capture_output=True,
        check=True,
    ).stdout
    return [p.decode("utf-8", "replace") for p in out.split(b"\0") if p]


def hit_lines(path, pattern):
    """文件内逐行匹配；跳过二进制（对齐 grep -I）与带放行标记的行。"""
    try:
        with open(path, "rb") as fh:
            data = fh.read()
    except OSError:
        return []
    if b"\0" in data:
        return []
    text = data.decode("utf-8", errors="replace")
    hits = []
    for lineno, line in enumerate(text.split("\n"), 1):
        if ALLOW_MARKER in line:
            continue
        if pattern.search(line):
            hits.append((lineno, line))
    return hits


def run(root):
    """扫描 root 下全部 git 跟踪文件，返回违规条目（每条含文件与命中行）。"""
    problems = []
    for rel in tracked_files(root):
        path = os.path.join(root, rel)
        for kind, patterns in (
            ("疑似硬编码密钥", CASE_SENSITIVE_PATTERNS),
            ("疑似硬编码凭据", CASE_INSENSITIVE_PATTERNS),
        ):
            for pattern in patterns:
                hits = hit_lines(path, pattern)
                if hits:
                    detail = "\n".join("  %d:%s" % hit for hit in hits)
                    problems.append("%s: %s\n%s" % (kind, rel, detail))
    return problems


def main():
    root = (
        sys.argv[1]
        if len(sys.argv) > 1
        else os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    )
    problems = run(root)
    if problems:
        for p in problems:
            print(p, file=sys.stderr)
        print("", file=sys.stderr)
        print("错误：检出疑似硬编码密钥/凭据（见上）。", file=sys.stderr)
        print("  - 真实泄漏：立即吊销并轮换该密钥，改从配置存储读取；", file=sys.stderr)
        print(
            "  - 合法字面量（文档示例 / 测试夹具）：在该行尾加 %s 并注明缘由。" % ALLOW_MARKER,
            file=sys.stderr,
        )
        return 1
    print("✓ 密钥扫描通过（无硬编码密钥/凭据）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
