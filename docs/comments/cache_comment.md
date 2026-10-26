# cache.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关符号时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## cache_key（2026-10-10，剪贴板图片方案 §5.5）

- 为什么从「`serde_json::to_vec(&(input, options))` 整体序列化再哈希」改为「输入按变体直接喂 `DefaultHasher`」：图像字节经 serde_json 会展开成 JSON 数字数组（每字节 1–3 字符 + 分隔符，约 4 倍体积）的临时缓冲，1–4 MB 的剪贴板图片在每次触发时都要付一次分配+拷贝；流式写入 SipHash 后哈希路径零中间缓冲，`benches/core.rs` 的 `cache_key` 组 `image/1mb`、`image/4mb` 用例（#42 引入）可直接对照前后成本。
- 为什么用 `std::mem::discriminant` 做模态区分而不是给变体手写标签字符串：`Discriminant` 的 `Hash` 实现就是枚举变体序号，语义即「模态不同则 key 不同」，不引入可拼错的魔法值；`TaskInput` 加新变体时此分支自然覆盖。
- 为什么 `region: Option<ScreenRect>` 直接 `Hash`：剪贴板来源 `None` 与框选来源 `Some(rect)` 必须不同 key（同图不同上下文，UI 角标不同）；`Option<T>` 的 `Hash` 对 `None` 有独立路径，天然参与。为此给 `core` 的 `ScreenRect` 补了 `Hash` derive——纯整数字段、与 `Eq` 同构的结构化哈希，加法式改动，无其它消费方受影响。
- 为什么 `Audio.duration_hint` 用 `f32::to_bits`：`f32` 不实现 `Hash`；位形态对同值稳定且全序可分辨。副作用（有意接受）：旧实现里 NaN 触发「serde 序列化失败 → Debug 文本兜底」的回退路径，对新实现不再可达——NaN 按位形态直接哈希，行为（同值同 key、与有限值可分辨）由改名后的测试钉住。
- 为什么 options 仍走 serde：`prompt_locale` 参与缓存 key 的口径依赖「选项字段全量参与」，serde 序列化免手写字段清单、加字段自动纳入；`TaskOptions` 字段均为可直接序列化的标量，`to_vec` 的 `Err` 分支只为 `Result` 完整性保留（`Debug` 文本哈希对同值稳定）。若未来 options 出现不可序列化字段，该分支兜底，测试应同步补回退路径用例。
- key 数值与旧实现不兼容（有意接受）：摘要只要求进程内稳定（moka 内存缓存、无持久化），换派生方式即全量换 key，无迁移问题。

## MAX_ENTRIES(2026-10-16,内存优化方案)

- 为什么从 256 收到 64:缓存是进程内存常驻项,条数上限直接钉住最坏总量;缓存只在会话内重复触发同一输入时受益(划词同文本二次触发),64 条的近期窗口对命中率边际收益趋零,LRU 逐出保住热条目。

## DEFAULT_TTL / MAX_ENTRIES

- `DEFAULT_TTL`：缓存条目存活时长：任务产物在会话内重复触发收益明显，超过后过期腾位置。`pub(crate)` 供 gloss-core `Config` 的出厂默认用例对齐——`Config::cache_ttl_secs` 的注释声称与此一致，两侧都写字面量时改一边不会有人红。
- `MAX_ENTRIES`：容量上限（条目数）：词条卡/翻译卡体积小，64 条覆盖近期高频场景，超出由 moka 按 LRU（TinyLFU）逐出——缓存是进程内存的常驻项，条数上限直接钉住它的最坏总量。

## hash_input

- 输入的变体化哈希：模态以 `discriminant` 区分，载荷逐字段喂入。

## TaskCache

- 任务产物缓存：线程安全的 moka 内存实现，`get`/`set` 可从任意线程调用（tokio 侧组装、主线程无需访问）。
- `new`：默认缓存：1 小时 TTL、64 条上限。
- `with_ttl`：自定义 TTL（生产由配置换算注入；测试用短 TTL 验证过期路径）。
- `get` / `set`：取/写缓存产物。
- `run_pending_tasks`：强制处理逐出/过期（测试同步过期判定用）。
