"""约束门禁·脚本卫生：脚本目录（scripts/）的桶间依赖与路径引导两类判定。
跨桶导入的出边只开放脚本根（scripts）单点小库，开发桶（dev）与三个叶桶
（perf、release、ci）禁入，存量过渡边按显式白名单放行并随依赖单向化拉取
请求删除；路径引导（sys.path.insert）按解析后的目标路径判定，定案只许子
包文件的宿主桶根与脚本根（scripts）两类目标、非测试文件至多一处、测试文
件零处，同目录与本桶根插入按冗余判罚，静态解析不了的写法即罚——改惯例形
态或行尾加放行标记带缘由放行。判定经抽象语法树静态解析，支持路径函数族
（os.path）与路径对象族（pathlib）的惯用形态；脚本面缺席按跳过处置（约
束单测的假仓夹具没有脚本树，属常态）。违规经统一诊断出口带规则标识即时打
印，证据行完整不截断。桶模型、过渡白名单条目与解析近似口径的取舍见记录文
件。
"""

from __future__ import annotations

import ast
import os
from typing import Any

import gate_report

from . import manifests

RULE_SCRIPT_DEP = "SCRIPT-DEP"
RULE_SCRIPT_BOOTSTRAP = "SCRIPT-BOOTSTRAP"

SCRIPT_DEP_REMEDY = (
    "consume single-point libraries from the scripts root or keep imports"
    " inside the bucket; whitelisted transition edges retire together with"
    " the module moves"
)
BOOTSTRAP_REMEDY = (
    "bootstrap the own bucket root from a subpackage script or the scripts"
    " root for cross-bucket single-point imports, and delete redundant inserts"
)
UNRESOLVED_REMEDY = (
    "rewrite the bootstrap in the conventional os.path form, or add"
    " bootstrap:allow with a reason on the same line"
)
TEST_INSERT_REMEDY = (
    "import through package-qualified imports or rely on the production"
    " bootstrap; test files carry no sys.path.insert"
)

SCRIPTS_DIR = "scripts"
PRUNE_DIRS = frozenset({".git", "__pycache__"})

ALLOW_MARKER = "bootstrap:allow"

# 过渡白名单：条目随依赖单向化拉取请求整段删除，保证红牌不进主分支；每条
# 带缘由，与该拉取请求的归位步骤一一对应。依赖条目为（导入方路径，顶层模块
# 名，缘由），引导条目为（脚本路径，解析后目标路径，缘由）。
DEP_TRANSITION_EDGES = (
    (
        "scripts/hooks/check_duplication.py",
        "tool_version",
        "tool_version still lives in the dev bucket; the move to the scripts"
        " root retargets this edge",
    ),
    (
        "scripts/hooks/comments/check_record_names.py",
        "comment_map",
        "comment_map still lives in the dev bucket; the move into the comments"
        " package removes this edge",
    ),
    (
        "scripts/dev/comment_map.py",
        "gate_report",
        "comment_map consumes the gate infrastructure until it moves into the comments package",
    ),
)

BOOTSTRAP_TRANSITION = (
    (
        "scripts/dev/comment_map.py",
        "scripts/hooks",
        "comment_map consumes the gate infrastructure until it moves into the comments package",
    ),
    (
        "scripts/hooks/check_duplication.py",
        "scripts/dev",
        "tool_version still lives in the dev bucket; the move to the scripts"
        " root retargets this insert",
    ),
    (
        "scripts/hooks/comments/check_record_names.py",
        "scripts/dev",
        "comment_map still lives in the dev bucket; the move into the comments"
        " package removes this insert",
    ),
    (
        "scripts/hooks/comments/check_record_punct_test.py",
        "scripts/dev",
        "the rule quick-reference check reads the dev-side registry until that module moves",
    ),
)


class _Unresolved(Exception):
    """引导目标表达式超出静态解析支持面的哨兵。"""


class _TargetResolver:
    """路径引导目标（sys.path.insert）的静态解析：名字查模块级赋值与遍历
    循环（for）的单名目标（同名重绑按并集近似），支持路径函数族（os.path）
    与路径对象族（pathlib）的惯用形态——对象构造（Path）、父级清单下标
    （parents）、单层父级（parent）、规整解析（resolve）与除法拼接。"""

    def __init__(self, file_path: str, bindings: dict[str, list[ast.expr]]) -> None:
        self.file_path = os.path.normpath(os.path.abspath(file_path))
        self.base_dir = os.path.dirname(self.file_path)
        self.bindings = bindings

    def targets(self, expr: ast.expr) -> list[str]:
        """目标表达式到绝对路径清单：任何成分解析不了即整体按失败处置；仍为
        相对形态的结果锚定脚本所在目录后归一。"""
        out = []
        for path in self._expr(expr, frozenset()):
            if not os.path.isabs(path):
                path = os.path.join(self.base_dir, path)
            out.append(os.path.normpath(path))
        if not out:
            raise _Unresolved
        return out

    def _expr(self, expr: ast.expr, seen: frozenset[int]) -> list[str]:
        if isinstance(expr, ast.Constant) and isinstance(expr.value, str):
            return [expr.value]
        if isinstance(expr, ast.Name):
            if expr.id == "__file__":
                return [self.file_path]
            out = []
            for binding in self.bindings.get(expr.id, ()):
                key = id(binding)
                if key in seen:
                    continue
                out.extend(self._expr(binding, seen | {key}))
            if not out:
                raise _Unresolved
            return out
        if isinstance(expr, (ast.Tuple, ast.List)):
            out = []
            for elt in expr.elts:
                out.extend(self._expr(elt, seen))
            return out
        if isinstance(expr, ast.BinOp) and isinstance(expr.op, ast.Div):
            bases = self._expr(expr.left, seen)
            names = self._expr(expr.right, seen)
            return [os.path.join(b, n) for b in bases for n in names]
        if isinstance(expr, ast.Attribute) and expr.attr == "parent":
            return [os.path.dirname(p) for p in self._expr(expr.value, seen)]
        if isinstance(expr, ast.Subscript):
            return self._subscript(expr, seen)
        if isinstance(expr, ast.Call):
            return self._call(expr, seen)
        raise _Unresolved

    def _subscript(self, expr: ast.Subscript, seen: frozenset[int]) -> list[str]:
        value = expr.value
        if not (isinstance(value, ast.Attribute) and value.attr == "parents"):
            raise _Unresolved
        depth = expr.slice
        if not (isinstance(depth, ast.Constant) and isinstance(depth.value, int)):
            raise _Unresolved
        out = []
        for path in self._expr(value.value, seen):
            for _ in range(depth.value + 1):
                path = os.path.dirname(path)
            out.append(path)
        return out

    def _call(self, call: ast.Call, seen: frozenset[int]) -> list[str]:
        func = call.func
        if isinstance(func, ast.Name):
            if func.id == "str" and len(call.args) == 1:
                return self._expr(call.args[0], seen)
            if func.id == "Path" and call.args:
                parts = []
                for arg in call.args:
                    parts.extend(self._expr(arg, seen))
                return [os.path.join(*parts)]
            raise _Unresolved
        if not isinstance(func, ast.Attribute):
            raise _Unresolved
        if func.attr == "resolve" and not self._is_os_path(func.value):
            return self._expr(func.value, seen)
        if not self._is_os_path(func.value):
            raise _Unresolved
        args: list[str] = []
        for arg in call.args:
            args.extend(self._expr(arg, seen))
        if func.attr == "join":
            return [os.path.join(*args)]
        if func.attr in ("abspath", "realpath", "normpath"):
            return [os.path.normpath(p) for p in args]
        if func.attr == "dirname":
            return [os.path.dirname(p) for p in args]
        raise _Unresolved

    @staticmethod
    def _is_os_path(node: ast.expr) -> bool:
        return (
            isinstance(node, ast.Attribute)
            and node.attr == "path"
            and isinstance(node.value, ast.Name)
            and node.value.id == "os"
        )


def _name_bindings(tree: ast.Module) -> dict[str, list[ast.expr]]:
    """绑定表：模块级单名赋值与遍历循环（for）的单名目标到表达式清单，供名
    字回查。"""
    out: dict[str, list[ast.expr]] = {}
    for node in ast.walk(tree):
        if isinstance(node, ast.Assign) and len(node.targets) == 1:
            target = node.targets[0]
            if isinstance(target, ast.Name):
                out.setdefault(target.id, []).append(node.value)
        elif isinstance(node, ast.For) and isinstance(node.target, ast.Name):
            if isinstance(node.iter, (ast.Tuple, ast.List)):
                out.setdefault(node.target.id, []).extend(node.iter.elts)
    return out


def _is_insert(node: ast.AST) -> bool:
    """路径引导调用（sys.path.insert）两参形态的判定。"""
    return (
        isinstance(node, ast.Call)
        and isinstance(node.func, ast.Attribute)
        and node.func.attr == "insert"
        and isinstance(node.func.value, ast.Attribute)
        and node.func.value.attr == "path"
        and isinstance(node.func.value.value, ast.Name)
        and node.func.value.value.id == "sys"
        and len(node.args) == 2
    )


def _import_tops(tree: ast.Module) -> dict[str, int]:
    """绝对导入的顶层模块名到语句首行：相对导入不判（包内自持），导入语句
    （import）多别名逐名登记，同名取首行。"""
    out: dict[str, int] = {}
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                out.setdefault(alias.name.split(".")[0], node.lineno)
        elif isinstance(node, ast.ImportFrom) and node.level == 0 and node.module:
            out.setdefault(node.module.split(".")[0], node.lineno)
    return out


def _bucket_of(rel: str) -> str:
    """脚本相对路径到桶名：脚本根（scripts）的直属文件归脚本根，其余取脚
    本树的下一段（子包归属宿主桶）。"""
    parts = rel.split("/")
    if len(parts) <= 2:
        return "scripts"
    return parts[1]


def _bucket_root(rel: str) -> str:
    """脚本所在桶的桶根相对路径。"""
    parts = rel.split("/")
    return "scripts/" + parts[1] if len(parts) > 2 else "scripts"


def _script_files(root: str, loader: Any) -> list[tuple[str, str, str | None]]:
    """脚本面枚举：脚本树（scripts）下全部脚本文件，剪去缓存与版本库目录，
    按路径排序保证输出确定；返回（相对路径，绝对路径，文本）三元组，读取
    失败的文件文本置空交调用方判基础设施条目。"""
    out = []
    script_root = os.path.join(root, SCRIPTS_DIR)
    if not os.path.isdir(script_root):
        return out
    for dirpath, dirnames, filenames in os.walk(script_root):
        dirnames[:] = sorted(d for d in dirnames if d not in PRUNE_DIRS)
        for name in sorted(filenames):
            if not name.endswith(".py"):
                continue
            abs_path = os.path.join(dirpath, name)
            rel = manifests.rel(root, abs_path)
            try:
                text: str | None = manifests.read_text(abs_path, loader)
            except OSError:
                text = None
            out.append((rel, abs_path, text))
    return sorted(out)


def _module_index(files: list[tuple[str, str, str]]) -> dict[str, list[str]]:
    """顶层模块索引：模块名到提供它的脚本相对路径清单——普通模块文件按名
    登记，包目录的初始化文件（__init__.py）按目录名登记。"""
    out: dict[str, list[str]] = {}
    for rel, _abs, _text in files:
        name = os.path.basename(rel)
        stem = name[:-3]
        if stem == "__init__":
            stem = os.path.basename(os.path.dirname(rel))
        out.setdefault(stem, []).append(rel)
    return out


def _line_text(lines: list[str], lineno: int) -> str:
    return lines[lineno - 1] if 0 < lineno <= len(lines) else ""


def _dep_allowed(rel: str, top: str) -> bool:
    return any(entry[0] == rel and entry[1] == top for entry in DEP_TRANSITION_EDGES)


def _bootstrap_allowed(rel: str, target: str) -> bool:
    return any(entry[0] == rel and entry[1] == target for entry in BOOTSTRAP_TRANSITION)


def _classify_target(rel: str, target: str) -> str | None:
    """引导目标的定案分类：违规返回消息，放行返回空。"""
    file_dir = os.path.dirname(rel)
    if target == file_dir:
        return "redundant bootstrap: %s is already on sys.path" % target
    if target == _bucket_root(rel) and file_dir != _bucket_root(rel):
        return None
    if target == SCRIPTS_DIR and file_dir != SCRIPTS_DIR:
        return None
    parts = target.split("/")
    if parts[0] != SCRIPTS_DIR:
        return "bootstrap target %s lies outside the scripts tree" % target
    if len(parts) > 1 and parts[1] != _bucket_of(rel):
        return (
            "bootstrap crosses into the %s bucket: %s (allowed targets are the"
            " own bucket root and the scripts root)" % (parts[1], target)
        )
    return "bootstrap target %s is neither the own bucket root nor the scripts root" % target


def _judge_dep(
    rel: str,
    tree: ast.Module,
    index: dict[str, list[str]],
    is_test: bool,
    lines: list[str],
    rep: manifests.Reporter,
) -> bool:
    """桶间依赖判定：跨桶导入只许指向脚本根（scripts），过渡白名单边放行；
    测试文件不判（跨目录导入走包限定形态，随生产引导归位自然收敛）。"""
    if is_test:
        return True
    own = _bucket_of(rel)
    clean = True
    for top, lineno in _import_tops(tree).items():
        providers = index.get(top)
        if not providers:
            continue
        buckets = {_bucket_of(p) for p in providers}
        if own in buckets or "scripts" in buckets:
            continue
        if _dep_allowed(rel, top):
            continue
        clean = False
        buckets = "/".join(sorted(buckets))
        rep.note(
            gate_report.Finding(
                rule=RULE_SCRIPT_DEP,
                path=rel,
                line=lineno,
                message=(
                    "cross-bucket import %r reaching the %s bucket from the %s"
                    " bucket (only the scripts root accepts cross-bucket"
                    " imports)" % (top, buckets, own)
                ),
                evidence=_line_text(lines, lineno),
                remedy=SCRIPT_DEP_REMEDY,
            )
        )
    return clean


def _judge_bootstrap(
    root: str,
    rel: str,
    abs_path: str,
    tree: ast.Module,
    bindings: dict[str, list[ast.expr]],
    is_test: bool,
    lines: list[str],
    rep: manifests.Reporter,
) -> bool:
    """路径引导判定：按解析后的目标分类，放行标记带缘由豁免目标分类（不豁
    免数量上限），测试文件零处、非测试文件至多一处。"""
    resolver = _TargetResolver(abs_path, bindings)
    inserts = [node for node in ast.walk(tree) if _is_insert(node)]
    clean = True
    if len(inserts) > 1 and not is_test:
        clean = False
        rep.note(
            gate_report.Finding(
                rule=RULE_SCRIPT_BOOTSTRAP,
                path=rel,
                line=inserts[0].lineno,
                message=(
                    "script carries %d sys.path.insert entries; a non-test"
                    " script carries at most one" % len(inserts)
                ),
                evidence=_line_text(lines, inserts[0].lineno),
                remedy=BOOTSTRAP_REMEDY,
            )
        )
    for node in inserts:
        lineno = node.lineno
        line = _line_text(lines, lineno)
        if ALLOW_MARKER in line:
            if gate_report.allow_reason_missing(ALLOW_MARKER, line):
                rep.note(gate_report.allow_reason_finding(ALLOW_MARKER, rel, lineno, line))
                clean = False
            continue
        try:
            targets = resolver.targets(node.args[1])
        except _Unresolved:
            targets = None
        rel_targets = [manifests.rel(root, t) for t in targets] if targets else None
        if is_test:
            if rel_targets and all(_bootstrap_allowed(rel, t) for t in rel_targets):
                continue
            clean = False
            rep.note(
                gate_report.Finding(
                    rule=RULE_SCRIPT_BOOTSTRAP,
                    path=rel,
                    line=lineno,
                    message=(
                        "test file carries a sys.path.insert (tests import"
                        " through package-qualified imports)"
                    ),
                    evidence=line,
                    remedy=TEST_INSERT_REMEDY,
                )
            )
            continue
        if rel_targets is None:
            clean = False
            rep.note(
                gate_report.Finding(
                    rule=RULE_SCRIPT_BOOTSTRAP,
                    path=rel,
                    line=lineno,
                    message="bootstrap target cannot be resolved statically",
                    evidence=line,
                    remedy=UNRESOLVED_REMEDY,
                )
            )
            continue
        for target in rel_targets:
            message = _classify_target(rel, target)
            if message is None or _bootstrap_allowed(rel, target):
                continue
            clean = False
            rep.note(
                gate_report.Finding(
                    rule=RULE_SCRIPT_BOOTSTRAP,
                    path=rel,
                    line=lineno,
                    message=message,
                    evidence=line,
                    remedy=BOOTSTRAP_REMEDY,
                )
            )
    return clean


def run(
    root: str, loader: Any = None, report: manifests.Reporter | None = None
) -> tuple[list[str], list[str]]:
    """执行脚本桶间依赖与路径引导两类约束，返回通过与失败两个清单。"""
    root = os.path.abspath(root)
    rep = report if report is not None else manifests.Reporter()
    oks: list[str] = []
    files = _script_files(root, loader)
    if not files:
        oks.append("script import direction (no scripts/ surface, skipped)")
        oks.append("bootstrap form (no scripts/ surface, skipped)")
        return oks, rep.lines
    readable = []
    for rel, path, text in files:
        if text is None:
            rep.note(
                gate_report.Finding(
                    rule=gate_report.RULE_INFRA,
                    path="",
                    line=0,
                    message="script hygiene cannot read this file: %s" % rel,
                    remedy=gate_report.INFRA_REMEDY,
                )
            )
        else:
            readable.append((rel, path, text))
    if not readable:
        return oks, rep.lines
    dep_clean = True
    boot_clean = True
    index = _module_index(readable)
    for rel, path, text in readable:
        try:
            tree = ast.parse(text)
        except SyntaxError as exc:
            dep_clean = boot_clean = False
            rep.note(
                gate_report.Finding(
                    rule=gate_report.RULE_INFRA,
                    path=rel,
                    line=exc.lineno or 0,
                    message="script hygiene cannot parse this file: %s" % exc.msg,
                    remedy=gate_report.INFRA_REMEDY,
                )
            )
            continue
        is_test = os.path.basename(rel).endswith("_test.py")
        lines = text.split("\n")
        bindings = _name_bindings(tree)
        dep_clean = _judge_dep(rel, tree, index, is_test, lines, rep) and dep_clean
        boot_clean = (
            _judge_bootstrap(root, rel, path, tree, bindings, is_test, lines, rep) and boot_clean
        )
    if dep_clean:
        oks.append(
            "script import direction (%d scripts scanned; cross-bucket imports"
            " reach only the scripts root or listed transition edges)" % len(readable)
        )
    if boot_clean:
        oks.append(
            "bootstrap form (%d scripts scanned; one insert at most per"
            " non-test script, tests carry none)" % len(readable)
        )
    return oks, rep.lines
