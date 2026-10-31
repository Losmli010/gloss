# ffi 模块决策记录

> 按符号锚定（不写行号），改到相关代码时同步更新。本文件是“为什么”的落点：
> 接口契约与不变量写在代码注释里，决策、备选方案与已知缺口记在这里。

## 为什么要一个 ffi 模块（R2）

- 2026-09-23 之前每个业务文件自己声明 C 接口、自己写守卫：`AXIsProcessTrusted` 两处声明（一处 `-> u8`、一处 `-> Boolean`，ABI 等价但两处口径）、粘贴板 extern 两处（imp 与 live_tests 各一份，签名靠人眼对齐）、`CfGuard` 两份 + `ReleaseOnDrop` 一份、CFString 构造三次同形实现。签名漂移这类问题不会有编译错误，只会在运行到某条罕见路径时炸。
- 收编后：声明、不透明类型、枚举常量、守卫、CF 转换助手都在 `src/ffi/`，评审只审这一处；业务模块只写“为什么此处的前置条件成立”。新增框架（例如 T9 要链的 Carbon）直接在这里加一个文件。
- 回调不进 ffi：`events/mod.rs::fire`、`events/mouse.rs::raw_callback` 是我们导出给 C 的函数，逻辑属于各自模块，留在原处。

## CfGuard 的构造契约

- `CfGuard::new` 只接管 +1 引用（创建/拷贝/Retain 的返回值）或 NULL；`Drop` 判空后恰好释放一次。字段私有，外部只能经 `new` 构造。
- 借用访问器（`string_ref` / `data_ref`）用于把引用传给 C 接口——调用方保证类型正确，守卫仍持所有权、调用方不得释放。
- `cf_string` / `cf_data` 直接返回守卫，调用方不再“先拿裸指针、再手工包守卫”；这两步分开正是过去错误路径漏释放的来源。

## 已删除符号的缘由留档

- `MAX_SELECTION_BYTES` 的语义没有随 `cfstring` 进 ffi：上界是**选择领域**的策略（远端报告的长度不可信，无界分配失败会 abort 进程而非返回错误），所以 `string_from` 只接受 `max_bytes` 参数，数值留在 `selection`。删掉的旧注释“超限按取不到选区处理”现在由 `cf::string_from` 的文档承接。

## 已知缺口（记录项）

- **`ffi/` 内的签名漂移没有自动化检查**：门禁只防“在别处新声明”，防不了“在这一处写错”。部分调用点由 CI 覆盖（粘贴板快照/恢复的 live_tests 会在真实剪贴板上跑，且断言了条目与 flavor 的对应关系），事件 tap 的 `CGEventGetLocation` 只有 L4 手工测试会走到——返回结构若写错（如两个 f64 顺序反了）编译器不会报错。要根治得上 bindgen 或 sys crate，当前取舍是“集中 + 人工评审”。
- **`live_tests::read_flavor_data` 不释放拷贝出的 CFData**（+1 引用泄漏，仅测试路径，随进程结束回收）。R2 收编时保持原样以免混入行为变更；要清账另开小任务。
- **`ffi/cf.rs::string_from` 多了一条 `used > buf.len()` 判定**（原实现直接 `truncate`）。按 `CFStringGetBytes` 的契约不可达，属于防御性收口：真触发时返回 `None` 而不是静默截断。

## ffi/carbon.rs 的登记（任务 14，2026-09-24）

- 本目录此前只有 `cf.rs`（CoreFoundation 的少数几个函数），`carbon.rs` 是第二个
  模块，也是本仓第一次显式链接系统框架（`#[link(name = "Carbon")]`）。登记在
  `ffi/mod.rs` 的模块清单里与 `cf.rs` 并列，遵守“FFI 声明集中”这条门禁的
  同一口径：声明在这一处、评审在这一处、签名漂移的风险也只集中在这一处
  （见上文“已知缺口”——门禁防不了签名写错，所以少一处就少一处风险）。
- 只用框架里的**一个函数**。不引 `objc2-carbon` 之类的绑定 crate：为一个声明
  换进一整代 deprecated API 面与一个额外的依赖维护点，不划算；这条取舍与新
  框架引入一起写在 carbon_comment.md。
