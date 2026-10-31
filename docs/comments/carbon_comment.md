# carbon.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关符号时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），代码内不留指向本文件的指针。

## is_secure_event_input_enabled / link(name = "Carbon")

- 2026-09-24 这是本仓库第一条非系统默认链接的框架依赖（此前只有 Foundation
  等经 objc2 引入的框架）。选择 Carbon 的原因：判断“当前是否有密码框持有
  焦点”在 AppKit / Security 里没有等价接口，而 Carbon 的
  `IsSecureEventInputEnabled` 是系统自己用来回答这个问题的那个函数——它与
  输入法、密码管理器看到的是同一份状态，不会因为我们自己实现一套检测而
  与实际行为不一致。
- 为什么直接写 `#[link(name = "Carbon", kind = "framework")]` 而不是引
  `objc2-carbon` 之类的 crate：这是**一个 C 函数、一个框架**，引 crate 会把
  一整代被 Apple 标记为 deprecated 的 API 面拖进依赖树（且这些 crate 的
  维护状态参差）。声明集中在这里、连着 `// SAFETY`/链接说明一起读，比在
  依赖清单里多一行更可控。
- 为什么这个声明必须落在 `ffi/` 下：仓库的“FFI 声明集中”约束
  （拆包后住 scripts/hooks/constraints/ui_boundary.py）扫的就是 `extern "C" {` 的落点，理由
  是同类声明散在各适配器里时无法审出重复与不一致；新增框架依赖要走
  `ffi/mod.rs` 的登记（与 `cf.rs` 同一处）。
- 返回类型照抄头文件的 `Boolean`（`u8`）而不是直接声明成 Rust `bool`：
  C 的 `Boolean` 是 1 字节整数，`bool` 在 FFI 上要求调用方保证取值只能是
  0/1——照抄类型再由 Rust 侧做 `!= 0` 判定，就不依赖对端实现的细节。
- 测试只断言“链接得上且默认返回假”：真机上让安全输入为真需要用户手动把
  焦点放进密码框，L1 测试做不到。这条用例的价值在于**链接期与符号名**——
  框架没链上或符号拼错，测试直接起不来（不会悄悄降级成永远放行）。
  安全输入为真的那条路径属于 L4 真机走查（见 docs/tests/bdd.md 人工测试）。

## 符号契约（自代码注释迁入）

- `IsSecureEventInputEnabled`：系统当前是否处于安全输入态（Secure Event Input）。
- `is_secure_event_input_enabled`：系统是否处于安全输入态——有任何进程开启了 Secure Event Input（密码框、钥匙串授权弹窗、登录窗）时为真；纯查询，无前置条件，不索取权限。
