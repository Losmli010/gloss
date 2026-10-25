#!/usr/bin/env python3
"""硬编码密钥扫描门禁：扫描版本库跟踪的全部文件，命中任一模式即失败。
本地提交前门禁与持续集成检查共用此脚本，保证本地与远端判定一致。

规则取向：只收高信号格式（服务商密钥前缀、私钥块、带引号的凭据赋值）。
确属合法字面量（文档示例、测试夹具）时，在该行尾加放行标记并注明缘由。
匹配引擎委托正则检索工具（ripgrep）：模式清单在本文件单点定义，运行时拼装
参数一次子进程跑全部模式；二进制文件与放行标记行由本文件前后滤除，判定
口径与既有逐行实现一致，缘由见记录文件。
"""

import base64
import json
import os
import re
import subprocess
import sys

# 服务商专属格式，区分大小写（这些前缀本身就有大小写语义）
CASE_SENSITIVE_PATTERNS = [
    # 模型服务商（OpenAI）等：短前缀左侧必须是非标识符字符，避免普通英文词连字误报
    re.compile(r"(^|[^A-Za-z0-9_-])sk-[A-Za-z0-9_-]{20,}"),
    # 云服务商：长期密钥与临时凭证各有专属前缀
    re.compile(r"A[SK]IA[0-9A-Z]{16}"),
    # 托管平台（GitHub）令牌：经典、应用与细粒度三种前缀
    re.compile(r"gh[pousr]_[A-Za-z0-9]{36,}"),
    re.compile(r"github_pat_[A-Za-z0-9_]{20,}"),
    # 另一代码托管平台的访问令牌
    re.compile(r"glpat-[A-Za-z0-9_-]{20,}"),
    # 即时通讯平台的令牌，前缀按字母位区分类型
    re.compile(r"xox[baprs]-[A-Za-z0-9-]{10,}"),
    # 云服务接口密钥，前缀定长
    re.compile(r"AIza[0-9A-Za-z_-]{35}"),
    # 私钥块（任何算法）
    re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----"),
]

# 通用凭据赋值，大小写不敏感：
# 其一，带引号的值（各类配置文件的常见形态），十二字符起；
# 其二，不带引号的值（环境变量导出形态），二十字符起压误报；
# 其三，请求头携带的持票人凭据。
# 不含裸令牌关键字——真实令牌泄漏由服务商前缀模式兜底。
CASE_INSENSITIVE_PATTERNS = [
    re.compile(
        r'(api[_-]?key|secret|password|passwd|credential|access[_-]?key)["\']?'
        r"\s*[:=]\s*[\"'][^\"']{12,}[\"']",
        re.IGNORECASE,
    ),
    re.compile(
        r"(api[_-]?key|secret|password|passwd|credential|access[_-]?key)[\"']?"
        r"\s*[:=]\s*[A-Za-z0-9+/_-]{20,}",
        re.IGNORECASE,
    ),
    re.compile(
        r'authorization["\']?\s*[:=]\s*["\']?bearer\s+[A-Za-z0-9._-]{16,}',
        re.IGNORECASE,
    ),
]

ALLOW_MARKER = "secrets:allow"

# 字节序标记前缀：引擎会剥除或据此整文件转码，与既有的容错解码口径对不齐
BOM_PREFIXES = (b"\xff\xfe", b"\xfe\xff", b"\xef\xbb\xbf")

# 空白类超集：解释器的空白类比引擎多四个控制分隔符，候选面按超集展开后
# 引擎筛出的行必不漏解释器可判的行，判定仍由解释器侧模式收口
SPACE_SUPERSET = r"[\s\x1c-\x1f]"


def tracked_files(root):
    # 只看版本库跟踪的文件：被忽略规则排除的本地文件不扫（也不该进仓库）
    out = subprocess.run(
        ["git", "-C", root, "ls-files", "-z"],
        capture_output=True,
        check=True,
    ).stdout
    return [p.decode("utf-8", "replace") for p in out.split(b"\0") if p]


def scannable_files(root):
    """跟踪文件分流：工作区不在场或含空字节的整文件跳过，带字节序标记的改逐行兜底。

    返回可扫路径的清单序与其中需兜底子集，其余路径交引擎。"""
    out = []
    fallback = set()
    for rel in tracked_files(root):
        try:
            with open(os.path.join(root, rel), "rb") as fh:
                data = fh.read()
        except OSError:
            continue
        if b"\0" in data:
            continue
        if data.startswith(BOM_PREFIXES):
            fallback.add(rel)
        out.append(rel)
    return out, fallback


def engine_expressions():
    """把模式清单翻成引擎参数序列：不敏感组加内联旗标前缀并把空白类放大成超集。

    正则本体不复制第二份——超集只作用在候选预筛上，归类判定仍跑清单原模式；
    引擎的方括号类支持嵌套，空白类出现在类内时替换也成活。"""
    out = []
    for pattern in CASE_SENSITIVE_PATTERNS:
        out.append(pattern.pattern)
    for pattern in CASE_INSENSITIVE_PATTERNS:
        out.append("(?i)" + pattern.pattern.replace("\\s", SPACE_SUPERSET))
    return out


def json_field(blob):
    """引擎输出的取值字段：文本直取，字节按编码还原后容错解码。"""
    if "text" in blob:
        return blob["text"]
    return base64.b64decode(blob["bytes"]).decode("utf-8", "replace")


def collect_hits(root, files):
    """一次子进程跑全部模式，命中整理成「路径到行号与行文」的嵌套映射。"""
    if not files:
        return {}
    cmd = ["rg", "--json", "--no-config"]
    for expression in engine_expressions():
        cmd += ["-e", expression]
    cmd += ["--"]
    cmd += files
    try:
        proc = subprocess.run(cmd, cwd=root, capture_output=True)
    except FileNotFoundError:
        raise RuntimeError(
            "ripgrep not found: the secrets gate depends on it on every commit —"
            " install the pinned version from the repo tool list (just setup verifies it)"
        ) from None
    stderr = proc.stderr.decode("utf-8", "replace").strip()
    if proc.returncode == 2:
        raise RuntimeError("ripgrep engine failed (exit 2): %s" % stderr)
    if proc.returncode not in (0, 1):
        raise RuntimeError(
            "ripgrep engine exited unexpectedly (%d): %s" % (proc.returncode, stderr)
        )
    hits = {}
    # 事件流按换行符逐字切：通用换行语义会把行文里未转义的同类字符当切点撕裂事件
    for line in proc.stdout.decode("utf-8", "replace").split("\n"):
        if not line:
            continue
        try:
            event = json.loads(line)
            if event.get("type") != "match":
                continue
            data = event["data"]
            rel = json_field(data["path"])
            text = json_field(data["lines"])
            if text.endswith("\n"):
                text = text[:-1]
        except (ValueError, KeyError, TypeError) as exc:
            raise RuntimeError(
                "ripgrep produced an unparsable event stream (%r); refusing to"
                " continue — report this as a gate failure" % exc
            ) from None
        hits.setdefault(rel, {})[data["line_number"]] = text
    return hits


def pattern_hits(pattern, file_hits):
    """单模式的可见命中：放行标记行先滤掉，其余命中行按本模式逐条归类。"""
    return [
        (lineno, line)
        for lineno, line in sorted(file_hits.items())
        if ALLOW_MARKER not in line and pattern.search(line)
    ]


def fallback_hits(path):
    """字节序标记文件的逐行命中：任一模式即收，归类与放行过滤与引擎命中同路。"""
    try:
        with open(path, "rb") as fh:
            text = fh.read().decode("utf-8", "replace")
    except OSError:
        return {}
    all_patterns = CASE_SENSITIVE_PATTERNS + CASE_INSENSITIVE_PATTERNS
    hits = {}
    for lineno, line in enumerate(text.split("\n"), 1):
        if any(pattern.search(line) for pattern in all_patterns):
            hits[lineno] = line
    return hits


def run(root):
    """扫描 root 下全部 git 跟踪文件，返回违规条目（每条含文件与命中行）。"""
    files, fallback = scannable_files(root)
    hits_by_file = collect_hits(root, [rel for rel in files if rel not in fallback])
    for rel in fallback:
        line_hits = fallback_hits(os.path.join(root, rel))
        if line_hits:
            hits_by_file[rel] = line_hits
    problems = []
    for rel in files:
        file_hits = hits_by_file.get(rel)
        if not file_hits:
            continue
        for kind, patterns in (
            ("suspected hardcoded secret", CASE_SENSITIVE_PATTERNS),
            ("suspected hardcoded credential", CASE_INSENSITIVE_PATTERNS),
        ):
            for pattern in patterns:
                hits = pattern_hits(pattern, file_hits)
                if hits:
                    detail = "\n".join("  %d:%s" % hit for hit in hits)
                    problems.append("%s: %s\n%s" % (kind, rel, detail))
    return problems


def main():
    root = (
        sys.argv[1]
        if len(sys.argv) > 1
        else os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    )
    try:
        problems = run(root)
    except RuntimeError as exc:
        print("error: %s" % exc, file=sys.stderr)
        return 1
    if problems:
        for p in problems:
            print(p, file=sys.stderr)
        print("", file=sys.stderr)
        print(
            "error: suspected hardcoded secrets/credentials detected (see above).", file=sys.stderr
        )
        print(
            "  - real leak: revoke and rotate the secret immediately;"
            " read it from the config store instead;",
            file=sys.stderr,
        )
        print(
            "  - legitimate literal (doc example / test fixture): add %s at the end of"
            " the line with a reason." % ALLOW_MARKER,
            file=sys.stderr,
        )
        return 1
    print("✓ secrets scan passed (no hardcoded secrets/credentials)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
