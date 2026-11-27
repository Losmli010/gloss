"""约束门禁·脚本卫生域的单测：桶间依赖与路径引导两规则在假仓夹具上正反两
向逐条覆盖——白名单过渡边按真实路径复刻放行；红牌逐类成例（反向依赖边、
未登记跨桶边、叶桶导入、同目录冗余引导、测试文件插入、多处插入、目标解析
不了、解析失败），两类定案引导与放行标记带缘由放行，另含聚合路由的领域挂
载用例。真实仓库的白名单生效由约束配方的全绿承担，不进单测。
"""

from __future__ import annotations

from pathlib import Path

import constraints
from constraints import fixtures, script_hygiene


def script_skeleton(root: Path) -> None:
    """最小脚本面：门禁基建住门禁桶（hooks）根，注释子包（comments）带包
    初始化。"""
    (root / "scripts/hooks/comments").mkdir(parents=True)
    (root / "scripts/hooks/gate_report.py").write_text("EXIT_PASS = 0\n", encoding="utf-8")
    (root / "scripts/hooks/comments/__init__.py").write_text("\n", encoding="utf-8")


def write_dev_modules(root: Path) -> None:
    """开发桶（dev）的两单点库（过渡位置）：版本单点与命名映射。"""
    (root / "scripts/dev").mkdir(parents=True)
    (root / "scripts/dev/tool_version.py").write_text('VERSION = "0"\n', encoding="utf-8")
    (root / "scripts/dev/comment_map.py").write_text("MAP = {}\n", encoding="utf-8")


def write_transition_edges(root: Path) -> None:
    """白名单四份存量文件按真实路径复刻：三处跨桶引导与两条跨桶依赖分住三
    份生产脚本，一处测试函数体内插入，形态对齐仓库现状。"""
    (root / "scripts/hooks/check_duplication.py").write_text(
        "import os\nimport sys\n\n"
        "_HOOKS = os.path.dirname(os.path.abspath(__file__))\n"
        '_DEV = os.path.abspath(os.path.join(_HOOKS, "..", "dev"))\n'
        "if _DEV not in sys.path:\n    sys.path.insert(0, _DEV)\n\n"
        "import tool_version  # noqa: E402\n",
        encoding="utf-8",
    )
    (root / "scripts/hooks/comments/check_record_names.py").write_text(
        "import os\nimport sys\n\n"
        "_HOOKS = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))\n"
        '_DEV = os.path.abspath(os.path.join(_HOOKS, "..", "dev"))\n'
        "for _entry in (_HOOKS, _DEV):\n"
        "    if _entry not in sys.path:\n        sys.path.insert(0, _entry)\n\n"
        "import comment_map  # noqa: E402\nimport gate_report  # noqa: E402\n",
        encoding="utf-8",
    )
    (root / "scripts/dev/comment_map.py").write_text(
        "import os\nimport sys\n\n"
        '_HOOKS = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "hooks"))\n'
        "if _HOOKS not in sys.path:\n    sys.path.insert(0, _HOOKS)\n\n"
        "import gate_report  # noqa: E402\n",
        encoding="utf-8",
    )
    (root / "scripts/hooks/comments/check_record_punct_test.py").write_text(
        "import sys\nfrom pathlib import Path\n\n\n"
        "def probe() -> None:\n"
        '    sys.path.insert(0, str(Path(__file__).parents[3] / "scripts" / "dev"))\n'
        "    import comment_rules  # noqa: E402\n",
        encoding="utf-8",
    )


def test_transition_edges_pass(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        script_skeleton(root)
        write_dev_modules(root)
        write_transition_edges(root)

    oks, fails = fixtures.run_case(script_hygiene, tmp_path, mutate)
    assert fails == []
    assert any(ok.startswith("script import direction") for ok in oks)
    assert any(ok.startswith("bootstrap form") for ok in oks)


def test_reverse_dependency_edge_rejected(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        script_skeleton(root)
        write_dev_modules(root)
        write_transition_edges(root)
        (root / "scripts/dev/other_tool.py").write_text(
            "import os\nimport sys\n\n"
            '_HOOKS = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "hooks"))\n'
            "if _HOOKS not in sys.path:\n    sys.path.insert(0, _HOOKS)\n\n"
            "import gate_report  # noqa: E402\n",
            encoding="utf-8",
        )

    _, fails = fixtures.run_case(script_hygiene, tmp_path, mutate)
    assert any("cross-bucket import" in f for f in fails)


def test_unlisted_cross_bucket_import_rejected(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        script_skeleton(root)
        write_dev_modules(root)
        (root / "scripts/hooks/check_extra.py").write_text(
            "import os\nimport sys\n\n"
            "_DEV = os.path.abspath(\n"
            '    os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "dev")\n'
            ")\n"
            "if _DEV not in sys.path:\n    sys.path.insert(0, _DEV)\n\n"
            "import comment_map  # noqa: E402\n",
            encoding="utf-8",
        )

    _, fails = fixtures.run_case(script_hygiene, tmp_path, mutate)
    assert any("cross-bucket import" in f for f in fails)


def test_leaf_bucket_import_rejected(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        script_skeleton(root)
        (root / "scripts/perf").mkdir(parents=True)
        (root / "scripts/perf/helper.py").write_text("VALUE = 1\n", encoding="utf-8")
        (root / "scripts/hooks/check_leaf.py").write_text("import helper\n", encoding="utf-8")

    _, fails = fixtures.run_case(script_hygiene, tmp_path, mutate)
    assert any("cross-bucket import" in f for f in fails)


def test_scripts_root_import_passes(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        script_skeleton(root)
        (root / "scripts/tool_version.py").write_text('VERSION = "0"\n', encoding="utf-8")
        (root / "scripts/hooks/check_final.py").write_text(
            "import os\nimport sys\n\n"
            "_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))\n"
            "if _ROOT not in sys.path:\n    sys.path.insert(0, _ROOT)\n\n"
            "import tool_version  # noqa: E402\n",
            encoding="utf-8",
        )

    oks, fails = fixtures.run_case(script_hygiene, tmp_path, mutate)
    assert fails == []
    assert any(ok.startswith("script import direction") for ok in oks)
    assert any(ok.startswith("bootstrap form") for ok in oks)


def test_subpackage_bucket_root_bootstrap_passes(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        script_skeleton(root)
        (root / "scripts/hooks/comments/check_router.py").write_text(
            "import os\nimport sys\n\n"
            "_HOOKS = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))\n"
            "if _HOOKS not in sys.path:\n    sys.path.insert(0, _HOOKS)\n\n"
            "import gate_report  # noqa: E402\n",
            encoding="utf-8",
        )

    oks, fails = fixtures.run_case(script_hygiene, tmp_path, mutate)
    assert fails == []
    assert any(ok.startswith("bootstrap form") for ok in oks)


def test_redundant_own_directory_bootstrap_rejected(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        script_skeleton(root)
        (root / "scripts/hooks/check_redundant.py").write_text(
            "import os\nimport sys\n\n"
            "if True:\n    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))\n",
            encoding="utf-8",
        )

    _, fails = fixtures.run_case(script_hygiene, tmp_path, mutate)
    assert any("redundant bootstrap" in f for f in fails)


def test_test_file_insert_rejected(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        script_skeleton(root)
        (root / "scripts/hooks/check_x_test.py").write_text(
            "import os\nimport sys\n\n\n"
            "def probe() -> None:\n"
            "    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))\n",
            encoding="utf-8",
        )

    _, fails = fixtures.run_case(script_hygiene, tmp_path, mutate)
    assert any("test file carries a sys.path.insert" in f for f in fails)


def test_multiple_inserts_rejected(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        script_skeleton(root)
        (root / "scripts/hooks/check_two.py").write_text(
            "import os\nimport sys\n\n"
            "_A = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))\n"
            "if _A not in sys.path:\n    sys.path.insert(0, _A)\n"
            "if _A not in sys.path:\n    sys.path.insert(0, _A)\n",
            encoding="utf-8",
        )

    _, fails = fixtures.run_case(script_hygiene, tmp_path, mutate)
    assert any("at most one" in f for f in fails)


def test_unresolved_bootstrap_target_rejected(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        script_skeleton(root)
        (root / "scripts/hooks/check_odd.py").write_text(
            "import sys\n\nif True:\n    sys.path.insert(0, compute_base())\n",
            encoding="utf-8",
        )

    _, fails = fixtures.run_case(script_hygiene, tmp_path, mutate)
    assert any("cannot be resolved" in f for f in fails)


def test_bootstrap_allow_marker_with_reason_passes(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        script_skeleton(root)
        (root / "scripts/hooks/check_probe.py").write_text(
            "import sys\n\n"
            "if True:\n"
            "    sys.path.insert(0, compute_base())"
            "  # bootstrap:allow 工具链探针路径，运行时拼装\n",
            encoding="utf-8",
        )

    oks, fails = fixtures.run_case(script_hygiene, tmp_path, mutate)
    assert fails == []
    assert any(ok.startswith("bootstrap form") for ok in oks)


def test_bootstrap_allow_marker_without_reason_flagged(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        script_skeleton(root)
        (root / "scripts/hooks/check_bare.py").write_text(
            "import sys\n\nif True:\n    sys.path.insert(0, compute_base())  # bootstrap:allow\n",
            encoding="utf-8",
        )

    _, fails = fixtures.run_case(script_hygiene, tmp_path, mutate)
    assert any("marker without a reason" in f for f in fails)


def test_router_reports_script_hygiene_stems(tmp_path: Path) -> None:
    oks, fails = fixtures.run_case(constraints, tmp_path, script_skeleton)
    assert fails == []
    assert any(ok.startswith("script import direction") for ok in oks)
    assert any(ok.startswith("bootstrap form") for ok in oks)


def test_syntax_error_fails_closed(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        script_skeleton(root)
        (root / "scripts/hooks/broken.py").write_text("def f(:\n", encoding="utf-8")

    _, fails = fixtures.run_case(script_hygiene, tmp_path, mutate)
    assert any("cannot parse this file" in f for f in fails)
