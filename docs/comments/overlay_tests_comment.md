# overlay.rs 测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md「注释纪律」节），本文件 crate 级 `//!` 文档属 missing_docs 门禁强制，与 SAFETY 同理保留在代码内；验收意图记录在此。
> L3 显隐自检（harness = false 的端到端测试目标）：自带 main()、运行在进程主线程（winit 约束），经 gloss-app 公共 API 驱动与生产完全相同的窗口栈，需要窗口服务与 GPU。退出码：跑满 100 轮且有延迟统计 0；无帧、首帧超预算、RSS 尾段净增长超预算 1。

## 验收行为（BDD）

- 给定预创建的窗口栈，当反复显隐 100 轮，则每轮 show → 首帧延迟 < 100ms（预创建生效，预算计入门禁）、窗口句柄数不增长（复用而非反复销毁，句柄数进日志供人工走查）。
- 自检期间恒渲染自检卡（视图 None；无失败卡，动作位忽略）。
- 自检渲染传 `Locale::default()`（出厂中文）：量的是显隐与首帧预算，与语言无关——文案表选哪一份都不影响这两项。
- resumed 可能连续投递，窗口栈只起一次；init 失败即退出（无渲染栈带病进循环只会静默空转）。
- 预创建即隐藏（Idle 态），先在隐藏状态画一帧预热——egui 图集构建、Metal 管线编译与纹理上传都发生在首帧，不预热的话首次显示会超预算（理由详见 app_comment.md 的「预热帧」条目）。

## 常量与状态

- 每轮浮层停留时长 SELFTEST_VISIBLE；验收轮数（反复显隐 100 次）；首次显示预算（< 100ms，预创建生效）。
- 运行状态字段：egui 要求的下一帧时间点（None 表示等到有事件再画）、本轮隐藏时刻、当前轮次（1-based）、本轮 show_at 时刻（首帧记录后清空，保证每轮只计一次）。
- 汇总统计：无帧返回 None。

## 性能导出（GLOSS_PERF_OUT，2026-09-21）

- 为什么追加而非覆盖：文件积累成「每运行一行」的趋势序列，供同机回归对照；消费端按行读。
- 为什么写失败只记日志不改变退出码：导出是旁路观测，不能反过来影响门禁语义——预算判定失败仍退出 1、通过仍退出 0，与导出成败正交。
- 为什么只在有延迟统计时导出：无帧失败没有测量数据可审计，该失败由退出码与日志呈现；有测量的预算失败（verdict=fail）照常落一行，失败运行的数字同样可审计。
- 为什么 ts 用 unix 毫秒而不是 ISO 字符串：格式化日历时间要引日期库或手搓历法换算，两者都不值得；毫秒时间戳无歧义且消费端转换零成本。
- 为什么 commit 由 justfile 注入（GLOSS_PERF_COMMIT）而不是测试里 spawn git：测试进程保持「只读环境变量」契约；CI 天然有 GITHUB_SHA 作回退。
- 为什么用 serde_json::json! 而不是 serde derive：记录是一次性扁平对象，json! 免去 derive proc-macro 依赖，转义由 serde_json 处理。
- GPU 名经 Frame::adapter_name → GpuSurface::adapter_name（只读访问器）取自 wgpu adapter get_info；环境三要素 os/arch/gpu 中 GPU 变更对渲染成本趋势影响最大，值得这条几行的通路。
- 为什么导出侧自建父目录（create_dir_all）：CI 直接把 GLOSS_PERF_OUT 指到 runner.temp 的子路径，没有步骤负责建目录，OpenOptions 只能建文件不能建父目录——不自建的话 artifact 恒空且静默；建失败与写失败同级处理，记日志不改变退出码。
- 导出契约（GLOSS_PERF_OUT/GLOSS_PERF_COMMIT 语义）的登记处：crate 级 `//!` 只留一句能力描述，细则以 bdd.md 的 overlay 行与本文件为准，避免三处同源。

## 预算数值的单一事实源（2026-09-21）

- 首帧预算（SHOW_BUDGET）与轮数等数值以源码常量为准，bdd.md 的 overlay 行随改动同步；AGENTS.md 质量条目总表的**性能预算**条目按设计只写普适原则、不写数字，避免第三处同源漂移。改预算数值时同步前两处即可。

## 显形与露面决策测在纯函数层（PR #72 评审，2026-09-27）

`pending_reveal_shows_the_skeleton_only_over_a_live_view` 与
`reveal_decision_combines_the_pending_request_with_the_batch` 直测
`should_reveal`：`drain_events` 的 `&ActiveEventLoop` 参数在 L1 无法构造，
壳层整条「触发即显」路径没有更低的驱动面；断言守卫（视图为空不显形）与
「挂起显形或失败即弹」的取或优先级。测试体内曾有的两行普通注释（「触发
即显把取材成功的露面职责接走了」）按注释纪律删除——该缘由已由生产侧
`should_reveal` / `auto_show_for` 的文档承载，assert 消息「骨架已代为露
面」保留同一语义。

## 显隐自检信号改 stderr 结构化行（T3，2026-10-07）

- 背景：性能指标体系重构（`docs/plans/2026-10-07-perf-metrics-redesign.md` 三 3.0/3.3）废弃 GLOSS_PERF_OUT 文件导出；测试信号改为 stderr 结构化 JSON 行（`kind` 分型：每轮 `overlay_round`、收尾一条 `overlay_perf`），由 `just selftest-report` wrapper 提取落 `target/perf/selftest.jsonl`。
- **export 通道用 eprintln! + serde_json 而不是日志宏**：3.3 示例的行形状（嵌套 env 对象、Option 字段落 null、无日志信封字段）只有 serde_json 能精确表达（tracing 字段值是平面的，嵌套对象与 Option 都记不进去）；clippy.toml 显式放行测试代码的 print_stdout / print_stderr，与「日志统一出口」条目不冲突——该行是数据导出通道不是诊断日志，失败诊断仍走 error!。wrapper 按 kind 字段过滤，与日志行互不干扰。
- **补了 init(None)**：此前 tests/overlay.rs 不初始化订阅者，error! 诊断全部静默（无帧、超预算、RSS 尾段超限都不可见）；现在与 startup.rs 一致 init(None)（只写 stderr 不挂文件层），失败诊断可见。
- **hide 延迟口径**：记 hide() 调用自身的主线程耗时（AppKit orderOut 的同步成本）。进程内观察不到「窗口真正从屏幕消失」的异步确认点（is_visible 在调用返回后即 false，无信息量），调用耗时是唯一诚实的进程内代理信号；每轮 100 个样本取 p50/最差。
- **frame_* 保留决定（任务卡退出条件的评估结论）**：帧间隔在既有 draw() 路径上顺带记录（一次 Instant 相减 + Vec push），不调度额外重绘、不改事件循环时序，对显隐计时口径零干扰——未触发「砍掉 frame_* 只留 hide」的淘汰条件。实测干扰源是 wrapper 的外部 ps 轮询（每次 spawn ms 级 CPU）与机器负载：只影响 wrapper 采集时的 frame_p95 读数（趋势信号、非门禁），门禁判定在测试进程内、不经 wrapper。
- **每轮重置淡入（reset_appear_animation）**：淡入起点存 egui memory，不清除则首轮之后 progress 恒为完成态——每轮只剩单帧，帧间隔样本退化为仅首轮约 4–5 个。生产显形入口（flow::show_overlay）每次显示都重置淡入，自检经新增 `Frame::reset_appear_animation`（pub，跨 crate 供自检用）复现同一行为，每轮淡入期（APPEAR_SECONDS=0.18 > 每轮停留 80ms，淡入不会完成）持续产生帧间隔，样本覆盖全程 100 轮。reveal.rs 同步改走 Frame 方法，消除直接摸 egui_ctx 的第二条路径。

### 延迟终点回归与修复（PR #129 评审，2026-10-07）

- 初版实现把「取时间戳」提前到了 render_frame 调用前（为帧间隔与延迟共用
  一枚时间戳），使 show→首帧延迟只计「显示请求→拿到渲染机会」的调度等待、
  系统性漏计 egui run_ui + tessellate + wgpu present 的整段渲染成本——与
  预算定义的「首帧已渲染」不符，且与既有 Intel 历史趋势点失去可比性（评审
  以 origin/main 旧 draw() 对照指出）。修复：延迟终点恢复为 render_frame
  返回后的 `Instant::now()`（减 shown，同 origin/main 语义，调度+渲染全计）；
  帧间隔保持相邻两帧渲染起点的 start-to-start 口径——两枚时间戳分持，两族
  指标互不牵连。中途试过 `render_started.elapsed()`（只计渲染段）作为终点，
  实测 first_ms 跌到 2.5ms 当场暴露它丢了调度等待——终点必须是渲染完成后
  对 shown 的差值，不是渲染段时长。

### round 标记行与 RSS 采样键统一刻度（PR #129 评审二轮，2026-10-07）

- 初版 round 标记行在 `self.round += 1` 之后发出（1 起始的「进行中轮号」），
  而 rss_curve 采样键在自增之前取值（0 起始的「已完成轮数」0/25/50/75）——
  同名 `round` 两套基数，语义错位一轮（评审指出）。统一为**已完成轮数**刻度：
  标记行移到自增之前发出（0..99），与曲线采样键同刻度同瞬间（标记 25 恰逢
  曲线 25 = 预热边界），尾段门禁锚点与既有 Intel 历史趋势点的口径零变化。
  选统一而不是改名：曲线键、门禁锚点、`round` 字段三个消费面共用一个刻度，
  比「两个名字两套基数」更不易再错位；wrapper 只按 kind 计数不解析取值，
  不受影响。

## 测量与信号（自文件头迁入，2026-10-24）

- 测量形态：预创建窗口反复显隐 100 轮（每轮显示重置淡入，复现生产的逐次显形），统计显示 → 首帧延迟、隐藏延迟（隐藏调用的主线程耗时）、动画期帧间隔、窗口句柄数与进程 RSS 增长（首帧预算、RSS 尾段净增长预算计入门禁——前 25 轮属一次性预热分配，泄漏信号看尾段斜率）。
- 信号走标准错误的结构化 JSON 行（类别字段分型）：每轮一条轻量轮次标记行（其值＝发出时已完成的轮数，0 起始——与进程内 RSS 采样键同一刻度，25 = 预热边界），收尾一条自检汇总行（延迟分位/帧时间/尾段/句柄/预算与判定字段）；完整 RSS 曲线由自检报告脚本在进程外轮询采集，不在本测试内。
- 日志纪律的豁免面：信号行是数据导出通道（报告脚本按字段提取），不是诊断日志——诊断走错误级日志——标准错误结构化行只能直写，故整文件豁免打印类告警（clippy::print_stderr 文件级豁免）。
