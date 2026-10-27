# check_pr_title.py 注释记录

- 调用方：.zcode/config.json 的 hooks.events.PreToolUse；校验器为
  scripts/hooks/check_commit_msg.py、check_pr_prose.py 与 check_review_result.py。
- 触发工具：mcp__github__create_pull_request、mcp__github__update_pull_request 校验
  tool_input.title 与 body；mcp__github__create_and_submit_pull_request_review、
  mcp__github__submit_pending_pull_request_review 校验 tool_input.body；Bash 经
  shlex 分词后定位 `gh pr create|edit|review`，只在其后找标题与正文旗标
  （--title/-t、--body/-b，空间、等号、连写三形态等价）。
- 取不到的情形：--fill/-f、--web/-w、--title-file/-T、--body-file/-F、无旗标、
  分词失败；旗标值含美元符或反引号属运行时才可知，放行。
- 评审路由：`gh pr review` 与评审接口工具的正文走评审结果模板（骨架、声明核对、
  书写规则），建链改链的正文走正文模板（必需小节、书写规则）；普通评论
  （`gh pr comment`、`gh issue comment`、add_issue_comment）不是评审结果产物，
  不在判定面——全拦会误伤日常讨论。
- 手动测试：`echo '<hook-json>' | python3 .zcode/hooks/check_pr_title.py`
- squash 模拟占位：拼装单点在 scripts/hooks/check_commit_msg.py 的合并模拟
  函数（五位编号占位，最长常见形态），本钩子经它取模拟消息，不再自抄占位。
