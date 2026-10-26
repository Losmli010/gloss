#!/usr/bin/env python3
"""硬编码密钥扫描门禁：扫描版本库跟踪的全部文件，命中任一模式即失败。
本地提交前门禁与持续集成检查共用此脚本，保证本地与远端判定一致。

规则取向：只收高信号格式（服务商密钥前缀、私钥块、带引号的凭据赋值）。
确属合法字面量（文档示例、测试夹具）时，在该行尾加放行标记并注明缘由。
匹配委托共享检索引擎（ripgrep）：引擎入口住检索引擎模块单点收口，模式清
单在本文件单点定义，运行时拼装参数一次子进程跑全部模式；二进制文件与放
行标记行由本文件前后滤除，放行标记须带非空缘由、缺失即判罚，判定口径与
既有逐行实现一致，缘由见记录文件。
违规报告经统一诊断出口打印：每
条带规则标识、完整证据行与修复指引，退出码按全脚本统一约定。
"""

from __future__ import annotations

import os
import re
import sys
from typing import Any

import gate_report
import rg_engine

# 服务商专属格式，区分大小写（这些前缀本身就有大小写语义）
CASE_SENSITIVE_PATTERNS = [
    # 模型服务商（OpenAI）等：短前缀左侧必须是非标识符字符，避免普通英文词连字误报
    re.compile(r"(^|[^A-Za-z0-9_-])sk-[A-Za-z0-9_-]{20,}"),
    # 云服务商：长期密钥与临时凭证各有专属前缀
    re.compile(r"A[SK]IA[0-9A-Z]{16}"),
    # 托管平台（GitHub）令牌：经典、应用与细粒度三种前缀
    re.compile(r"gh[pousr]_[A-Za-z0-9]{36,}"),
    re.compile(r"github_pat_[A-Za-z0-9_]{20,}"),
    # 另一代码托管平台的访问令牌
    re.compile(r"glpat-[A-Za-z0-9_-]{20,}"),
    # 即时通讯平台的令牌，前缀按字母位区分类型
    re.compile(r"xox[baprs]-[A-Za-z0-9-]{10,}"),
    # 云服务接口密钥，前缀定长
    re.compile(r"AIza[0-9A-Za-z_-]{35}"),
    # 私钥块（任何算法）
    re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----"),
]

# 通用凭据赋值，大小写不敏感：
# 其一，带引号的值（各类配置文件的常见形态），十二字符起；
# 其二，不带引号的值（环境变量导出形态），二十字符起压误报；
# 不含裸令牌关键字——真实令牌泄漏由服务商前缀模式兜底。
CREDENTIAL_PATTERNS = [
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
]

# 授权头携带的持票人凭据，与通用赋值分属不同规则标识
AUTH_PATTERNS = [
    re.compile(
        r'authorization["\']?\s*[:=]\s*["\']?bearer\s+[A-Za-z0-9._-]{16,}',
        re.IGNORECASE,
    ),
]

# 大小写不敏感组合：引擎参数与既有单测都按此清单序消费
CASE_INSENSITIVE_PATTERNS = CREDENTIAL_PATTERNS + AUTH_PATTERNS

ALLOW_MARKER = "secrets:allow"

RULE_SECRET_VENDOR = "SECRET-VENDOR"
RULE_SECRET_CREDENTIAL = "SECRET-CREDENTIAL"  # secrets:allow 规则标识常量是登记名不是凭据
RULE_SECRET_AUTH = "SECRET-AUTH"

VENDOR_MESSAGE = "suspected vendor-specific hardcoded secret"
CREDENTIAL_MESSAGE = "suspected hardcoded credential assignment"
AUTH_MESSAGE = "suspected bearer token in authorization header"

REMEDIY = (
    "real leak: revoke and rotate the secret immediately, then read it from the"
    " config store; legitimate literal (doc example / test fixture): add"
    " secrets:allow at the end of the line with a reason"
)

# 规则分组：模式清单到标识与消息的固定映射，报告顺序即此序
RULE_GROUPS = (
    (RULE_SECRET_VENDOR, VENDOR_MESSAGE, CASE_SENSITIVE_PATTERNS),
    (RULE_SECRET_CREDENTIAL, CREDENTIAL_MESSAGE, CREDENTIAL_PATTERNS),
    (RULE_SECRET_AUTH, AUTH_MESSAGE, AUTH_PATTERNS),
)

# 字节序标记前缀：引擎会剥除或据此整文件转码，与既有的容错解码口径对不齐
BOM_PREFIXES = (b"\xff\xfe", b"\xfe\xff", b"\xef\xbb\xbf")

# 空白类超集：解释器的空白类比引擎多四个控制分隔符，候选面按超集展开后
# 引擎筛出的行必不漏解释器可判的行，判定仍由解释器侧模式收口
SPACE_SUPERSET = r"[\s\x1c-\x1f]"


def tracked_files(root: str) -> list[str]:
    # 只看版本库跟踪的文件：被忽略规则排除的本地文件不扫（也不该进仓库）
    out = gate_report.run_git(["ls-files", "-z"], cwd=root).stdout
    return [p.decode("utf-8", "replace") for p in out.split(b"\0") if p]


def scannable_files(root: str, loader: Any = None) -> tuple[list[str], set[str]]:
    """跟踪文件分流：工作区不在场或含空字节的整文件跳过，带字节序标记的改逐行兜底。

    返回可扫路径的清单序与其中需兜底子集，其余路径交引擎；装入器在场时
    清单枚举与字节读取走共享缓存。"""
    out = []
    fallback = set()
    tracked = loader.tracked() if loader is not None else tracked_files(root)
    for rel in tracked:
        try:
            if loader is not None:
                data = loader.read_bytes(rel)
            else:
                with open(os.path.join(root, rel), "rb") as fh:
                    data = fh.read()
        except OSError:
            continue
        if b"\0" in data:
            continue
        if data.startswith(BOM_PREFIXES):
            fallback.add(rel)
        out.append(rel)
    return out, fallback


def engine_expressions() -> list[str]:
    """把模式清单翻成引擎参数序列：不敏感组加内联旗标前缀并把空白类放大成超集。

    正则本体不复制第二份——超集只作用在候选预筛上，归类判定仍跑清单原模式；
    引擎的方括号类支持嵌套，空白类出现在类内时替换也成活。"""
    out = []
    for pattern in CASE_SENSITIVE_PATTERNS:
        out.append(pattern.pattern)
    for pattern in CASE_INSENSITIVE_PATTERNS:
        out.append("(?i)" + pattern.pattern.replace("\\s", SPACE_SUPERSET))
    return out


def collect_hits(root: str, files: list[str]) -> dict[str, dict[int, str]]:
    """一次子进程跑全部模式（共享检索引擎），命中整理成「路径到行号与行文」的嵌套映射。"""
    return rg_engine.run(root, engine_expressions(), files)


def pattern_hits(pattern: re.Pattern[str], file_hits: dict[int, str]) -> list[tuple[int, str]]:
    """单模式的可见命中：放行标记行先滤掉，其余命中行按本模式逐条归类。"""
    return [
        (lineno, line)
        for lineno, line in sorted(file_hits.items())
        if ALLOW_MARKER not in line and pattern.search(line)
    ]


def allow_reason_findings(rel: str, file_hits: dict[int, str]) -> list[gate_report.Finding]:
    """放行标记缺缘由的条目：标记行只有在命中扫描模式时才构成豁免现场，
    缺同行非空缘由即报告；豁免语义不变，标记行照旧放行。"""
    return [
        gate_report.allow_reason_finding(ALLOW_MARKER, rel, lineno, line)
        for lineno, line in sorted(file_hits.items())
        if ALLOW_MARKER in line and gate_report.allow_reason_missing(ALLOW_MARKER, line)
    ]


def fallback_hits(path: str, loader: Any = None) -> dict[int, str]:
    """字节序标记文件的逐行命中：任一模式即收，归类与放行过滤与引擎命中同路。"""
    try:
        if loader is not None:
            text = loader.read_bytes(path).decode("utf-8", "replace")
        else:
            with open(path, "rb") as fh:
                text = fh.read().decode("utf-8", "replace")
    except OSError:
        return {}
    all_patterns = CASE_SENSITIVE_PATTERNS + CASE_INSENSITIVE_PATTERNS
    hits = {}
    for lineno, line in enumerate(text.split("\n"), 1):
        if any(pattern.search(line) for pattern in all_patterns):
            hits[lineno] = line
    return hits


def scan_hits(root: str, loader: Any = None) -> tuple[list[str], dict[str, dict[int, str]]]:
    """单次扫描的可复用结果：可扫清单序与逐文件命中映射，两种报告形态共用。"""
    files, fallback = scannable_files(root, loader)
    hits_by_file = collect_hits(root, [rel for rel in files if rel not in fallback])
    for rel in fallback:
        line_hits = fallback_hits(os.path.join(root, rel), loader)
        if line_hits:
            hits_by_file[rel] = line_hits
    return files, hits_by_file


def run(root: str, loader: Any = None) -> list[str]:
    """扫描 root 下全部 git 跟踪文件，返回违规条目（每条含文件与命中行）；
    装入器在场时清单与内容走共享缓存，缺省自行读取。"""
    files, hits_by_file = scan_hits(root, loader)
    problems = []
    for rel in files:
        file_hits = hits_by_file.get(rel)
        if not file_hits:
            continue
        for kind, patterns in (
            ("suspected hardcoded secret", CASE_SENSITIVE_PATTERNS),
            ("suspected hardcoded credential", CASE_INSENSITIVE_PATTERNS),
        ):
            for pattern in patterns:
                hits = pattern_hits(pattern, file_hits)
                if hits:
                    detail = "\n".join("  %d:%s" % hit for hit in hits)
                    problems.append("%s: %s\n%s" % (kind, rel, detail))
        bare = allow_reason_findings(rel, file_hits)
        if bare:
            detail = "\n".join("  %d:%s" % (f.line, f.evidence) for f in bare)
            problems.append("allow marker without reason: %s\n%s" % (rel, detail))
    return problems


def findings(root: str, loader: Any = None) -> list[gate_report.Finding]:
    """结构化违规条目：每条带规则标识、位置、消息、完整证据行与补救，按
    文件序、规则序、行号序输出；判定面与运行函数同源同扫描。"""
    files, hits_by_file = scan_hits(root, loader)
    out = []
    for rel in files:
        file_hits = hits_by_file.get(rel)
        if not file_hits:
            continue
        for rule, message, patterns in RULE_GROUPS:
            lines = {}
            for pattern in patterns:
                for lineno, line in pattern_hits(pattern, file_hits):
                    lines[lineno] = line
            for lineno in sorted(lines):
                out.append(
                    gate_report.Finding(
                        rule=rule,
                        path=rel,
                        line=lineno,
                        message=message,
                        evidence=lines[lineno],
                        remedy=REMEDIY,
                    )
                )
        out += allow_reason_findings(rel, file_hits)
    return out


def usage_failure(detail: str) -> int:
    print("usage: check_secrets.py [repo-root]", file=sys.stderr)
    print("  %s" % detail, file=sys.stderr)
    return gate_report.EXIT_USAGE


def main(loader: Any = None) -> int:
    if len(sys.argv) > 2:
        return usage_failure("too many arguments: expected at most one repo-root")
    root = (
        sys.argv[1]
        if len(sys.argv) > 1
        else os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    )
    if len(sys.argv) > 1 and not os.path.isdir(root):
        return usage_failure("repo-root is not a directory: %s" % root)
    try:
        found = findings(root, loader)
    except RuntimeError as exc:
        gate_report.fail(gate_report.infra_failure("cannot complete the secrets scan: %s" % exc))
        return gate_report.EXIT_INFRA
    for finding in found:
        gate_report.fail(finding)
    if found:
        gate_report.summary(found)
        return gate_report.EXIT_VIOLATION
    print("ok: secrets scan passed (no hardcoded secrets/credentials)")
    return gate_report.EXIT_PASS


if __name__ == "__main__":
    sys.exit(main())
