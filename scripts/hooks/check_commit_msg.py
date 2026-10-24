#!/usr/bin/env python3
# Conventional Commits 校验：本地 commit-msg 钩子（.githooks/commit-msg）、ZCode 客户端
# PR 标题钩子（.zcode/hooks/check_pr_title.py）与 CI 的 Commit message check job 共用此
# 单点，保证三处判定一致。
#
# 规则：header 形如 <type>(<scope>)?!: <subject>；type 白名单见 ALLOWED_TYPES；
# header ≤81 字节（按 UTF-8 字节计，跨实现一致）；正文与 header 之间必须有空行。
# git 忽略的 # 注释行只在多行输入时剥离——单行输入没有注释区，# 是标题本身
# （如 issue 引用式 PR 标题），剥离会被误报为空 message。
#
# 用法：scripts/hooks/check_commit_msg.py <commit-message-file|->
#   - 传文件路径：读取文件内容作为 commit message（本地 hook 场景）
#   - 传 "-" ：从 stdin 读取（CI 场景）

import re
import sys

ALLOWED_TYPES = "feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert"
HEADER_RE = re.compile(r"^(%s)(\([a-zA-Z0-9._-]+\))?!?: " % ALLOWED_TYPES)
SUBJECT_RE = re.compile(r"^[a-z]+(\([a-zA-Z0-9._-]+\))?!?: ")
MAX_HEADER_BYTES = 81


def split_message(msg):
    """预处理出待校验的行序列：剥注释行、去掉开头的空白行。"""
    msg = msg.rstrip("\n")
    if "\n" in msg:
        msg = "\n".join(line for line in msg.split("\n") if not line.startswith("#"))
    lines = msg.split("\n")
    first = 0
    while first < len(lines) and not lines[first].strip():
        first += 1
    return lines[first:]


def run(msg):
    """校验一条 commit message，返回多行错误串；通过返回 None。"""
    lines = split_message(msg)
    if not lines:
        return "错误：commit message 为空"

    header = lines[0]
    if not HEADER_RE.match(header):
        return (
            "错误：commit message 不符合 Conventional Commits 规范\n"
            "\n"
            "  规范格式: <type>[optional scope]: <subject>\n"
            "  type 可选: %s\n"
            "  示例:\n"
            "    feat: 添加翻译浮层\n"
            "    fix(core): 修复选区读取崩溃\n"
            "    chore(deps): 升级 egui 到 0.31\n"
            "    docs: 补充使用说明\n"
            "\n"
            "  你提交的是: %s" % (ALLOWED_TYPES, header)
        )

    subject = SUBJECT_RE.sub("", header, count=1)
    if not subject:
        return "错误：commit 标题缺少主题（冒号后应有内容）\n  你提交的是: %s" % header

    header_bytes = len(header.encode("utf-8"))
    if header_bytes > MAX_HEADER_BYTES:
        return (
            "错误：commit 标题过长（%d 字节，上限 %d）\n"
            "  建议精简，把细节放到正文\n"
            "  你提交的是: %s" % (header_bytes, MAX_HEADER_BYTES, header)
        )

    if len(lines) > 1 and lines[1] != "":
        return (
            "错误：commit 标题后应有一个空行，再写正文\n  第二行应为空行，你写的是: %s" % lines[1]
        )
    return None


def main():
    if len(sys.argv) < 2:
        print("用法: %s <commit-message-file|->" % sys.argv[0], file=sys.stderr)
        return 2
    if sys.argv[1] == "-":
        msg = sys.stdin.read()
    else:
        with open(sys.argv[1], encoding="utf-8", errors="replace") as fh:
            msg = fh.read()
    problem = run(msg)
    if problem:
        print(problem, file=sys.stderr)
        return 1
    print("✓ commit message 校验通过: %s" % split_message(msg)[0])
    return 0


if __name__ == "__main__":
    sys.exit(main())
