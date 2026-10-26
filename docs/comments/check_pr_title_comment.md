# check_pr_title.py 注释记录

- 调用方：.zcode/config.json 的 hooks.events.PreToolUse；校验器为
  scripts/hooks/check_commit_msg.py。
- 触发工具：mcp__github__create_pull_request、mcp__github__update_pull_request 校验
  tool_input.title；Bash 经 shlex 分词后定位 `gh pr create|edit`，只在其后找
  --title（--title "x" / 'x' / =x 等价）。
- 标题取不到的情形：--fill、--web、--title-file、无 --title、分词失败。
- 手动测试：`echo '<hook-json>' | python3 .zcode/hooks/check_pr_title.py`
- squash 模拟占位：拼装单点在 scripts/hooks/check_commit_msg.py 的合并模拟
  函数（五位编号占位，最长常见形态），本钩子经它取模拟消息，不再自抄占位。
