"""门禁统一诊断出口：违规条目的数据结构、即时报告、末尾汇总与版本库调用包
装，供各门禁脚本共用。报告全部走标准错误，消息语言保持英文；退出码语义全
脚本一致：零为通过、一为发现违规、二为用法错误、三为无法完成判定（失败关
闭，绝不因没检查到而放行）。规则标识常量与登记清单在本模块或各脚本常量区
同源定义，各标识的含义、判罚口径与豁免方式住登记表记录文件。
"""

from __future__ import annotations

import subprocess
import sys
from dataclasses import dataclass
from typing import TextIO

EXIT_PASS = 0
EXIT_VIOLATION = 1
EXIT_USAGE = 2
EXIT_INFRA = 3

RULE_INFRA = "INFRA"
RULE_EMPTY_SURFACE = "EMPTY-SURFACE"
RULE_ALLOW_REASON = "ALLOW-REASON"

INFRA_REMEDY = "fix the reported tool or repository problem, then rerun the gate"

REGISTERED_RULES = frozenset(
    {
        "DEP-DIRECTION",
        "DEP-CORE-REDLINE",
        "DEP-UNREGISTERED",
        "EVAL-DEP",
        "EVAL-ASSET",
        "STUB-DRIFT",
        "LOG-EXIT",
        "LOG-ENGLISH",
        "EGUI-CTX",
        "FFI-CENTRAL",
        "VERSION-SINGLE",
        "DEFAULT-FEATURES",
        "DEAD-DEP",
        "LINE-CAP",
        "TASK-MARKER",
        "SECRET-VENDOR",
        "SECRET-CREDENTIAL",
        "SECRET-AUTH",
        "DOC-RECIPE",
        "DOC-PATH",
        "DOC-FILE",
        "DOC-TEST",
        "DOC-RULE-REF",
        "DOC-INPUT",
        "BDD-STALE",
        "BDD-MISSING",
        "BDD-DUP-ROW",
        "BDD-DUP-CHAPTER",
        "BDD-DUP-BLOCK",
        "BDD-TABLE",
        "I18N-LITERAL",
        "I18N-LOADPOINT",
        "I18N-CATALOG",
        "COMMIT-EMPTY",
        "COMMIT-FORMAT",
        "COMMIT-SUBJECT",
        "COMMIT-LENGTH",
        "COMMIT-BLANK-LINE",
        "COMMIT-PROSE",
        "PR-SECTION",
        "PR-PROSE",
        "PR-TITLE-SIM",
        "COMMENT-RUST-HEADER-MISSING",
        "COMMENT-RUST-HEADER-LANGUAGE",
        "COMMENT-RUST-HEADER-CODEREF",
        "COMMENT-RUST-HEADER-SECTION-REF",
        "COMMENT-RUST-HEADER-LATE",
        "COMMENT-RUST-DOC",
        "COMMENT-RUST-BLOCK",
        "COMMENT-RUST-TRAILING",
        "COMMENT-RUST-INLINE",
        "COMMENT-RUST-SAFETY-ADJACENCY",
        "COMMENT-SCRIPT-CODEREF",
        "COMMENT-SCRIPT-SECTION-REF",
        "COMMENT-SCRIPT-LANGUAGE",
        "COMMENT-SCRIPT-HEADER-CODEREF",
        "COMMENT-SCRIPT-HEADER-SECTION-REF",
        "COMMENT-SCRIPT-HEADER-LANGUAGE",
        "COMMENT-DOC-HEADING-JUMP",
        "COMMENT-DOC-HEADING-DUP",
        "COMMENT-DOC-FENCE-UNCLOSED",
        "PROSE-FOREIGN",
        "PROSE-GLOSS-MIXED",
        "PROSE-GLOSS-UNKNOWN",
        "PROSE-GLOSS-NESTED",
        "PROSE-GLOSS-UNPAIRED",
        "PROSE-CODEREF",
        "PROSE-SECTION-REF",
        "PROSE-LANGUAGE",
        "PROSE-ENGLISH",
        "ALLOW-REASON",
        "EMPTY-SURFACE",
        "INFRA",
    }
)


@dataclass(frozen=True)
class Finding:
    """单条违规的完整描述：规则标识、位置、消息、证据行与补救动作。"""

    rule: str
    path: str
    line: int
    message: str
    evidence: str = ""
    remedy: str = ""
    column: int = 0


def location(finding: Finding) -> str:
    """位置的文本形态：相对路径加行号，列号在场则追加，路径为空则无位置。"""
    if not finding.path:
        return ""
    loc = finding.path if finding.line <= 0 else "%s:%d" % (finding.path, finding.line)
    if finding.column > 0:
        loc = "%s:%d" % (loc, finding.column)
    return loc


def render(finding: Finding) -> str:
    """违规渲染行：消息加位置，作为运行函数返回清单里的条目。"""
    loc = location(finding)
    return "%s (%s)" % (finding.message, loc) if loc else finding.message


def fail(finding: Finding, file: TextIO | None = None) -> Finding:
    """即时打印单条违规到标准错误，返回同一条目供调用方汇集计数。"""
    out = file if file is not None else sys.stderr
    head = "✗ error[%s]: %s" % (finding.rule, finding.message)
    loc = location(finding)
    if loc:
        head = "%s (%s)" % (head, loc)
    print(head, file=out)
    if finding.evidence:
        print("    证据: %s" % finding.evidence, file=out)
    if finding.remedy:
        print("    help: %s" % finding.remedy, file=out)
    return finding


def summary(findings: list[Finding], file: TextIO | None = None) -> None:
    """末尾汇总行：计数必须等于已打印明细数。"""
    out = file if file is not None else sys.stderr
    count = len(findings)
    unit = "violation" if count == 1 else "violations"
    print("error: %d %s found (see above)" % (count, unit), file=out)


def infra_failure(message: str, evidence: str = "") -> Finding:
    """门禁自身故障的条目：无法完成判定，按失败关闭处置。"""
    return Finding(
        rule=RULE_INFRA,
        path="",
        line=0,
        message=message,
        evidence=evidence,
        remedy=INFRA_REMEDY,
    )


def run_git(
    args: list[str], cwd: str | None = None, file: TextIO | None = None
) -> subprocess.CompletedProcess[bytes]:
    """统一包装版本库调用：成功返回完成态子进程，失败打基础设施条目并退出。"""
    out = file if file is not None else sys.stderr
    cmd = ["git"] + list(args)
    try:
        proc = subprocess.run(cmd, cwd=cwd, capture_output=True)
    except FileNotFoundError:
        fail(infra_failure("git executable not found: git %s" % " ".join(args)), file=out)
        sys.exit(EXIT_INFRA)
    if proc.returncode != 0:
        detail = " ".join(proc.stderr.decode("utf-8", "replace").split())
        fail(
            infra_failure(
                "git command failed (exit %d): git %s" % (proc.returncode, " ".join(args)),
                evidence=detail,
            ),
            file=out,
        )
        sys.exit(EXIT_INFRA)
    return proc
