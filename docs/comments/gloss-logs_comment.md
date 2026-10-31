# scripts/dev/gloss-logs.py 决策记录

> 锚定到脚本内的函数名，不写行号。改到相关代码时同步更新。

## 为什么是独立脚本而不是 justfile 里的内联管道

- 2026-09-24 日志改成 JSONL 之后，`tail -f` 出来的原始 JSON 不再适合人看。内联 shell 再叠 `python3 -c` 既难读也难测，故抽成脚本：`just logs [args…]` 只是它的薄包装。
- 不用 `jq`：stock macOS 没预装（本机就没有），脚本只用 python3 标准库（系统自带）。

## `format_line` 的取舍

- 输出 `HH:MM:SS 级别 短模块名 消息 字段…`：时间转**本地时区**（日志本体是 UTC RFC3339，给人看要换成本地）；模块名砍掉 crate 前缀（`gloss_app::app::channels` → `app::channels`）省横向空间。
- `generation` 不特殊渲染：它已经是顶层普通字段，与其它字段一起列在行尾——过滤由 `matches` 负责，`--generation N` 正是“一条命令拉出全链路”的入口。
- TTY 才上色；`--raw` 原样透传（给 jq/其它工具），`--no-color` 手动关闭。

## `follow` 的两个细节

- 默认跟随（等价 `tail -f`），`--no-follow` 打完就退出。
- 跟随期间每轮重新解析“最新文件”：跨天换文件后自动切过去，不用重开终端。
- 管道下游提前关闭（`| head`）时按 SIGPIPE 静默退出，不抛栈。
