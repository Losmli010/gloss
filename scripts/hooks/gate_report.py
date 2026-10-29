"""门禁统一诊断出口：违规条目的数据结构、即时报告、末尾汇总、失败收尾与
版本库调用包装，供各门禁共用。报告全部走标准错误，消息语言保持英文；退出
码语义全脚本一致：零为通过、一为发现违规、二为用法错误、三为无法完成判定
（失败关闭，绝不因没检查到而放行）。横幅与失败收尾（空行、汇总、指引行）
也在此单点收口，各门禁不再各拼一份。警告通道承载软约束候选清单：条目带软
约束标记（advisory），走警告前缀即时打印与警告汇总，不计违规计数与退出码。
规则标识常量与登记清单在本模块或各脚本常量区同源定义，各标识的含义、判罚
口径与豁免方式住登记表记录文件。
"""

from __future__ import annotations

import re
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

ALLOW_REASON_REMEDY = "add a non-empty reason right after the marker on the same line"

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
        "BDD-DUP-QUALIFIER",
        "BDD-INSTANCE",
        "BDD-BINDING",
        "I18N-LITERAL",
        "I18N-LOADPOINT",
        "I18N-CATALOG",
        "I18N-LANGUAGE",
        "I18N-REGISTRY",
        "COMMIT-EMPTY",
        "COMMIT-FORMAT",
        "COMMIT-SUBJECT",
        "COMMIT-LENGTH",
        "COMMIT-BLANK-LINE",
        "COMMIT-PROSE",
        "PR-SECTION",
        "PR-PROSE",
        "PR-TITLE-SIM",
        "REVIEW-SECTION",
        "REVIEW-DECL",
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
        "COMMENT-DOC-CODE-LANGUAGE",
        "COMMENT-DOC-CODE-CODEREF",
        "COMMENT-DOC-CODE-ENGLISH",
        "COMMENT-RECORD-NAME",
        "COMMENT-RECORD-STALE",
        "COMMENT-RECORD-COLLISION",
        "COMMENT-JUST-ONE-LINE",
        "PROSE-FOREIGN",
        "PROSE-GLOSS-MIXED",
        "PROSE-GLOSS-UNPAIRED",
        "PROSE-PANGU",
        "PROSE-TRADITIONAL",
        "PROSE-ANCHOR",
        "PROSE-CODEREF",
        "PROSE-SECTION-REF",
        "PROSE-LANGUAGE",
        "PROSE-ENGLISH",
        "ALLOW-REASON",
        "EMPTY-SURFACE",
        "INFRA",
        "UI-STYLE-COLOR",
        "UI-STYLE-DARK",
        "UI-STYLE-FONT",
        "UI-STYLE-CONST",
    }
)


@dataclass(frozen=True)
class Finding:
    """单条违规的完整描述：规则标识、位置、消息、证据行与补救动作；软约束
    候选置 advisory，走警告通道不计退出码。"""

    rule: str
    path: str
    line: int
    message: str
    evidence: str = ""
    remedy: str = ""
    column: int = 0
    advisory: bool = False


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
    head = "error[%s]: %s" % (finding.rule, finding.message)
    loc = location(finding)
    if loc:
        head = "%s (%s)" % (head, loc)
    print(head, file=out)
    if finding.evidence:
        print("    evidence: %s" % finding.evidence, file=out)
    if finding.remedy:
        print("    help: %s" % finding.remedy, file=out)
    return finding


def warn(finding: Finding, file: TextIO | None = None) -> Finding:
    """即时打印单条软约束候选到标准错误：与 fail 同模板，前缀换警告——候选
    清单供人工核查，不计违规。"""
    out = file if file is not None else sys.stderr
    head = "warning[%s]: %s" % (finding.rule, finding.message)
    loc = location(finding)
    if loc:
        head = "%s (%s)" % (head, loc)
    print(head, file=out)
    if finding.evidence:
        print("    evidence: %s" % finding.evidence, file=out)
    if finding.remedy:
        print("    help: %s" % finding.remedy, file=out)
    return finding


def warning_summary(advisories: list[Finding], file: TextIO | None = None) -> None:
    """警告汇总行：计数必须等于已打印警告明细数；候选不计违规，汇总行如实
    标注软约束属性。"""
    out = file if file is not None else sys.stderr
    count = len(advisories)
    unit = "advisory candidate" if count == 1 else "advisory candidates"
    print("warning: %d %s found (advisory, not counted as violations)" % (count, unit), file=out)


def banner(label: str, scope: str, file: TextIO | None = None) -> None:
    """门禁横幅统一出口：标签加范围打到标准输出，标记门禁段的起点；即时冲
    刷——标准输出管道下块缓冲，与标准错误合流重定向时横幅会错位到明细之后。"""
    out = file if file is not None else sys.stdout
    print("== %s (%s) ==" % (label, scope), file=out, flush=True)


def summary(findings: list[Finding], file: TextIO | None = None) -> None:
    """末尾汇总行：计数必须等于已打印明细数。"""
    out = file if file is not None else sys.stderr
    count = len(findings)
    unit = "violation" if count == 1 else "violations"
    print("error: %d %s found (see above)" % (count, unit), file=out)


def finish(findings: list[Finding], *footer: str, file: TextIO | None = None) -> None:
    """失败收尾统一出口：空行分隔、计数汇总与收尾指引行，全部走标准错误。"""
    out = file if file is not None else sys.stderr
    print("", file=out)
    summary(findings, file=out)
    for line in footer:
        print(line, file=out)


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


def allow_reason_missing(marker: str, text: str) -> bool:
    """放行标记缺缘由的判定：标记之后未紧跟空白与非空白字符即算缺失。"""
    return re.search(re.escape(marker) + r"\s+\S", text) is None


def allow_reason_finding(marker: str, path: str, line: int, evidence: str) -> Finding:
    """放行标记缺缘由的违规条目：位置与证据由各门禁的豁免现场给定。"""
    return Finding(
        rule=RULE_ALLOW_REASON,
        path=path,
        line=line,
        message="%s marker without a reason" % marker,
        evidence=evidence,
        remedy=ALLOW_REASON_REMEDY,
    )


ALLOW_REASON_MARKERS = (
    "constraints:allow-context",
    "secrets:allow",
    "lines:allow",
    "i18n:allow",
    "deps:allow",
    "style:allow",
)


def allow_reason_findings_for_line(path: str, line: int, text: str) -> list[Finding]:
    """一行文本里全部缺缘由放行标记的条目：登记表内标记全名逐个核对同行
    缘由，供标记类别不定与全名形态（界面边界上下文标记）不误报的消费方使用。"""
    return [
        allow_reason_finding(marker, path, line, text)
        for marker in ALLOW_REASON_MARKERS
        if marker in text and allow_reason_missing(marker, text)
    ]


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
