"""约束门禁·路由聚合的单测：基线夹具经聚合入口断言全部十二条通过行按模块
序齐全——少挂一个领域模块或漏一条约束即变红；违规夹具断言判罚穿透聚合、
对应的条件通过行随之缺席。
"""

import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

import constraints  # noqa: E402
from constraints import fixtures  # noqa: E402

OK_STEMS = (
    "ok dependency direction",
    "ok eval asset isolation",
    "ok unified log exit",
    "ok log messages English-only",
    "ok egui context unified load point",
    "ok FFI declarations centralized",
    "ok single version source",
    "ok default features off",
    "ok no dead dependencies",
    "ok stub copies identical",
    "ok line cap",
    "ok no leftover task markers",
)


def test_baseline_aggregate_reports_every_domain(tmp_path):
    fixtures.new_fixture(tmp_path)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    oks, fails = constraints.run(str(tmp_path))
    assert fails == []
    assert [ok.split(" (", 1)[0] for ok in oks] == list(OK_STEMS)


def test_aggregate_surfaces_domain_violations(tmp_path):
    def mutate(root):
        fixtures.add_dep(
            root / "crates/gloss-app/Cargo.toml", 'gloss-eval = { path = "../gloss-eval" }'
        )

    oks, fails = fixtures.run_case(constraints, tmp_path, mutate)
    assert any("must stay out of the production dependency graph" in f for f in fails)
    assert not any(ok.startswith("ok eval asset isolation") for ok in oks)
