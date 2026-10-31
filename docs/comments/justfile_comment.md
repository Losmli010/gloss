# justfile 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关配方时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## coverage_min

- 2026-09-18 70 的来历：M3 分层测试（状态机抽出 + pipeline/popup 集成与 harness 测试）落地后总量实测 81%，先前 35 的临时值还账调回 70；本地 `just coverage` 与 CI 的 coverage job 共用同一判定。

## fmt-toml

- 2026-09-18 CI 的 tombi 钉 1.5.5，本地版本以接近为佳——版本差远了会出现本地绿、CI 红的格式判定漂移。

## lint-toml

- 2026-09-18 不带 --error-on-warnings 的缘由：根 Cargo.toml 现有 6 条「表格乱序」风格 warning 待整理；error 级（语法/schema 错误）仍会失败。

## miri

- 2026-09-18 只覆盖 gloss-core 纯逻辑层的原因：Miri 解释执行 MIR、无法执行 FFI，平台层（macOS 框架）与渲染层（wgpu/egui）跑不了。
- 2026-09-18 两处实测裁剪（2026-09-17）：`--skip cache:: / engine::`——moka 背后的 crossbeam-epoch 指针技巧在 Miri 的 Stacked Borrows 下报 UB（tree-borrows 更糟），凡经 MokaCache 的测试整体排除；`-Zmiri-disable-isolation`——放行 config 落盘类测试的文件/环境访问，内存 UB 检测不变。
- 2026-09-18 CI 的 sanitizers.yml 跑同一条配方；Miri 只随 nightly 发布、上游只对最新 nightly 测试，跑挂了先跟着升 nightly。
- 2026-09-19 第三处裁剪 `--skip file_writer_persists_lines_into_daily_file`：该测试断言「WorkerGuard drop（shutdown + join）后日志行已落盘」，语义本身确定，但 tracing-appender 的非阻塞 worker 线程在 Miri 解释执行 + libtest 并行下偶发不落盘——CI 实测约半数轮次以「文件内容为空」失败（8bda13c/5b43efc/bc57b29 三轮失败，4502ca0/d4c6745 等同代码轮次通过；本地单测 Miri 单线程复现不出）。疑似 worker 在 Miri 的 IO 转发下失败后 channel 断开、shutdown send 失败跳过 join，无法从外部观测确认。与 crossbeam-epoch 裁剪同一先例：上游组件在 Miri 下不可靠即裁剪并记录；该测试在常规测试（非 Miri）里保持全绿，刷盘契约的覆盖不丢失。

## precommit

- 2026-09-18 不含 test：测试由 CI 跑，本地提交不必等编译测试。
- 2026-10-25 重组为 `text-gates + lint`（PR-S2）：依赖经中间配方垒叠，不再平铺手抄十二个配方名；集合与旧清单相同（约束、文档引用、行为清单、文案表、注释纪律、五条格式检查、Clippy、密钥扫描），只把成本最低的密钥扫描提到最前——密钥问题不再等全量跑完才暴露。

## text-gates（2026-10-25，PR-S2）

- 纯文本门禁组合：六条纯文本配方（secrets、agents-doc、test-bdd、constraints、i18n、comments）加五条工具格式配方（fmt、fmt-py、lint-py、fmt-toml、lint-toml），密钥扫描前置。
- `fmt` 归文本层的缘由：cargo fmt 只解析语法树不编译、秒级——把它留在编译层会让无源码提交走全套编译链；归入文本层保 precommit 组合对现状只增不减（旧 precommit 本就含 fmt）。
- just 依赖按声明序串行执行，配方的先后即成本序；此配方是内部中间层，不进 AGENTS.md 常用命令节。

## hooks-test（2026-10-25，PR-S2）

- 检查器单测入口：`{{py}} -m pytest scripts/hooks .zcode/hooks -q` 目录发现——比 CI 逐作业点名单文件更能防新测试文件漏跑；`.zcode/hooks` 现无测试文件，pytest 目录发现对空目录收零条不报错。
- 进 `check` 不进 `precommit`：单测非秒级，提交钩子不加这份等待。

## check

- 2026-10-25 重组为 `precommit + hooks-test + test`（PR-S2）：比旧清单净增检查器单测，其余集合不变；AGENTS.md 常用命令节描述同步。

## py 变量（2026-10-25，PR-S2）

- `py := "python3"`：全部脚本配方经此变量调用，统一「./x.py 直调」与「python3 x.py」两种形态；解释器升级（本地 3.8 → 3.12）时只翻这一处。连带效果：justfile 不再按路径直调任何脚本，install_hooks.py 的执行位清单随之缩到仍被按路径直调的文件（钩子两件＋工作流直调的提交信息与发布、性能脚本）。
- 2026-10-25 翻到 `python3.12`（PR-P2）：消除本地系统 3.8 与远端 3.12 的判定分叉，注释门禁实测提速约四成；入口名与版本清单的解释器下限同值，环境初始化按下限同名派生校验（见 setup_dev_comment.md）。`.zcode/config.json` 的钩子解释器调用随同翻名，避免第三处分叉。

## selftest-report

- 2026-09-21 commit 由配方注入（`git rev-parse`）而不是测试进程自己 spawn git 的原因：测试保持「只读环境变量」契约，也避免非 git 环境里测试行为分叉；out 落 target/perf/ 缺省值，随 cargo clean 一起清掉，长期趋势文件应由使用方指定仓库外路径保存。
- 2026-10-07 改造为 wrapper（T3）：GLOSS_PERF_OUT 文件导出废除，配方改为调 scripts/perf/selftest_wrapper.py 跑双自检（overlay + startup）、从 stderr 提取结构化信号行并外部轮询完整 RSS 曲线，落 target/perf/selftest.jsonl（决策细节见 selftest_wrapper.py_comment.md）。commit 注入改由 wrapper 执行，2026-09-21 的「测试只读环境变量」契约不变；out 缺省值随 cargo clean 清掉的口径不变。

## package-macos

- 2026-09-18 ad-hoc 签名免费可跑，用户首次打开需右键 → 打开；上 Developer ID 后把 `codesign -` 换成正式身份并接 notarytool。

## agents-doc

- 2026-09-18 为什么有这道门禁：指令文件是唯一会被逐次加载的文档，它描述的世界一旦过期，代理就照着错的信息干活。

## bench

- 2026-09-19 按名选取（cargo bench --bench core）的缘由：根包将来若有第二个 bench 目标（如自检类迁入），按名选取保证 just bench 恒等于「量化热点」这一件事；本地量化配方，不进任何 CI workflow（2026-09-20 按评审核实全部 workflow 后修正：先前误写「CI 上只跑不判趋势」）。
- 2026-09-21 基线工作流：bench 每次存滚动基线 last（--save-baseline），criterion 默认行为保持「对照上一次运行」；命名基线作审计锚点（bench-save，如在干净 main 上存 main），bench-check 用 --baseline 指名对照。基线只落在本机 target/，不进 git 也不进 CI——跨机器与跨负载的数字会互相污染，趋势对照只认同机同环境的历史数据（AGENTS.md 测试节）。
- 2026-09-21 对照判定不设退出码：bench-check 与 bench-summary 的「回归/改善」只作量化参考，判定列口径见 bench-summary.py_comment.md；不把它接进 precommit/CI，与「基准是量化工具不是门禁」同一口径。

## clone-stats / clone-baseline / clone-check

- 2026-10-05 放 dev 桶 bench-summary 旁：同为度量/审计类配方；统计口径与判定细节见 clone-stats.py_comment.md。
- 2026-10-05 与 bench 基线分居两处的缘由：基准数字跨机器不可比，基线只落本机 target/；clone 计数是文本确定值，跨机器可比，基线随 git 走（scripts/baselines/），变更走 PR 审查。
- 2026-10-05 clone-check 有退出码（上升即 1）但不进 precommit/CI：退出码供改动者自查与将来按需接线，是否收紧留给后续决定。

## 删除 changelog / changelog-preview 配方（2026-09-30）

- 缘由：发版说明改以 GitHub Releases 页为**唯一来源**（用户决定），仓库不再
  维护 CHANGELOG.md 文件——本地再生成一份与 Release 页构成双源，必然漂移。
  cliff.toml 保留（它只是渲染模板，CI 打 tag 后以 `git cliff --latest` 渲染
  当前 tag 的一节作 Release 正文，见 release_comment.md）。
- 2026-10-05 `clone-bench` 与 `bench` 分立而机制同源：两个 bench 目标的测量量不同
  （clone 基准是自定义分配计数，core 基准是墙钟），criterion 的滚动基线 last 按
  基准目录各自独立，互不污染；clone-baseline 的性能段从 criterion 的 new/ 结果
  读取（与 bench-summary.py 同一路径约定），`just clone-bench` 不跑时基线退化为
  纯计数并提示。
- 2026-10-05（归拢）性能脚本与数据集中 `scripts/perf/`，配方随路径调整；
  clone-baseline/clone-chart 两配方折叠为 `perf-baseline`（clone 基线 + core
  快照 + 双图重绘一条链），clone-check 保留专名——它是带判罚语义的入口，
  与「只重建」的 perf-baseline 分开；bench-summary 移位不改名（AGENTS.md
  引用的是配方名）。
- 2026-10-05（PR #109 审查）perf-baseline 注释补「clone 侧需先跑 just
  clone-bench」：链首 write 有降级防护，但前置条件写在入口注释才拦得住人；
  core 侧同款说明先前已有。

## 2026-10-07 新增 profile-run / profile-cpu（P3-1 火焰图配方）

- 采样工具选型：samply 与 cargo-flamegraph 本机都未装（零安装约束），落 macOS
  自带 `/usr/bin/sample`——样本报告即调用树，渲染由 profile_cpu.py 内置 SVG
  输出，不引任何第三方依赖。samply 留作后续可选增强，不进门禁也不进 precommit。
- profile-run 与 profile-cpu 分立：剖析对象必须带符号，而 release
  `strip=true` 采出来全是 `???`；profile-run 负责以 profiling 配置启动带符号
  实例，profile-cpu 只管「采样 + 渲染」，对什么实例在跑保持中立（--pid 可指）。
- `profile-cpu pid="" duration="10"`：pid 空串传给脚本由脚本侧回落 pgrep，
  不在 justfile 里做条件拼接（保持配方单行）。

## 2026-10-06 flame / flame-startup 替代 profile-cpu（P4-1，profile_cpu.py 删除）

- 用户决定换 cargo-flamegraph 全家（inferno 折叠/渲染 + dtrace 采样），
  profile_cpu.py 的 239 行自研解析/SVG 渲染随之删除。crates.io 包名是
  `flamegraph`（装的二进制才叫 cargo-flamegraph），setup-dev 提示按包名写。
- `flame`（attach）与 `flame-startup`（launch）分立：cargo-flamegraph 不支持
  attach 到既有 pid，attach 交给 scripts/perf/flamegraph.py（dtrace 采集 +
  inferno 链），launch 交给 cargo-flamegraph 本体；两者覆盖的采样窗口互补，
  缘由详见 flamegraph.py_comment.md。
- `flame-startup` 带 `--root`：flamegraph 0.6.14 源码确认 macOS 的 dtrace
  只在显式传 `--root` 时才包 sudo——不传则以无权限直接失败；`--root` 只
  提权 dtrace 进程，gloss 本体仍以普通用户运行，AX/TCC 授权不受影响
  （整条命令跑 sudo 会让构建产物变 root 属主，不可取）。cargo-flamegraph
  无 --timeout（--help 实证），launch 采样持续到目标退出，注释写明
  「采完退出 gloss 即出图」。
- dtrace 采样需 root：`flame` 在交互终端跑（sudo 提示输密码），无人值守
  环境走失败分支给指引——这是相对 sample 方案多出的成本，接受（用户决策）。
- 2026-10-07（P3-3）新增 `size-report`：与 profile-run/profile-cpu 同款
  分立思路——节级任何产物都能跑（strip 不影响 size -m），符号级需要未
  strip 产物（profiling/dev），脚本内检测符号表并就地提示，不在配方层分叉。

## 2026-10-08 perf-baseline / perf-chart / app-metrics / clone-check 重配线（T6）

- perf-baseline 三段旧链（clone-stats --write → bench-baseline.py → perf-history.py）收敛为
  perf-collect → perf-chart 两步：按源格式搬运链已随 T2 删除，配方只保留「聚合本地产物为一个
  趋势点 + 重绘派生视图」；入口注释列各源前置配方（clone-bench / bench / selftest-report /
  app-metrics），某源缺席对应指标整体跳过、不判罚（collector 的缺源即缺段语义）。
- 新增 perf-chart 配方：只重绘三张趋势 SVG，不与采集绑死——图表是 history 的确定性派生视图，
  数据修订或脚本修复后可单独重绘，重绘本身不产生数据。
- app-metrics 链尾从 perf-history/perf-chart 换成 perf-collect/perf-chart：app 采样产物是
  target/ 本地 v2 片段不入库，入库路径唯一经 collector（不留绕过采集直写图表的暗道）。
- clone-check 注释改指 history 最新点基线并注明前置（clone-bench + perf-baseline）：旧注释的
  「core 墙钟仅记录不判罚」描述的是已删除的旧对照行为，clone-check 自 T2 起只对照 clone 三项。

## fmt-md / fmt-md-fix / lint-md

- 2026-10-26 新增三条标记语言（Markdown）配方（排版工具 rumdl）：文档结构与排版
  （标题递进、重名标题、围栏闭合、空行、列表、表格）从注释门禁的结构三罚移交工具
  强制，工具侧可自动修复是主因；受检面为版本库跟踪的全部 `.md`（`git ls-files`
  圈定，与 fmt-toml 同构），提示词目录按用户决策一并进排版面。
- 关闭的两条规则（`MD013` 行长、`MD041` 首行一级标题）缘由住 rumdl.toml 注释与
  rumdl_comment.md：中文无空格折行行长无意义；提示词与拉取请求模板首行非标题是
  有意为之。
- text-gates 组合从五条工具格式检查扩到七条（fmt-md、lint-md 入列）：提交钩子与
  CI 钩子工作流经组合配方自动覆盖，不另设旁路。

## text-gates（并轨入口）

- 2026-10-26 成员变更：`fmt-md`、`lint-md` 入列，七条工具格式检查；六条纯文本
  门禁成员不变。

## pr-lint / pr-lint-draft

- 2026-10-31 两配方分立而判罚单点同源（收口计划阶段 4）：都只经
  check_pr_prose.py 判定，与钩子工作流的 PR content check 同一脚本，不复制判
  罚口径；脚本形参正文在前、标题在后，配方签名按计划为标题在前、正文在后，
  参数序翻译落在配方体内。拆两个入口而不给 pr-lint 加开关，是两个入口语义
  各自单一——pr-lint 面向已存在的线上 PR 全量复检（经 gh pr view 取标题与
  正文），pr-lint-draft 面向创建前的草稿文件，互不掺条件。
- 2026-10-31 pr-lint-draft 定位是本地软闸：git 钩子覆盖不到 `gh pr create`
  动作，创建前拦截只能靠流程约定加工具，红灯不拦提交；CI 侧 PR content
  check 已是必需检查，作硬闸兜底。软闸定位写进配方注释，创建流程（起草 →
  预检 → 创建 → 复检）的 AGENTS.md 登记归收口计划阶段 6，不在阶段 4 提前改。
- 2026-10-31 参数序按计划签名标题在前、正文在后，配方体内按脚本形参翻译成
  正文在前；与 CI 同款违规清单的等价性由 check_pr_prose_test.py 的配方契约
  测试锁定（字节级对齐直调输出），防参数序翻转这类无其他测试能暴露的错。

## default（自检速查行，2026-10-31）

- 两档自检分工（precommit 快回路、check 全量）此前只住在文件头注释与桶结构
  里，`just --list` 的平铺输出看不到文件头——agent 与新人最常卡的是「改完代码
  按什么顺序跑哪些命令」。把速查放进默认输出而非另写文档：索引输出即第一
  入口，命令自描述优先。

## coverage / coverage-check / audit / deny / miri（CI 必跑标注，2026-10-31）

- 五条配方注释统一补时机。实跑清单核实：ci.yml 调 `just coverage`、
  `just audit`、`just deny`，sanitizers.yml 调 `just miri`；coverage-check
  不在任何工作流调用清单，只补「本地按需」。
- 缘由：编译门禁桶的桶头「本地与持续集成共用同一配方」说的是配方复用，不
  回答「哪些在提交回路里」；`just --list` 按字母序平铺、桶结构不可见，
  precommit 与 check 的依赖组合才是提交回路的成员清单。注释自带时机让每条
  配方可独立纠偏，agent 不会把慢门禁当成提交前必跑。

## install-hooks（关系注明，2026-10-31）

- setup_dev.py 收尾经 `just install-hooks` 装钩子，setup 即包含本步；配方
  保留供单独补装——chmod 与 git config 均幂等，可重复运行。两条入口并存时
  agent 需要知道该跑哪个，注释写明包含关系消歧，不合并入口（保留最小动作
  单元）。

## eval（触发权语义，2026-10-31）

- 原注释「显式选择参与」没写谁有权选择，agent 可能据「参与」自行触发；实跑
  轨有真实凭据与真实计费，触发权收归人工——注释补「仅人工明确要求时运行」。

## profile-run（退出语义，2026-10-31）

- 行尾 `&` 让配方随即以退出码 0 返回，应用是否存活与退出码脱钩；agent 拿到
  绿色退出码易误判「验证完成」。注释补「配方随即返回，应用存活与日志经日志
  跟随（logs）观察」，验证语义交给日志而不是退出码。
