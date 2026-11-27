"""注释纪律门禁子包（包初始化）：再导出四个书写判罚子门禁与记录名门禁，
供并轨入口与跨目录测试经包限定导入取用，形态对齐约束门禁子包；判罚路由住
路由模块（check_comments），命令入口不设——配方与并轨入口按路径直跑路由模
块，本包不仿约束子包另立命令入口。包化动机与引导清理的取舍见记录文件。
"""

from . import (
    check_docs,
    check_record_names,
    check_record_punct,
    check_rust_comments,
    check_script_comments,
)

__all__ = [
    "check_docs",
    "check_record_names",
    "check_record_punct",
    "check_rust_comments",
    "check_script_comments",
]
