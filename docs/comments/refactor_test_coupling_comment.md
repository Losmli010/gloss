# 测试 mock 与源码强耦合重构准备记录

分支：`refactor/test-mock-decoupling`（自 main 1d7d952 切出）。
本文是重构前的全仓测试审计结论与整改方案，供后续 PR 引用。行号为审计时快照，动手前以代码现状为准。

## 审计方法

两路并行扫描 gloss-app / gloss-core / gloss-platform 全部 `#[cfg(test)]` 模块，判据取 AGENTS.md“测试”节：断言可观察契约、从公共 API 驱动、同步点用事件不用时间。关键结论已逐条抽查证实（clipboard 真机测试、hotkey 私有字段、cache sleep）。

## 全局面

- **没有测试手搓端口 fake**：`ports.rs::mocks` 与 `engine/mock.rs` 的桩被正确复用（config_handle、llm、app、pipeline、tests/pipeline.rs）。规范“别在测试里手搓 fake”在执行。
- 仅三处必要自建：llm.rs `VecStream`（core 无字节流桩）、storage/mod.rs `MemorySecrets`（platform 本地 trait）、composite.rs 闭包（边界情形）。
- **桩冗余**：`ScriptedEngine` 能力是 `MockEngine` 子集且只有 ports.rs 自测在用；`delta_stream` 与 `ChunkStream`、两份 `lock_or_recover` 重复。
- **mock 基建长在生产源码树**：`engine/mock.rs` 与 `ports::mocks` 以 `cfg(any(test, feature = "test-util"))` 门控；`test-util = ["dep:tokio"]` 把 tokio 拉进 core 特性图。

## P0 门禁层（先修，影响每次 cargo test）

1. **clipboard.rs 四个未 ignore 的真机测试**（`fallback_read_restores_original_clipboard` 604、`multiflavor` 623、`empty` 655、`multi_item` 675）：默认套件即驱动真机剪贴板 + Cmd+C 按键注入，单测最长真实阻塞 2×2s。修法：全部 `#[ignore]` 迁入 live 集（对照同文件 912/927 已 ignore 的两条与 keychain.rs 写法）。
2. **时间同步点换事件**：
   - cache.rs:161 `sleep(120ms)`（moka 无时钟注入，可加大裕量并标注，或评估 moka test 特性）；
   - events/mod.rs:381、462 `sleep(60ms)` → 通道事件收敛；
   - app/src/pipeline.rs:267/282 `with_chunk_delay(150ms)` + `recv_timeout(400ms).is_err()` 时间窗 → 以取消事件/流终结为同步点。
   - mock.rs:216 `Instant` 下界断言方向安全（只松不红），可保留。

## P1 mock 基建整合（用户点名部分）

**已落地（2026-09-19）**——安置方案拍板为第三条路：桩分别迁入各自
crate 的 tests/stubs/（cargo 集成测试目录，只在 `cargo test` 编译，
天然不进生产构建），并按被替身的对象分文件（`engine.rs` / `ports.rs`，
`mod.rs` 仅留模块文档、锁工具与 re-export），`test-util` 特性与 core
对 tokio 的可选测试依赖整体删除。

- 两个编译上下文共享同一份桩源：集成测试目标经 `mod stubs;` 引入；
  `src/` 内联单测经 `lib.rs` 的 `#[cfg(test)] #[path = "../tests/stubs/mod.rs"]`
  包含为 `crate::stubs`（src 内联单测无法直接 import tests/ 目录，这是
  cargo 的硬约束）。core 侧桩文件以 `gloss_core::` 绝对路径引用库条目，
  lib 侧靠 `#[cfg(test)] extern crate self as gloss_core;` 让自引用成立。
- 桩按 crate 自持、跨 crate 刻意不共享：core 留全量（MockEngine +
  FixedSelectionReader + FixedRegionCapture + MemoryConfigStore +
  MemoryCache + RecordingHotkeyBinder）；app 留 MockEngine（全量注入面）
  - MemoryConfigStore + RecordingHotkeyBinder；platform 留
  MemoryConfigStore。engine.rs 跨 crate 逐字一致；ports.rs 为同名桩
  子集，模块文档互注来源。
- `ScriptedEngine` 与 `delta_stream` 删除（能力为 MockEngine 子集，
  除 ports.rs 自测外零使用者）；`ChunkStream` 并为唯一流实现，
  `lock_or_recover` 提到桩库 mod.rs 单份。
- 桩自测迁至 `crates/gloss-core/tests/stubs_behavior.rs`（12 条：原
  两处内联桩自测合并 + 补 failure_once / panic 注入两条缺口覆盖），
  缘由与速览见 `docs/comments/stubs_behavior_tests_comment.md`。
- 桩文件按生产代码对待（AGENTS.md“注释纪律”条款已同步改写），
  文件级 `#![allow(dead_code)]`——桩是按需取用的能力全集，各编译
  目标只用到其中一部分。

## P2 私有面直连改造（按严重度）

| 文件 | 症状 | 方向 |
| --- | --- | --- |
| events/hotkey.rs | 私有 `table`/`registered` 字段直读（353-356、446-455 等）、`with_manager` 后门（62）、`Arc::ptr_eq` 内部共享断言、按字段构造 `GlobalHotKeyEvent`（518-537） | 把“降级保留解析表”“泵共享表”升为可观察契约（只读快照访问器或经事件通道断言）；后门删除 |
| events/mouse.rs | 生产代码埋 `#[cfg(test)]` 常量（45-52）与 `subscribed_mask` 后门（224/286）；私有 `GestureDetector`/`classify` 直测 | 删后门；纯逻辑抽出或走 pub 入口；掩码断言改“订阅面”可观察行为 |
| ui/settings.rs | `draft`/`api_key`/`clear_key` 字段直写后门（403-459）、私有 `build_save` 直调、密钥不进 config 的断言绑 Debug 格式（427-430）、出厂绑定条数 3 硬编码（578-582） | 写侧改走 `draw()` 交互路径（harness 测试 503-572 是正路示范）；Debug 断言换结构化断言 |
| app.rs | 私有 `GlossApp` 无公共 seam，字段直读/直写 20+ 处、私有函数直测 10+ 处（882-1524） | 结构性最大项：从 winit `ApplicationHandler` 拆出可构造的状态核心，或明确白盒边界并收敛后门；keychain 条目名 `"gloss/deepseek"`（1248 等）改引 `keychain_id_for()` |
| ui/fonts.rs | 私有 `append_fallback`/`FONT_NAME` 直测、`find_cjk()` 依赖宿主机中文字体（CI 风险） | 字体候选表参数化；环境依赖测试加前置检查或 ignore |
| gpu.rs | 私有 `pick_alpha_mode` 直测、错误文案子串断言 | 纯函数给 seam 或并入集成面；文案断言收敛 |
| core/engine.rs | 私有 `parse_structured` 直测 3 处（331/348/375）；用真 moka 而非 `MemoryCache`；围栏字面量 6 处 | 走公共 `execute` 注入脚本断言产出；换 `MemoryCache`；围栏改引 `STRUCTURED_FENCE` |
| engine/sse.rs | 私有 `decode_chunk` 直测 4 处、私有常量 `MAX_LINE_BYTES`/`MAX_DIAGNOSTIC_CHARS`、诊断串复制 | 走 `SseDecoder::push`（237-300 的性质测试是范本）；常量提为可观察契约或测试内推算 |
| engine/llm.rs | `resolve_endpoint`/`resolve_api_key`/`chat_request_body`/`map_failure`/`SseStream` 全私有面（根因：公共 execute 无缝） | 端点/错误映射抽纯函数模块（pub(crate) 显式声明为契约面）或本地服务器集成 |
| selection/composite.rs、accessibility.rs | 全部测试打私有 `combine`/`interpret`，公共面零覆盖（根因同上：绑死真实 AX/剪贴板） | 读源参数化（接受 reader 闭包表/解释函数），公共组合行为才可测 |
| storage/keychain.rs | 私有 `cached_map()` 注入缓存态（152-153） | 走公共 API 造态 |
| log.rs | 私有 `FILE_PREFIX`/`build_filter`/`open_file_writer` 直测、`EnvFilter::to_string()` 精确渲染断言（钉第三方 Display 格式） | 断言改行为面（过滤结果而非渲染串） |
| prompt.rs | 私有构造器直连、整串 JSON 精确断言（钉 serde 字段序）、模板措辞逐条复制 | `serde_json::json!` 结构比较；模板断言只钉语义锚 |
| config.rs / storage/mod.rs | `60*60` TTL 三处独立对账（config.rs:362/430 + storage/mod.rs:534）、出厂默认字面量复制 | 统一常量引用；`DEFAULT_TTL` 口子去留随 P1 拍板 |

正面样本（重构时保持该形态）：config_handle.rs、model.rs、appearance/icon.rs、channel.rs、machine.rs、events/mod.rs 主体、sse.rs 性质测试、storage/mod.rs 主体、tests/pipeline.rs 主体。

## P3 魔法常量统一

- `"gloss/deepseek"`：五文件七处 → `keychain_id_for()` / config 常量。
- ```` ```gloss ````：六处 → `STRUCTURED_FENCE`。
- `"chat/completions"`、`"com.apple.pasteboard.clipboard"`、`"sse line exceeds limit"`、`"HTTP {status}: {message}"`：测试复制源码/第三方字面量，各归其位。
- machine.rs 测试用 `ALL_KINDS[0..2]` 位置索引表达语义（558-590）——重排常量即静默变义，改按 kind 名取值。

## 建议动工顺序

P0（两个 PR 可独立：live 测试门控 / 时间同步点）→ P1 双桩合一（含方案拍板）→ P3 常量统一（低风险先行，顺带消掉 P2 一部分）→ P2 按 hotkey → mouse → settings → engine/sse/engine → app.rs 收尾的顺序逐文件改，每文件保持“测试红=行为变”的标准。

每个文件改完同步更新 docs/tests/bdd.md 与 docs/comments 对应 `*_tests_comment.md`（外置缘由与形态说明）。
