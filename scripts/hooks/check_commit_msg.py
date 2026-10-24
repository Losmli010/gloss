#!/usr/bin/env python3
# 提交信息规范校验：本地提交钩子、ZCode 客户端的拉取请求标题检查与持续集成检查
# 共用此单点，保证多处判定一致。
#
# 规则：标题形如「类型（可选范围）可加叹号，冒号后接主题」；类型白名单见下方常量；
# 标题按 UTF-8 字节计不超上限（跨实现一致）；正文与标题之间必须有空行。
# 井号起头的注释行只在多行输入时剥离——单行输入没有注释区，井号是标题本身
# （如议题引用式拉取请求标题），剥离会被误报为空消息。
#
# 用法：以解释器运行本脚本，首参为消息文件路径，或传单个减号表示从标准输入读取
# （本地钩子传文件路径，持续集成传减号）。

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
