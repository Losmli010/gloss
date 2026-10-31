# 重构记录：core/platform 职责收紧与 app 分层重组（refactor/core-llm-app-layering）

计划全文见 `docs/plans/refactor-core-platform-app.md`（本机文档，不进 git）。
本文记录实施中相对计划的偏差与取舍缘由，按主题锚定。

## LLM 传输为何进 core（engine/llm+sse）

- 用户定策：llm 与 log 归 core，不新增 crate。LlmClient/SSE 是任务 AI 层的
  执行末端，与 `AiTaskService` 同层；`engine.rs`（编排）与 `engine/{llm,sse}.rs`
  （传输适配）同模块共存（2018 edition 合法形态）。
- 代价：core 引入网络依赖（reqwest/bytes）。check-constraints 的 core 红线
  无需放宽——reqwest/bytes 本就不在禁入清单，红线继续禁止渲染/窗口/系统 UI/
  存储栈（objc2/security-framework/toml/directories 等）。

## 密钥与配置存储为何整体留在 platform

- 文件系统与 keychain 都是 OS 服务，归平台适配层；core 只持 `ConfigStore`
  端口 trait，`LlmClient` 经 `Arc<dyn ConfigStore>` 每请求直查密钥，
  “密钥来源唯一：只从配置存储读入内存”的路径不变。

## eval 依赖边变化

- eval 对 platform 的唯一消费就是 `LlmClient`，随平移改走 core 后 platform
  依赖整体移除；check-constraints 边表保留 eval→platform 作为允许边（未删，
  留给未来 judge 轨可能的平台需求），AGENTS.md 已同步为“只消费 core”。

## app 分层：目录即分层

- `runtime/`（pipeline/cache/finalize，后台桥）· `present/`（windows/gpu/
  render/workspace）· `flow/`（probe/task/reveal/actions/session/
  settings_session）· 壳 `app/`（events/handler/GlossApp）· 决策 `machine` ·
  契约 `channel` · 视图 `ui`（i18n 并入）· 特性 `update/` 不动。
- 依赖规则：壳→编排→决策/呈现→视图；runtime 不碰主线程类型；ui 只读视图
  模型、上交动作。跨层可见性用 `pub(crate)` 显式标注（拆分期间曾临时放宽，
  神对象拆完后即构成最终形态）。

## 露面策略为何进 machine 而非 flow

- `should_reveal`/`auto_show_after`/`EventKind` 是零 IO 的纯决策，与
  `trigger_decision`/`error_action` 同类，归决策层（machine.rs）；
  “怎么显示”（show_position/显隐出口）留 `flow::reveal`。Placement 是
  present 类型，show_position 因此不进 machine——决策层不依赖呈现类型。

## Workspace 为何是包装而非“WindowManager 吸收帧”

- 计划原文是 WindowManager 吸收 Frame/repaint/theme。实施时改为独立的
  `Workspace` 所有者包装 WindowManager：`build_window_stack` 公开接口
  原样保留（根包 L3 自检 tests/overlay.rs 直接消费），WindowManager 的
  窗口职责不被渲染帧状态稀释。呈现资源“唯一所有者”的目标不变。

## GlossApp 的最终形态

- 六个所有者：`workspace`（呈现资源）、`endpoints`（四通道端点）、
  `machine`（任务决策）、`session`（划词锚点/排查线索/显形挂起/任务 span）、
  `env`（config/store/scene/system_locale/update，`locale`/`target_theme`
  随迁）、`settings`（设置编辑草稿，未包新类型——单一字段单一职责，
  包一层无增益）。计划写的“五个所有者”未把 settings 草稿计入，
  实际为六字段组合根。

## 门禁脚本的两处随动

- `check_i18n.py`：装入点 OWNER 随 i18n.rs 移入 `src/ui/i18n.rs`。
- `check_constraints.py`：零改动（reqwest/bytes 不在 core 红线清单；
  依赖边表、tracing 条款、eval 隔离、桩对齐全部原样）。
