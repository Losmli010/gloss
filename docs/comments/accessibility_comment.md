# accessibility.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## 只读一个属性的策略

- 2026-09-19 策略取最简一条路径（只读 `kAXSelectedTextAttribute`）的边界：部分应用（如个别 Electron/Chromium 场景）只暴露 parameterized 选区属性，暂不兜底——由 CompositeReader 的剪贴板兜底通道覆盖。

## interpret 的“空选区 / 读不到”分档（2026-09-30）

- 起因：组合通道原先对“AX 报任何非权限错误”都注入 Cmd+C 走剪贴板兜底，
  空选区（纯误滑：点按/甩动后焦点元素如实报告无选区）也会触发注入——
  剪贴板里的陈旧内容被当成“这次划词的选区”发给 LLM（interaction_review
  的 P2-16）。修复把 interpret 分成三档：`SelectionEmpty`（属性可读但
  选区为空：空串、kAXErrorNoValue）与 `SelectionUnavailable`（应用不
  支持属性、AX 调用失败、超长度上限的 None）分开建档，
  `composite::combine` 对前者不再兜底。`Ok(None)`（string_from 的拒绝
  路径：超 8MiB 上限、非字符串值）不归“空”档——超限选区恰恰是“选了
  很多”，归“读不到”才能保住剪贴板兜底这条旧路。
- 为什么 kAXErrorNoValue 归“空”而不归“读不到”：AXError.h 的语义是
  “属性存在但无值”——属性支持性已确认，只是没有选区。这与空串返回
  同义（多数 Cocoa 文本视图用空串表达无选区，个别应用用 NoValue）；
  “读不到”留给 `kAXErrorAttributeUnsupported` 这类“应用根本不暴露该
  属性”的情形——只有后者才是剪贴板兜底要服务的场景。
- 已知残留：AX 返回陈旧选区（属性在、有值、但已不是用户刚选的）无法在
  读取侧识别，误执行只能靠上游（手势时长滤除 + 手势触发空选区静默收回）
  压低概率，见 machine_comment.md。

## AX_MESSAGE_TIMEOUT_SECS（跨进程调用收紧到 1 秒）

- 2026-09-29 起因：owner 真机报障“在 ZCode（Electron）里划词偶尔不弹窗”。日志复盘发现 AX 跨进程同步调用对 ZCode 的焦点元素阻塞约 4 秒（系统默认超时）才返回 NoValue，期间事件线程被整个占住、后续手势只能排队；若用户在这 4 秒内再次划词，前一次的失败回传会被陈旧过滤静默丢弃——两次划词只弹一张卡，另一次全程无反馈。修复：`AXUIElementSetMessagingTimeout(1.0)` 对 **systemwide 元素**设置即对本进程全局生效（覆盖此后焦点元素的取值调用），超时即以 kAXErrorCannotComplete 返回（常规应用不受影响，正常读取远快于 1 秒），失败快速交由剪贴板兜底接管。作用域口径以 AXUIElement.h 为准：systemwide = 进程全局；设 0 = 重置回默认；普通元素上的设置只对元素自身生效、不传播——初版曾对 focused 元素重复设置并宣称“经 systemwide 取到的子元素会继承”，两者都与头文件不符，已删（systemwide 一处即足够）。
- 2026-09-29 教训：`AXUIElementSetMessagingTimeout` 的 C 签名是 `float`，初版 extern 声明写成 `f64`——ABI 上两架构浮点参数都走寄存器低位，被调方按 float 读低 32 位，1.0f64 被读成 0.0（systemwide 设 0 = 重置默认），设置完全静默失效且返回 Success，clippy/测试均无法发现。FFI 声明的标量类型必须逐字对照 SDK 头文件，`just check` 不覆盖跨语言 ABI。
- 2026-09-29 关联事实：Electron 应用读不到 `kAXSelectedTextAttribute` 的场景由剪贴板兜底覆盖（见 2026-09-19 条目），兜底依赖按键注入，注入的有效性取决于辅助功能授权——授权对运行中进程不追溯，换机器/重装后需要重启应用才生效（TCC 责任进程机制，见 main.rs_comment.md）。

## 消息超时 1s→0.3s（2026-10-16）

- 生产日志实测 AX 单次调用可达 1–2.3s（无响应宿主上逐读吃满旧超时），
  是取材段 p90=2.5s、max=9.4s 尾部的主成分；0.3s 下无响应宿主快速失败，
  交给组合通道的激活补读/剪贴板兜底接管。
- **残余风险（明示接受）**：极慢宿主上真实选区读取可能在 0.3s 内完不成
  而落空——后续补读有多次机会，且兜底通道最终兜住；热路径收益远大于
  该尾部。

## MAX_SELECTION_BYTES（2026-10-16，内存优化方案）

- 为什么从 8MiB 收到 256KB：长度来自远端进程报告，上限直接决定一次无校验分配的最坏量级；AX 超限归 SelectionUnavailable，组合通道的激活/剪贴板兜底照常接管超长选区（兜底路径有自己的确认写入语义），行为面无回归。划词翻译的选区量级远低于此。

## 符号契约（自代码注释迁入）

- `AX_ERROR_API_DISABLED`：AXError.h 中“辅助功能 API 未对当前进程启用”的返回码（kAXErrorAPIDisabled）。注意相邻码：-25212 是 kAXErrorNoValue（属性存在但无值，即“无选区”的常见返回），二者错一位语义就反转，见 tests 的相邻码对账测试。
- `AX_ERROR_NO_VALUE`：AXError.h 的 kAXErrorNoValue：属性存在但无值——“选区为空”的 AX 侧说法，与“取不到”（`SelectionUnavailable`）分开归档：前者让渡补读后与后者一样落剪贴板兜底（见 `composite::combine`），由确认写入分辨“确实没选”与“读了但拿不到”。
- `MAX_SELECTION_BYTES`：取值缓冲区的分配上界（字节）：长度来自远端进程报告，无校验的分配失败会直接 abort 进程而非返回错误，超限按取不到选区处理。256KB 已远超划词翻译的选区量级；超长选区经组合通道的兜底仍可读——超限归“取不到”（`SelectionUnavailable`），激活/剪贴板兜底照常接管。
- `interpret`：把“是否已授权 + AX 取值结果”映射为统一错误语义（纯逻辑，单测覆盖）：未授权一律 `AccessibilityDenied`（权限检查先于取值，AX 返回同一码时以更明确的权限语义收口）；空串与 `kAXErrorNoValue` 归 `SelectionEmpty`——属性在而选区空，是“没选东西”不是“读不到”；`Ok(None)`（超长度上限、非字符串值）与其余取不到的情形（应用不支持属性、系统调用失败）归 `SelectionUnavailable`——它们可能是“选了但读不动”，兜底通道（剪贴板）仍值得一试。
- `imp::AX_ERROR_FAILURE`：AXError.h 的 kAXErrorFailure：无具体语义的失败兜底。
- `imp::AX_ERROR_SUCCESS`：AXError.h 的 kAXErrorSuccess。
- `imp::AccessibilityReader`：AX 读选区实现：无状态，可按需构造。
- `imp::AX_MESSAGE_TIMEOUT_SECS`：AX 跨进程调用的消息超时（秒）：目标应用不响应 AX 轮询时（Electron/自绘控件常见），同步调用会阻塞到系统默认超时（数秒），把事件线程整个占住、后续手势只能排队——收紧到 0.3 秒，超时即返回错误并交由组合通道的激活补读与剪贴板兜底接管。对 systemwide 元素设置即对本进程全局生效，覆盖此后所有元素的取值调用。
- `imp::AccessibilityReader::read`：读取前台应用的选中文本。调用方保证：在平台事件线程上调用。未授权时不发起跨进程取值，直接按权限语义收口。
- `imp::copy_selected_text`：读选区的 FFI 薄壳：systemwide → 焦点元素 → 选中文本。任何一步失败都以 AX 原始错误码返回，语义映射交给 `interpret`。取值前对 systemwide 设一次消息超时即对本进程全局生效（见 `AX_MESSAGE_TIMEOUT_SECS`）。属性名按需构造、用毕即释放；分配失败视为本次读取失败。出参在失败时不写入，成功时由守卫接管 +1 引用。
