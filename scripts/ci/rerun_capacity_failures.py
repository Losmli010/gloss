#!/usr/bin/env python3
"""容量性失败自动复跑：识别托管运行器容量不足造成的假失败并复跑失败作业。

背景：托管运行器（macOS arm64）容量紧张时，作业取不到运行器被标记
取消，整个运行记为失败：这不是代码回归，不应留在门禁结果里。判定规
则：只处理失败与取消的作业，逐一读取检查运行注解，注解命中脚本内常
量登记的容量关键词才算容量性失败；全部失败作业都是容量性才复跑，混
入真实失败时保持红灯不动，不掩盖回归；只复跑第一次尝试，天然防循环，
本工作流不在被触发名单里，工作流层面也不会自触发。

用法：python3 scripts/ci/rerun_capacity_failures.py <run_id> --dry-run，加注只打印决策不实际复跑；仓库与令牌经托管平台的环境变量注入。
"""

from __future__ import annotations

import json
import os
import subprocess
import sys

CAPACITY_MARKERS = (
    "not acquired by Runner",
    "capacity constraints",
)
RETRYABLE_CONCLUSIONS = frozenset({"failure", "cancelled"})


def gh_api(endpoint: str):
    out = subprocess.run(
        ["gh", "api", endpoint, "--paginate"],
        capture_output=True,
        text=True,
        check=True,
    )
    return json.loads(out.stdout)


def job_annotations(repo: str, job_id: int) -> list[str]:
    endpoint = "repos/%s/check-runs/%d/annotations" % (repo, job_id)
    annotations = gh_api(endpoint)
    return [item.get("message", "") for item in annotations]


def is_capacity_caused(repo: str, job: dict) -> bool:
    messages = job_annotations(repo, job["id"])
    return any(marker in message for message in messages for marker in CAPACITY_MARKERS)


def rerun_failed_jobs(run_id: str, dry_run: bool) -> bool:
    repo = os.environ["GITHUB_REPOSITORY"]
    jobs = gh_api("repos/%s/actions/runs/%s/jobs" % (repo, run_id))["jobs"]
    failed = [job for job in jobs if job["conclusion"] in RETRYABLE_CONCLUSIONS]
    if not failed:
        print("ok: no failed or cancelled jobs")
        return True
    capacity_caused = {job["id"]: is_capacity_caused(repo, job) for job in failed}
    if not all(capacity_caused.values()):
        unresolved = [job["name"] for job in failed if not capacity_caused[job["id"]]]
        print(
            "skip: non-capacity failures present, keeping the run red: %s" % ", ".join(unresolved)
        )
        return True
    attempts = {job["id"]: job["run_attempt"] for job in failed}
    if any(attempt != 1 for attempt in attempts.values()):
        print("skip: failed jobs are past the first attempt, no further rerun")
        return True
    if dry_run:
        print("dry-run: would rerun failed jobs of run %s" % run_id)
        return True
    subprocess.run(
        ["gh", "run", "rerun", run_id, "--failed"],
        check=True,
    )
    print("ok: rerun requested for run %s" % run_id)
    return True


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: rerun_capacity_failures.py <run_id> [--dry-run]", file=sys.stderr)
        return 2
    run_id = sys.argv[1]
    dry_run = "--dry-run" in sys.argv[2:]
    return 0 if rerun_failed_jobs(run_id, dry_run) else 1


if __name__ == "__main__":
    sys.exit(main())
