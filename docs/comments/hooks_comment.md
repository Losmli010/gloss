# hooks.yml 决策记录

> 按符号锚定（不写 job/步骤名，不写行号，防漂移）。改到相关配置时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## Commit message check 步骤

- 2026-09-18 `--no-merges` 的原因：跳过 merge commit，避免 GitHub 自动生成的 "Merge ... into ..." 被误判。

## commit-msg job：PR 标题纳入校验（2026-09-19）

### 背景

squash 合并时 GitHub 把 PR 标题（追加 `(#N)`）作为 main 上的 commit message。PR #39
「AGENTS.md 质量条目框架重建 + BDD 测试清单 + 新门禁」标题缺 `type:` 前缀：PR 阶段的
Commit message check 只校验 PR 内的提交（base..head），标题不在其列，于是问题在合并
后才由 main 的 push 检查暴露（run 35444915695 失败，job「Commit message check」）。

### 决策

PR 场景下把 `github.event.pull_request.title` 经 `env:` 传入（避免表达式直接内插进
shell 的注入面），送进同一个 `check_commit_msg.py -` 校验；`pull_request` 触发加
`edited` 类型，标题改名后重跑校验。

### 备选与取舍

- **push 模式跳过以 `(#N)` 结尾的提交**：放弃。弱化 main 门禁，且对已合并的坏标题
  无补救价值。
- **main 历史改写（amend 3ff1588 的 message 后 force-push）**：放弃（本轮不执行）。
  改写已发布历史属外向动作，交由仓库所有者决定；即使不改写，下一次 squash 合并以
  规范标题进入 main 后，push 检查即转绿（push 检查只看 `github.event.before..after`
  增量区间）。
- **在 checker 脚本里特判标题场景**：放弃。脚本职责是校验一段 commit message 文本，
  「标题也是未来提交信息」是 workflow 层的编排事实，放 workflow 更贴切。

### 后续

- 修复分支 `ci/validate-pr-title`，PR 合并后 main 自动转绿。
- 若所有者想立即清掉 main 上那次红 X，可改写历史：
  `git checkout main && git pull && GIT_SEQUENCE_EDITOR=: git rebase -i --onto` …
  实操为 `git commit --amend -m "docs: AGENTS.md 质量条目框架重建 + BDD 测试清单 + 新门禁 (#39)" && git push --force-with-lease origin main`。

## PR 标题校验预留 squash 后缀（2026-10-03，#39 同族复发）

### 背景

## 39 补的 PR_TITLE 校验只覆盖了格式维度；长度维度在 PR #100 复发——标题

`feat(brand): 品牌换装宣纸墨朱——图标、强调色与官网色板` 76 字节，PR 阶段校验放行，
squash 合入后 GitHub 追加 `(#100)` 成 83 字节，超 81 上限，main 的 push 检查
（run 37088540818）红叉。本地 ZCode 钩子同一时刻同样放行：它校验的也是入库前的标题
（见 check_pr_title.py 记录）。

### 决策

PR_TITLE 校验串从裸标题改为 `标题 + " (#99999)"`——占位取五位编号的最长常见形态
（9 字节），即真实合并产物的最坏模拟；标题预算从 81 收缩到 72 字节（约 24 个汉字）。
校验脚本本身不动：它的职责是校验一段文本，「标题会带什么后缀」是调用方语境。
2026-10-26 起拼装收敛为校验脚本的合并模拟单点（见 check_commit_msg.py 记录）：
客户端钩子与持续集成作业改调单点取模拟消息，各处不再自抄占位字面量。

#### 备选与取舍

- **checker 上限收到 73**：放弃。commit message 是最终形态、无后缀，不该为标题场景陪绑。
- **按真实 PR 号回填**：放弃。workflow 拿得到 `pull_request.number`，但本地钩子在创建前
  拿不到编号，两边口径会分叉；统一占位模拟保持同构。
- **改用 rebase 合并绕开追加**：放弃。改变主线历史形态，代价大于 8 字节预算损失。

### 密钥作业的引擎安装步骤（2026-10-25）

- 密钥门禁改道 ripgrep 后，规则单测与扫描配方都依赖引擎在场：安装步骤先于单测步骤。
- 版本不写死在 yml：经版本清单小助手拼出「工具名＠钉版本」再交安装动作（taiki-e），与 ruff／tombi 的同读清单写法同源；分成「解析版本」「安装」两步是因安装动作的入参不支持壳层替换，只能借环境变量桥接。
- 运行器自带 ripgrep 但版本随镜像漂移：安装动作的产物目录排在路径前面，钉版本优先生效。

## review-result job：评审结果提交纳入门禁（2026-10-28，#257）

### 背景

正文模板门禁（pr-template 作业）只核对作者声明，评审人对拉取请求（PR）提交的评审
结果（pull_request_review 事件）不受检——评审结果是评审人的正式产物，骨架缺段或
声明缺项的评审照常入库（如 PR #257 的评审走旧式分级骨架，与评审结果模板不符）。

### 决策

- 触发用 `pull_request_review` 的 submitted 与 edited 两类型：评审提交与改评都即核，
  与 pr-template 作业对正文编辑即核的口径成对。
- 空正文、撤销（dismissed）与机器人评审在作业条件跳过：没有评审结果产物，不套模板。
- 既有两作业（text-gates、commit-msg）加事件守卫不随评审重跑：评审事件不引入仓库
  变更与提交，重跑纯文本门禁与提交信息校验是无谓负载；本作业自带检查器单测步骤，
  不依赖 text-gates 的 hooks-test。
- 校验脚本 check_review_result.py 与模板文件门禁共用骨架与声明解析单点，CI 与客户端
  钩子（check_pr_title.py 的评审路由）三处判定同源。

### 备选与取舍

- **复用 pull_request 事件全量重扫全部评审**：放弃。每次推送都重核历史评审，旧格式
  评审永远挡路，且评审编辑事件不触发 pull_request 类型。
- **把评审结果检查并入 text-gates 的纯文本组合**：放弃。纯文本门禁针对仓库跟踪文件，
  评审结果是事件载荷，不在仓库里；两者触发事件与取数路径都不同。
