# stubs_behavior.rs 测试外置记录（tests/stubs_behavior.rs）

锚定对象：`crates/gloss-core/tests/stubs_behavior.rs`。随桩库 tests/stubs/（按被替身对象分 engine / ports 两文件）从
`ports::mocks` 与 `engine::mock` 迁移而来（原内联桩自测合并于此），
并新增两条缺口覆盖。

## 豁免缘由

- 文件级 `#![allow(clippy::expect_used, clippy::panic)]`：桩自测直接
  沿用桩的断言风格——预置值用 `expect` 拆包、注入失败路径用 `panic!`
  反证；失败即停是测试语义，不引入降级写法。等价豁免此前以函数级
  `#[allow]` 形态存在于 ports.rs 与 engine/mock.rs 的内联测试。

## 测试行为速览（给定 / 当 / 则）

- chunk_delay_paces_the_stream：给定注入 30ms 延迟与三个 chunk 的引擎，
  当消费整条流，则 chunk 依序吐出且耗时至少跨过两个 chunk 间延迟。
- failures_are_injectable：给定分别注入四种 `GlossError` 的引擎，当
  execute，则错误原样浮出；给定脚本中段插 `Err` 的引擎，当消费流，
  则失败按序出现且流不提前终结。
- execute_failure_once_fails_exactly_once：给定注入一次性失败的引擎，
  当连续两次 execute，则仅第一次返回注入错误、第二次恢复产流。
- execute_panic_fires_on_first_poll：给定注入一次 panic 的引擎，当在
  tokio 任务里驱动 execute，则任务以 panic 收场（tokio 桥捕获语义的
  注入侧对账）。
- call_count_tracks_execute_invocations：给定带脚本的引擎与克隆体，
  当各执行一次任务，则克隆体共享同一计数，计数为 2。
- empty_script_yields_empty_stream：给定空脚本引擎，当消费流，则立即
  正常终结。
- selection_reader_mock_returns_presets：给定预置成功/失败的读取桩，
  当 read，则原样返回预置结果。
- region_capture_mock_returns_png：给定预置 PNG 的截图桩，当 capture，
  则原样返回同一缓冲（指针相等）。
- config_store_mock_round_trips_secrets：给定空存储桩，当写入再读密钥，
  则读回一致。
- config_store_mock_round_trips_document：给定空存储桩，当保存一份
  非默认配置再 load，则整份文档往返一致。
- cache_mock_stores_and_isolates_keys：给定空缓存桩，当写入 key=1，
  则 key=1 可读回、key=2 不可见。
- hotkey_binder_mock_records_every_call：给定记录型热键桩，当依次
  rebind 两份绑定表与空表，则每次调用计数递增、last 恒为最近一次、
  空表返回生效数 0。
