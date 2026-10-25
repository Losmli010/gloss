"""约束门禁·共享清单扫描：各领域模块共用的工具——工作区清单枚举、包名与
节正文提取、依赖条目抽取与汇总、本语言（Rust）源码树遍历、文本读取与违规
即时报告；另提供单次扫描的内容缓存：清单枚举与文件内容按路径缓进字典，
同一文件一次扫描只打开一次，聚合入口缺省自建缓存，装入器在场时共享其缓
存，单模块直跑仍各自读取；读取失败照常抛出，读不到即报违规的现行路径不
变形。条目跨行拼接与各助手的取舍见记录文件。
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
from collections.abc import Iterator
from typing import Any

PKG_NAME_RE = re.compile(r"name\s*=")

DEP_KEY_RE = re.compile(r"[A-Za-z0-9_-]+\s*=")


class ContentCache:
    """单次约束扫描的内容缓存：绑定扫描根，工作区清单枚举、版本库跟踪
    清单与文件内容的字节、解码文本各按路径（文本连带容错口径）缓进字典；
    读取失败照常抛出，不因缓存吞掉报错路径。"""

    def __init__(self, root: str) -> None:
        self.root = root
        self._manifests = None
        self._tracked = None
        self._bytes = {}
        self._text = {}

    def manifests(self, root: str) -> list[str]:
        """工作区清单枚举：首次调用枚举一次，此后整段扫描共享同一结果。"""
        if self._manifests is None:
            self._manifests = manifests(root)
        return self._manifests

    def tracked(self) -> list[str]:
        """版本库跟踪清单：枚举一次，供残留任务标记扫描消费。"""
        if self._tracked is None:
            out = subprocess.run(
                ["git", "-C", self.root, "ls-files", "-z"], capture_output=True, check=True
            ).stdout
            self._tracked = [p.decode("utf-8", "replace") for p in out.split(b"\0") if p]
        return self._tracked

    def read_bytes(self, path: str) -> bytes:
        """字节层：按路径缓存，读取失败照常抛出。"""
        if path not in self._bytes:
            with open(path, "rb") as fh:
                self._bytes[path] = fh.read()
        return self._bytes[path]

    def read_text(self, path: str, errors: str = "strict") -> str:
        """解码层：文本模式读取语义（通用换行翻译在内），按路径与容错口径缓存。"""
        key = (path, errors)
        if key not in self._text:
            with open(path, encoding="utf-8", errors=errors) as fh:
                self._text[key] = fh.read()
        return self._text[key]


def manifests(root: str, loader: Any = None) -> list[str]:
    if loader is not None:
        return loader.manifests(root)
    out = []
    root_manifest = os.path.join(root, "Cargo.toml")
    if os.path.isfile(root_manifest):
        out.append(root_manifest)
    crates = os.path.join(root, "crates")
    if os.path.isdir(crates):
        for name in sorted(os.listdir(crates)):
            m = os.path.join(crates, name, "Cargo.toml")
            if os.path.isfile(m):
                out.append(m)
    return out


def pkg_name(manifest_path: str, loader: Any = None) -> str:
    """取 [package] 里的 crate 名。"""
    in_section = False
    for line in read_text(manifest_path, loader).split("\n"):
        stripped = line.strip()
        if stripped.startswith("["):
            in_section = stripped == "[package]"
            continue
        if in_section and PKG_NAME_RE.match(stripped):
            return stripped.split("=", 1)[1].strip().strip("\"'")
    return ""


def section_body(manifest_path: str, want: str, loader: Any = None) -> list[str]:
    """取某个 section 的正文（到下一个 section 头为止）。"""
    out = []
    in_section = False
    for line in read_text(manifest_path, loader).split("\n"):
        if line.startswith("["):
            in_section = line.rstrip() == want
            continue
        if in_section:
            out.append(line.rstrip("\n"))
    return out


def dep_entries(manifest_path: str, loader: Any = None) -> list[tuple[str, str, int, str]]:
    """抽取依赖条目，输出 (section, key, 行号, 条目全文（跨行拼接）)。

    依赖表含 [dependencies] / [dev-dependencies] / [build-dependencies]，以及
    [target.'cfg(...)'.dev-dependencies] 这类门控形式（均以 dependencies] 结尾）。"""
    entries = []
    sec = None
    in_deps = False
    pending = False
    key = start = text = None
    for lineno, line in enumerate(read_text(manifest_path, loader).split("\n"), 1):
        line = line.rstrip("\n")
        if line.startswith("["):
            sec = line
            in_deps = sec.endswith("dependencies]")
            continue
        if not in_deps:
            continue
        if DEP_KEY_RE.match(line):
            if pending:
                entries.append((sec, key, start, text))
                pending = False
            key = line.split("=", 1)[0].strip()
            start = lineno
            text = line
            depth = line.count("{") - line.count("}")
            if depth <= 0:
                entries.append((sec, key, start, text))
            else:
                pending = True
            continue
        if pending:
            text = text + " " + line
            depth += line.count("{") - line.count("}")
            if depth <= 0:
                pending = False
                entries.append((sec, key, start, text))
    if pending:
        entries.append((sec, key, start, text))
    return entries


def dep_dump(root: str, loader: Any = None) -> list[tuple[str, str, str, str, int, str]]:
    """全仓依赖条目汇总：条目依次是属主、清单相对路径、节、键、行号与全文。

    属主取清单里的包名，取不到时回退清单所在目录名（根清单即包目录）。"""
    out = []
    for m in manifests(root, loader):
        name = pkg_name(m, loader)
        if not name:
            name = os.path.basename(os.path.dirname(m))
        for sec, key, start, text in dep_entries(m, loader):
            out.append((name, os.path.relpath(m, root), sec, key, start, text))
    return out


def walk_rs(*bases: str) -> Iterator[str]:
    """列各 base 下的 .rs 文件；base 不存在则跳过。任意层级剪枝 target / .git：
    cargo 产物只应出现在仓库根，但误提交进源码树深处的同名目录同样不该进
    扫描面（bash 版只剪根级，这里是审慎的超集）。"""
    for base in bases:
        if not os.path.isdir(base):
            continue
        for dirpath, dirnames, filenames in os.walk(base):
            dirnames[:] = sorted(d for d in dirnames if d not in ("target", ".git"))
            for name in sorted(filenames):
                if name.endswith(".rs"):
                    yield os.path.join(dirpath, name)


def read_text(path: str, loader: Any = None) -> str:
    if loader is not None:
        return loader.read_text(path, errors="replace")
    with open(path, encoding="utf-8", errors="replace") as fh:
        return fh.read()


def rel(root: str, path: str) -> str:
    """统一归一为斜杠风格的仓库相对路径，跨平台下与源码里的常量路径可比。"""
    return os.path.relpath(path, root).replace(os.sep, "/")


def is_path_dep(text: str) -> bool:
    return "path =" in text or "path=" in text


def note_violation(fails: list[str], msg: str) -> None:
    """违规随执行流即时打到标准错误并进清单，供调用方汇总返回。"""
    fails.append(msg)
    print("  ✗ %s" % msg, file=sys.stderr)
