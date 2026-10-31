#!/usr/bin/env python3
"""拉取请求第一版内容快照门禁：把标题加正文的摘要以提交状态挂上头提交，供
编辑事件比对基准，第一版标题与正文不可被静默改写。快照载体是托管平台的提
交状态，上下文名固定，描述字段存摘要十六进制；写入在拉取请求的开启、同步
与重开事件幂等重写——新推送换头提交后快照必须跟随，否则后续编辑比对不到
基准，正文不随提交变化时重写哈希不变；比对在编辑事件运行，操作者为机器人
的编辑由工作流条件排除，其余编辑的当前摘要与快照不一致即判罚快照规则，修
复方式两种——恢复第一版内容，或在正文首行放放行标记加缘由；有效豁免在场
时跳过判罚并以当前版为基准重写快照，否则豁免一次后每次编辑都红灯。豁免解
析、摘要配方与提交状态读写单点收口在本脚本，工作流两步骤只传事件参数，判
罚口径不落第二处。

摘要配方：对标题的（UTF-8）字节、一个换行分隔符与正文的（UTF-8）字节取
（SHA-256）摘要；正文空值在取数侧折算为空串并入配方。快照缺席或描述字段不
可解析按无法完成判定失败关闭，绝不因没检查到而放行。

校验结果以带规则标识的违规条目列表承载，报告经统一诊断出口打印；退出码
按全脚本统一约定：零为通过、一为发现违规、二为用法错误、三为无法完成判定。

用法：python3 scripts/hooks/check_pr_snapshot.py write|compare <pull-number> <head-sha>，写入与比对两模式；仓库根取本脚本位置向上回溯，仓库标识取远端（origin）地址解析。
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import subprocess
import sys
from collections.abc import Callable

import check_required_checks
import gate_report

CONTEXT = "Content snapshot"
DIGEST_PREFIX = "sha256:"
EXEMPT_MARKER = "snapshot:allow"
RULE_PR_SNAPSHOT = "PR-SNAPSHOT"

SNAPSHOT_REMEDY = (
    "restore the original title and body of the first version, or put a"
    " %s marker with a reason on the first line of the pull request body" % EXEMPT_MARKER
)

# 豁免标记锚定正文首行起始（可选反引号包裹加词边界）：首行中途提及标记不
# 开豁免通道，防无意引用静默重写基线；反引号包裹与裸形态都认——裸形态过
# 不了书写规则（外文字母进中文主体），认它只为让判罚清单指向正确的修复动作
_EXEMPT_RE = re.compile(r"^`?snapshot:allow`?\b")
_DIGEST_RE = re.compile(r"[0-9a-f]{64}")


def content_digest(title: str, body: str) -> str:
    """摘要配方单点：标题与正文按（UTF-8）编码、单换行分隔后取（SHA-256）
    十六进制；写入与比对两侧共用，配方漂移即基线整体失效，不许第二处实现。"""
    return hashlib.sha256(title.encode("utf-8") + b"\n" + body.encode("utf-8")).hexdigest()


def parse_exemption(body: str) -> tuple[bool, list[gate_report.Finding]]:
    """豁免解析单点：正文首行以放行标记起头且缘由非空即有效豁免；标记起头
    而缘由缺席给缺缘由条目、豁免无效；标记不在首行起始视为无豁免、无条目。"""
    first = body.split("\n", 1)[0]
    match = _EXEMPT_RE.match(first)
    if match is None:
        return (False, [])
    tail_at = match.end()
    if first[tail_at : tail_at + 1] == "`":
        tail_at += 1
    if re.search(r"\S", first[tail_at:]):
        return (True, [])
    return (
        False,
        [gate_report.allow_reason_finding(EXEMPT_MARKER, "pull request body", 1, first)],
    )


def snapshot_finding(current: str, stored: str) -> gate_report.Finding:
    """快照不一致条目：摘要差异进证据，修复指引给恢复与豁免两条路。"""
    return gate_report.Finding(
        rule=RULE_PR_SNAPSHOT,
        path="",
        line=0,
        message="pull request title and body no longer match the snapshot on the head commit",
        evidence="current %s%s, snapshot %s%s" % (DIGEST_PREFIX, current, DIGEST_PREFIX, stored),
        remedy=SNAPSHOT_REMEDY,
    )


def compare_findings(
    current: str, body: str, fetch_stored: Callable[[], str]
) -> tuple[bool, list[gate_report.Finding]]:
    """比对决策单点，生产路径 main 经此决策不另抄一份：豁免有效返回重写标
    记与空表，调用方以当前摘要重写基线（豁免在场不需要快照，回调不触发）；
    否则经回调取快照摘要——回调报错转基础设施条目失败关闭，缺缘由条目照收，
    摘要与快照不一致再叠加快照条目。"""
    exempt, problems = parse_exemption(body)
    if exempt:
        return (True, [])
    try:
        stored = fetch_stored()
    except RuntimeError as exc:
        problems.append(gate_report.infra_failure(str(exc)))
        return (False, problems)
    if stored != current:
        problems.append(snapshot_finding(current, stored))
    return (False, problems)


def snapshot_payload(digest: str) -> dict[str, str]:
    """提交状态载荷：成功态、固定上下文名与描述字段的摘要串，描述有长度上
    限而摘要串远在其内。"""
    return {"state": "success", "context": CONTEXT, "description": DIGEST_PREFIX + digest}


def parse_pull_content(raw: str) -> tuple[str, str]:
    """拉取请求载荷取标题与正文：正文空值折算为空串并入摘要配方，形态不对
    即报错，由调用方转基础设施条目。"""
    payload = json.loads(raw)
    if not isinstance(payload, dict):
        raise RuntimeError("unexpected pull request payload shape")
    title = payload.get("title")
    body = payload.get("body")
    if not isinstance(title, str) or not (body is None or isinstance(body, str)):
        raise RuntimeError("unexpected pull request payload shape")
    return (title, body or "")


def find_snapshot_status(raw: str) -> dict | None:
    """状态清单取本门禁上下文的最新条目：接口按新到旧排列，首个同上下文条
    目即最新，重复写入的旧条目不再读取。"""
    entries = json.loads(raw)
    if not isinstance(entries, list):
        raise RuntimeError("unexpected commit statuses payload shape")
    for entry in entries:
        if isinstance(entry, dict) and entry.get("context") == CONTEXT:
            return entry
    return None


def parse_status_digest(entry: dict) -> str:
    """状态描述字段还原摘要：前缀加六十四位十六进制整形，形态不对即报错，
    绝不拿残缺描述当基线。"""
    description = entry.get("description")
    if not isinstance(description, str) or not description.startswith(DIGEST_PREFIX):
        raise RuntimeError("content snapshot status description is malformed")
    digest = description[len(DIGEST_PREFIX) :]
    if not _DIGEST_RE.fullmatch(digest):
        raise RuntimeError("content snapshot status description is malformed")
    return digest


def run_gh(args: list[str]) -> str:
    """托管平台命令行调用包装：成功返回标准输出文本，失败报错由调用方转基
    础设施条目。"""
    cmd = ["gh"] + args
    try:
        proc = subprocess.run(cmd, capture_output=True)
    except FileNotFoundError as exc:
        raise RuntimeError("gh executable not found: %s" % " ".join(cmd)) from exc
    if proc.returncode != 0:
        detail = " ".join(proc.stderr.decode("utf-8", "replace").split())
        raise RuntimeError("gh api failed (exit %d): %s" % (proc.returncode, detail))
    return proc.stdout.decode("utf-8", "replace")


def fetch_pull_content(slug: str, number: str) -> tuple[str, str]:
    """经托管平台接口取拉取请求当前的标题与正文。"""
    return parse_pull_content(run_gh(["api", "repos/%s/pulls/%s" % (slug, number)]))


def write_snapshot(slug: str, sha: str, digest: str) -> None:
    """把摘要写上头提交：同上下文重复写入即幂等重写，最新条目按清单序胜出。"""
    payload = snapshot_payload(digest)
    run_gh(
        [
            "api",
            "--method",
            "POST",
            "repos/%s/statuses/%s" % (slug, sha),
            "-f",
            "state=%s" % payload["state"],
            "-f",
            "context=%s" % payload["context"],
            "-f",
            "description=%s" % payload["description"],
        ]
    )


def fetch_snapshot_digest(slug: str, sha: str) -> str:
    """读头提交上的快照摘要：条目缺席按失败关闭报错，修复指引指向写入事件。"""
    raw = run_gh(["api", "repos/%s/commits/%s/statuses" % (slug, sha)])
    entry = find_snapshot_status(raw)
    if entry is None:
        raise RuntimeError(
            "no %s status on the head commit; the write step runs on the opened,"
            " synchronize and reopened events of the pull request" % CONTEXT
        )
    return parse_status_digest(entry)


def infra_exit(exc: RuntimeError) -> int:
    """基础设施条目的失败收尾：即时打印后按无法完成判定退出。"""
    gate_report.fail(gate_report.infra_failure(str(exc)))
    return gate_report.EXIT_INFRA


def main() -> int:
    root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    if len(sys.argv) != 4 or sys.argv[1] not in ("write", "compare"):
        print("usage: %s write|compare <pull-number> <head-sha>" % sys.argv[0], file=sys.stderr)
        return gate_report.EXIT_USAGE
    mode, number, sha = sys.argv[1], sys.argv[2], sys.argv[3]
    try:
        slug = check_required_checks.repo_slug(root)
    except RuntimeError as exc:
        return infra_exit(exc)
    gate_report.banner(
        "content snapshot gate",
        "repo: %s, pull request: #%s, mode: %s" % (slug, number, mode),
    )
    try:
        title, body = fetch_pull_content(slug, number)
    except RuntimeError as exc:
        return infra_exit(exc)
    current = content_digest(title, body)
    if mode == "write":
        try:
            write_snapshot(slug, sha, current)
        except RuntimeError as exc:
            return infra_exit(exc)
        print("  ok: content snapshot written (%s%s)" % (DIGEST_PREFIX, current))
        return gate_report.EXIT_PASS
    rewrite, problems = compare_findings(current, body, lambda: fetch_snapshot_digest(slug, sha))
    if rewrite:
        try:
            write_snapshot(slug, sha, current)
        except RuntimeError as exc:
            return infra_exit(exc)
        print(
            "  ok: %s on the first body line, snapshot baseline rewritten to the"
            " current version (%s%s)" % (EXEMPT_MARKER, DIGEST_PREFIX, current)
        )
        return gate_report.EXIT_PASS
    for finding in problems:
        gate_report.fail(finding)
    if problems:
        gate_report.finish(problems)
        if any(f.rule == gate_report.RULE_INFRA for f in problems):
            return gate_report.EXIT_INFRA
        return gate_report.EXIT_VIOLATION
    print("  ok: title and body match the snapshot on the head commit")
    return gate_report.EXIT_PASS


if __name__ == "__main__":
    sys.exit(main())
