"""门禁统一诊断出口：违规条目的数据结构、即时报告、末尾汇总与版本库调用包
装，供各门禁脚本共用。报告全部走标准错误，消息语言保持英文；退出码语义全
脚本一致：零为通过、一为发现违规、二为用法错误、三为无法完成判定（失败关
闭，绝不因没检查到而放行）。规则标识常量与登记清单在本模块或各脚本常量区
同源定义，各标识的含义、判罚口径与豁免方式住登记表记录文件。
"""

import subprocess
import sys
from dataclasses import dataclass

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


def location(finding):
    """位置的文本形态：相对路径加行号，列号在场则追加，路径为空则无位置。"""
    if not finding.path:
        return ""
    loc = finding.path if finding.line <= 0 else "%s:%d" % (finding.path, finding.line)
    if finding.column > 0:
        loc = "%s:%d" % (loc, finding.column)
    return loc


def fail(finding, file=sys.stderr):
    """即时打印单条违规到标准错误，返回同一条目供调用方汇集计数。"""
    head = "✗ error[%s]: %s" % (finding.rule, finding.message)
    loc = location(finding)
    if loc:
        head = "%s (%s)" % (head, loc)
    print(head, file=file)
    if finding.evidence:
        print("    证据: %s" % finding.evidence, file=file)
    if finding.remedy:
        print("    help: %s" % finding.remedy, file=file)
    return finding


def summary(findings, file=sys.stderr):
    """末尾汇总行：计数必须等于已打印明细数。"""
    count = len(findings)
    unit = "violation" if count == 1 else "violations"
    print("error: %d %s found (see above)" % (count, unit), file=file)


def infra_failure(message, evidence=""):
    """门禁自身故障的条目：无法完成判定，按失败关闭处置。"""
    return Finding(
        rule=RULE_INFRA,
        path="",
        line=0,
        message=message,
        evidence=evidence,
        remedy=INFRA_REMEDY,
    )


def run_git(args, cwd=None, file=sys.stderr):
    """统一包装版本库调用：成功返回完成态子进程，失败打基础设施条目并退出。"""
    cmd = ["git"] + list(args)
    try:
        proc = subprocess.run(cmd, cwd=cwd, capture_output=True)
    except FileNotFoundError:
        fail(infra_failure("git executable not found: git %s" % " ".join(args)), file=file)
        sys.exit(EXIT_INFRA)
    if proc.returncode != 0:
        detail = " ".join(proc.stderr.decode("utf-8", "replace").split())
        fail(
            infra_failure(
                "git command failed (exit %d): git %s" % (proc.returncode, " ".join(args)),
                evidence=detail,
            ),
            file=file,
        )
        sys.exit(EXIT_INFRA)
    return proc
