#!/usr/bin/env python3
"""必需检查对账门禁：工作流在拉取请求（PR）链路上会运行的作业，逐一核对
主分支保护的必需检查清单，应然作业名缺失即违规。对账三步：解析工作流目录
下全部文件，只收集检查运行挂在拉取请求头提交、红灯能挡合并的三类链路事件
（pull_request、pull_request_target、pull_request_review）下会运行的作业，
作业条件把事件排除在外的不算；经托管平台接口取必需检查清单；差集判罚。
收集口径宁误报不漏报：条件表达式的未知操作数与解析失败都按会运行处置，
静态不可对账的作业名（矩阵展开）只走显式豁免登记，豁免与判罚之外没有第三
条静默路径。工作流文件解析失败按无法完成判定失败关闭，绝不因没检查到而放
行。必需检查的补设是托管平台的配置动作，由管理员完成，不进代码变更。

校验结果以带规则标识的违规条目列表承载，报告经统一诊断出口打印；退出码
按全脚本统一约定：零为通过、一为发现违规、二为用法错误、三为无法完成判定。

用法：python3 scripts/hooks/check_required_checks.py，无参数；仓库根取本脚本位置向上回溯。
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from dataclasses import dataclass

import gate_report

WORKFLOWS_REL = os.path.join(".github", "workflows")
PROTECTED_BRANCH = "main"

# 拉取请求（PR）链路事件全集：检查运行挂在拉取请求头提交、红灯能挡合并的
# 事件；定时与手动触发等不挂拉取请求的事件不进对账
PR_EVENTS = ("pull_request", "pull_request_target", "pull_request_review")

RULE_REQUIRED_CHECKS_MISSING = "REQUIRED-CHECKS-MISSING"

MISSING_REMEDY = (
    "a repository admin must add the check to the required status checks of the"
    " %s branch protection on the hosting platform (configuration change, not a"
    " code change)" % PROTECTED_BRANCH
)

# 豁免登记：作业显示名到缘由。缺省应进闸门，豁免必须显式登记、宁误报不漏
# 报；登记之外没有免检路径，新增豁免在本常量区落名并给出可核查的缘由
EXEMPTED_JOBS = {
    "Build (${{ matrix.target }})": (
        "matrix expansion produces one concrete check name per target; the"
        " expanded name (Build (aarch64-apple-darwin)) is verified manually"
        " against the required list"
    ),
}


@dataclass(frozen=True)
class ExpectedJob:
    """对账收集到的应然作业：检查名（显示名或作业 id）、所在工作流、作业
    id、声明行号、条件原文与会运行的 PR 链路事件。"""

    check: str
    workflow: str
    job_id: str
    line: int
    condition: str
    events: tuple[str, ...]


class WorkflowFormatError(RuntimeError):
    """工作流文件超出解析器认得的形态：失败关闭，由调用方按基础设施条目处置。"""


class ConditionError(ValueError):
    """条件表达式超出求值器认得的形态：调用方按未知处置（宁误报不漏报）。"""


# ---- 工作流子集解析 ----

_LINE_RE = re.compile(r"^([\w.-]+):(?:[ \t]+(.*))?$")
_BLOCK_SCALAR_RE = re.compile(r"[|>][0-9+-]*$")
# 事件名形态：内联标量与流式清单元素的准入校验，子集外形态红出来
_EVENT_NAME_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")


def _strip_comment(line: str) -> str:
    """引号感知去行尾注释：井号在引号外且行首或空白之后才算注释。"""
    in_str = None
    j, n = 0, len(line)
    while j < n:
        c = line[j]
        if in_str:
            if c == "\\" and in_str == '"':
                j += 2
                continue
            if c == in_str:
                in_str = None
            j += 1
            continue
        if c in "\"'":
            in_str = c
            j += 1
            continue
        if c == "#" and (j == 0 or line[j - 1] in " \t"):
            return line[:j]
        j += 1
    return line


def _significant(text: str) -> list[tuple[int, str, int]]:
    """去注释与空行后的实质行：缩进、内容与行号三元组，缩进按空格计。"""
    out = []
    for lineno, raw in enumerate(text.split("\n"), 1):
        line = _strip_comment(raw).rstrip()
        if not line.strip():
            continue
        out.append((len(line) - len(line.lstrip(" ")), line.strip(), lineno))
    return out


def _unquote(text: str) -> str:
    """整体成对的引号剥掉，其余原样保留（矩阵展开的显示名保持字面）。"""
    if len(text) >= 2 and text[0] == text[-1] and text[0] in "\"'":
        return text[1:-1]
    return text


def _flow_list(rel: str, text: str) -> list[str]:
    """流式清单解析：[a, b] 形态按逗号切分，元素须是事件名形态，子集之外
    的元素（如流式映射）按解析失败处置。"""
    inner = text[1:-1].strip()
    if not inner:
        return []
    names = [_unquote(part.strip()) for part in inner.split(",")]
    for name in names:
        if not _EVENT_NAME_RE.fullmatch(name):
            raise WorkflowFormatError("%s: unsupported trigger element (%s)" % (rel, name))
    return names


def _parse_events(rel: str, lines: list[tuple[int, str, int]]) -> tuple[str, ...]:
    """触发事件名清单：顶层 on 键的子键或流式清单，子键之下（branches 等）
    的结构不进结果；内联标量须是事件名形态，流式映射等子集之外的形态按解
    析失败处置，绝不猜。"""
    at = None
    for i, (indent, content, _) in enumerate(lines):
        if indent == 0:
            m = _LINE_RE.match(content)
            if m and m.group(1) == "on":
                at = (i, (m.group(2) or "").strip())
                break
    if at is None:
        raise WorkflowFormatError("%s: missing the on: trigger map" % rel)
    i, rest = at
    if rest:
        if rest.startswith("[") and rest.endswith("]"):
            return tuple(_flow_list(rel, rest))
        scalar = _unquote(rest)
        if not _EVENT_NAME_RE.fullmatch(scalar):
            raise WorkflowFormatError("%s: unsupported inline trigger form (%s)" % (rel, rest))
        return (scalar,)
    events = []
    child = None
    for indent, content, _ in lines[i + 1 :]:
        if indent == 0:
            break
        if child is None:
            child = indent
        if indent > child:
            continue
        if indent < child:
            raise WorkflowFormatError("%s: inconsistent indentation inside on:" % rel)
        m = _LINE_RE.match(content)
        if not m:
            raise WorkflowFormatError("%s: unexpected line inside on: (%s)" % (rel, content))
        if m.group(1) not in events:
            events.append(m.group(1))
    return tuple(events)


def _parse_jobs(rel: str, lines: list[tuple[int, str, int]]) -> dict[str, tuple[str, str, int]]:
    """作业清单：作业 id 到（检查名、条件原文、声明行号）；检查名取作业级
    name 键（缺省作业 id），条件取作业级 if 键，折行块标量按空格接回一行；
    steps 与 strategy 等其余子树整段跳过。"""
    at = None
    for i, (indent, content, _) in enumerate(lines):
        if indent == 0:
            m = _LINE_RE.match(content)
            if m and m.group(1) == "jobs":
                at = i
                break
    if at is None:
        raise WorkflowFormatError("%s: missing the jobs: map" % rel)
    if at + 1 >= len(lines):
        raise WorkflowFormatError("%s: jobs: map is empty" % rel)
    child = lines[at + 1][0]
    if child == 0:
        raise WorkflowFormatError("%s: jobs: map is empty" % rel)
    jobs: dict[str, tuple[str, str, int]] = {}
    i = at + 1
    while i < len(lines):
        indent, content, lineno = lines[i]
        if indent < child:
            break
        if indent > child:
            raise WorkflowFormatError("%s: unexpected nesting at job level" % rel)
        m = _LINE_RE.match(content)
        if not m:
            raise WorkflowFormatError("%s: unexpected job entry (%s)" % (rel, content))
        job_id = m.group(1)
        body = []
        j = i + 1
        while j < len(lines) and lines[j][0] > child:
            body.append(lines[j])
            j += 1
        jobs[job_id] = _job_fields(rel, job_id, body, lineno)
        i = j
    return jobs


def _job_fields(
    rel: str, job_id: str, body: list[tuple[int, str, int]], job_line: int
) -> tuple[str, str, int]:
    """作业体的直接子键扫描：只认 name 与 if，更深缩进的子树逐行跳过；行号
    恒取作业声明行，位置锚定作业本身。"""
    check = job_id
    condition = ""
    line = job_line
    if not body:
        return (check, condition, line)
    direct = body[0][0]
    i = 0
    while i < len(body):
        indent, content, lineno = body[i]
        if indent < direct:
            raise WorkflowFormatError("%s: inconsistent indentation in job %s" % (rel, job_id))
        if indent > direct:
            i += 1
            continue
        m = _LINE_RE.match(content)
        if not m:
            raise WorkflowFormatError("%s: unexpected line in job %s (%s)" % (rel, job_id, content))
        key, rest = m.group(1), (m.group(2) or "").strip()
        if key in ("name", "if"):
            value, i = _scalar_value(rel, job_id, rest, body, i, direct)
            if key == "name":
                check = value or job_id
            else:
                condition = value
            continue
        i += 1
    return (check, condition, line)


def _scalar_value(
    rel: str, job_id: str, rest: str, body: list[tuple[int, str, int]], i: int, direct: int
) -> tuple[str, int]:
    """作业级标量取值：折行块标量把更深层行按空格接回一行，其余按内联标量。"""
    if rest and _BLOCK_SCALAR_RE.fullmatch(rest):
        parts = []
        j = i + 1
        while j < len(body) and body[j][0] > direct:
            parts.append(body[j][1])
            j += 1
        return (" ".join(parts), j)
    if not rest:
        raise WorkflowFormatError("%s: empty value in job %s" % (rel, job_id))
    return (_unquote(rest), i + 1)


def parse_workflow(rel: str, text: str) -> tuple[tuple[str, ...], dict[str, tuple[str, str, int]]]:
    """工作流文件解析入口：触发事件名清单加作业清单，子集之外的形态一律
    解析失败，绝不猜。"""
    lines = _significant(text)
    return (_parse_events(rel, lines), _parse_jobs(rel, lines))


# ---- 条件表达式求值（三值逻辑）----

_UNKNOWN = object()

_TOKEN_RE = re.compile(r"'[^']*'|\"[^\"]*\"|==|!=|&&|\|\||!|[()]|,|[A-Za-z_][A-Za-z0-9_.-]*")
# 已知函数：双参且两侧可静态取值才出真假，其余函数一律未知
_KNOWN_FUNCTIONS = ("endsWith", "startsWith", "contains")


def _and3(left, right):
    if left is False or right is False:
        return False
    if left is True and right is True:
        return True
    return _UNKNOWN


def _or3(left, right):
    if left is True or right is True:
        return True
    if left is False and right is False:
        return False
    return _UNKNOWN


def _not3(value):
    if value is True:
        return False
    if value is False:
        return True
    return _UNKNOWN


class _Parser:
    """条件表达式递归下降求值：github.event_name 取合成事件名，其余上下文
    路径静态不可知；语法超出子集即 ConditionError。"""

    def __init__(self, tokens: list[str], event_name: str) -> None:
        self.tokens = tokens
        self.pos = 0
        self.event = event_name

    def peek(self) -> str | None:
        return self.tokens[self.pos] if self.pos < len(self.tokens) else None

    def take(self) -> str:
        token = self.peek()
        if token is None:
            raise ConditionError("unexpected end of expression")
        self.pos += 1
        return token

    def expect(self, token: str) -> None:
        if self.take() != token:
            raise ConditionError("expected %s" % token)

    def parse(self):
        value = self.or_expr()
        if self.peek() is not None:
            raise ConditionError("trailing tokens")
        return value

    def or_expr(self):
        value = self.and_expr()
        while self.peek() == "||":
            self.take()
            value = _or3(value, self.and_expr())
        return value

    def and_expr(self):
        value = self.unary()
        while self.peek() == "&&":
            self.take()
            value = _and3(value, self.unary())
        return value

    def unary(self):
        if self.peek() == "!":
            self.take()
            return _not3(self.unary())
        return self.comparison()

    def comparison(self):
        left = self.primary()
        if self.peek() in ("==", "!="):
            op = self.take()
            right = self.primary()
            if left is _UNKNOWN or right is _UNKNOWN:
                return _UNKNOWN
            return (left == right) if op == "==" else (left != right)
        return left

    def primary(self):
        token = self.take()
        if token == "(":
            value = self.or_expr()
            self.expect(")")
            return value
        if token[0] in "\"'":
            return token[1:-1]
        if token == "true":
            return True
        if token == "false":
            return False
        if self.peek() == "(":
            return self.call(token)
        if token == "github.event_name":
            return self.event
        return _UNKNOWN

    def call(self, name: str):
        self.take()
        args = []
        if self.peek() != ")":
            args.append(self.or_expr())
            while self.peek() == ",":
                self.take()
                args.append(self.or_expr())
        self.expect(")")
        if name in _KNOWN_FUNCTIONS and len(args) == 2:
            first, second = args
            if (
                _UNKNOWN in (first, second)
                or not isinstance(first, str)
                or not isinstance(second, str)
            ):
                return _UNKNOWN
            if name == "endsWith":
                return first.endswith(second)
            if name == "startsWith":
                return first.startswith(second)
            return second in first
        return _UNKNOWN


def evaluate_condition(expr: str, event_name: str):
    """条件求值：真假或未知；未知操作数与解析失败都返回未知，由调用方按
    会运行处置（宁误报不漏报）。"""
    tokens = []
    i = 0
    while i < len(expr):
        if expr[i].isspace():
            i += 1
            continue
        m = _TOKEN_RE.match(expr, i)
        if not m:
            return _UNKNOWN
        tokens.append(m.group(0))
        i = m.end()
    try:
        return _Parser(tokens, event_name).parse()
    except ConditionError:
        return _UNKNOWN


def runs_on(condition: str, event_name: str) -> bool:
    """作业在某事件下是否运行：无条件即运行；求值为假才排除，未知按运行
    处置（宁误报不漏报）。"""
    if not condition:
        return True
    return evaluate_condition(condition, event_name) is not False


# ---- 对账 ----


def collect_expected_jobs(
    workflows_dir: str,
) -> tuple[list[ExpectedJob], list[gate_report.Finding]]:
    """应然作业收集：逐个解析工作流目录文件，作业条件不排除任一所声明的
    PR 链路事件即收集；解析失败返回基础设施条目，失败关闭。"""
    if not os.path.isdir(workflows_dir):
        return (
            [],
            [gate_report.infra_failure("workflows directory is missing: %s" % workflows_dir)],
        )
    names = sorted(name for name in os.listdir(workflows_dir) if name.endswith((".yml", ".yaml")))
    jobs: list[ExpectedJob] = []
    infra: list[gate_report.Finding] = []
    for name in names:
        rel = os.path.join(WORKFLOWS_REL, name)
        try:
            with open(os.path.join(workflows_dir, name), encoding="utf-8", errors="replace") as fh:
                events, parsed = parse_workflow(rel, fh.read())
        except WorkflowFormatError as exc:
            infra.append(
                gate_report.infra_failure(
                    "workflow file cannot be parsed: %s" % rel, evidence=str(exc)
                )
            )
            continue
        declared = [event for event in PR_EVENTS if event in events]
        if not declared:
            continue
        for job_id, (check, condition, line) in parsed.items():
            run_events = tuple(event for event in declared if runs_on(condition, event))
            if run_events:
                jobs.append(ExpectedJob(check, rel, job_id, line, condition, run_events))
    return (jobs, infra)


def missing_required(jobs: list[ExpectedJob], required: list[str]) -> list[ExpectedJob]:
    """差集判罚：应然检查名不在必需清单且未豁免即缺，同名去重后按检查名
    稳定排序。"""
    present = set(required)
    out: list[ExpectedJob] = []
    seen: set[str] = set()
    for job in sorted(jobs, key=lambda item: (item.check, item.workflow, item.line)):
        if job.check in present or job.check in EXEMPTED_JOBS or job.check in seen:
            continue
        seen.add(job.check)
        out.append(job)
    return out


def exemption_notes(jobs: list[ExpectedJob]) -> list[str]:
    """豁免登记的报告行：登记项与收集面一一对照，登记了却收集不到的条目
    同样报告（登记陈旧提醒）。"""
    collected = {job.check for job in jobs}
    return [
        'note: job "%s" is exempted from reconciliation (%s)' % (name, EXEMPTED_JOBS[name])
        if name in collected
        else 'note: exemption entry "%s" matches no collected pull request job' % name
        for name in EXEMPTED_JOBS
    ]


def parse_remote_url(url: str) -> str:
    """远端地址到仓库标识 owner/name：https 与 ssh 两种形态，其余报错。"""
    m = re.search(r"[:/]([^/:]+)/([^/]+?)(?:\.git)?/?$", url.strip())
    if not m:
        raise RuntimeError("cannot parse the origin remote as owner/name: %s" % url)
    return "%s/%s" % (m.group(1), m.group(2))


def repo_slug(root: str) -> str:
    """仓库标识：origin 远端地址解析，版本库调用失败走统一出口退出码三。"""
    proc = gate_report.run_git(["remote", "get-url", "origin"], cwd=root)
    return parse_remote_url(proc.stdout.decode("utf-8", "replace"))


def parse_protection_payload(raw: str) -> list[str]:
    """必需 contexts 解析：取 required_status_checks.contexts，形态不对即
    报错，由调用方转基础设施条目。"""
    payload = json.loads(raw)
    checks = payload.get("required_status_checks") if isinstance(payload, dict) else None
    contexts = checks.get("contexts") if isinstance(checks, dict) else None
    if not isinstance(contexts, list) or not all(isinstance(c, str) for c in contexts):
        raise RuntimeError("unexpected branch protection payload shape")
    return list(contexts)


def fetch_required_contexts(slug: str) -> list[str]:
    """经托管平台接口取主分支保护的必需 contexts；gh 失败或载荷形态不对
    都报错，权限不足的修复指引随消息给出。"""
    cmd = ["gh", "api", "repos/%s/branches/%s/protection" % (slug, PROTECTED_BRANCH)]
    try:
        proc = subprocess.run(cmd, capture_output=True)
    except FileNotFoundError as exc:
        raise RuntimeError("gh executable not found: %s" % " ".join(cmd)) from exc
    if proc.returncode != 0:
        detail = " ".join(proc.stderr.decode("utf-8", "replace").split())
        raise RuntimeError(
            "gh api failed (exit %d): %s; the token needs the administration:read"
            " permission for the branch protection endpoint" % (proc.returncode, detail)
        )
    return parse_protection_payload(proc.stdout.decode("utf-8", "replace"))


def usage_failure(detail: str) -> int:
    print("usage: check_required_checks.py (no arguments)", file=sys.stderr)
    print("  %s" % detail, file=sys.stderr)
    return gate_report.EXIT_USAGE


def main() -> int:
    if len(sys.argv) > 1:
        return usage_failure("the gate always reconciles the repository it lives in")
    root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    try:
        slug = repo_slug(root)
    except RuntimeError as exc:
        gate_report.fail(gate_report.infra_failure(str(exc)))
        return gate_report.EXIT_INFRA
    gate_report.banner(
        "required checks reconcile gate",
        "repo: %s, protected branch: %s" % (slug, PROTECTED_BRANCH),
    )
    jobs, infra = collect_expected_jobs(os.path.join(root, WORKFLOWS_REL))
    if infra:
        for finding in infra:
            gate_report.fail(finding)
        gate_report.finish(infra)
        return gate_report.EXIT_INFRA
    for note in exemption_notes(jobs):
        print(note, file=sys.stderr)
    try:
        required = fetch_required_contexts(slug)
    except RuntimeError as exc:
        gate_report.fail(gate_report.infra_failure(str(exc)))
        return gate_report.EXIT_INFRA
    findings = [
        gate_report.Finding(
            rule=RULE_REQUIRED_CHECKS_MISSING,
            path=job.workflow,
            line=job.line,
            message='pull request check "%s" is not a required status check on %s'
            % (job.check, PROTECTED_BRANCH),
            evidence='job "%s" runs on %s' % (job.job_id, ", ".join(job.events)),
            remedy=MISSING_REMEDY,
        )
        for job in missing_required(jobs, required)
    ]
    for finding in findings:
        gate_report.fail(finding)
    if findings:
        gate_report.finish(findings)
        return gate_report.EXIT_VIOLATION
    print(
        "ok: required checks reconcile passed (%d pull request checks, all required)"
        % len({job.check for job in jobs})
    )
    return gate_report.EXIT_PASS


if __name__ == "__main__":
    sys.exit(main())
