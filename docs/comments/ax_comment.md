# ax.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## AXUIElementRef

- HIServices 的 AX 元素句柄：CF 不透明类型，遵守 +1/-1 内存管理规则。

## 属性名常量

- `FOCUSED_UI_ELEMENT_ATTRIBUTE`：“焦点元素”属性名的稳定字符串值（kAXFocusedUIElementAttribute 的文档契约，与 System Events/AppleScript 所用同名）。
- `FOCUSED_APPLICATION_ATTRIBUTE`：“焦点应用”属性名的稳定字符串值（kAXFocusedApplicationAttribute 同上）：由系统 AX 运行时供给，目标应用建不建无障碍树都能取到。
- `SELECTED_TEXT_ATTRIBUTE`：“选中文本”属性名的稳定字符串值（kAXSelectedTextAttribute 同上）。
- `MANUAL_ACCESSIBILITY_ATTRIBUTE`：Chromium/Electron 识别的私有开关属性：写入 true 让宿主按需构建无障碍树（Electron 与 Chrome 现行内核都识别）。
- `ENHANCED_USER_INTERFACE_ATTRIBUTE`：老版 Chromium 的等价开关属性：与上一条一起写入，覆盖未认新属性的旧内核；非 Chromium 宿主对未知属性返回属性不支持错误并忽略。
- `TRUSTED_CHECK_OPTION_PROMPT`：AXIsProcessTrustedWithOptions 选项字典的键（kAXTrustedCheckOptionPrompt 的文档契约值）：true 时对未授权进程弹系统授权引导对话框。

## extern 声明契约

- `AXIsProcessTrusted`：当前进程是否已获辅助功能授权（TCC）。
- `AXIsProcessTrustedWithOptions`：带选项的授权查询：`options` 含 prompt=true 时对未授权进程弹系统授权引导，返回值仍是当前授权状态。
- `AXUIElementCreateSystemWide`：创建 systemwide AX 元素，成功返回 +1 引用。
- `AXUIElementCopyAttributeValue`：复制元素属性：成功时 `value` 收到 +1 引用，失败时不写入。
- `AXUIElementSetAttributeValue`：设置元素属性（值为 kCFBooleanTrue 的布尔写）：跨进程写，返回 AXError；宿主不支持该属性时返回属性不支持错误，不做任何修改。
- `AXUIElementGetPid`：查询元素所属进程的 pid：纯查询，返回 AXError。
- `AXUIElementSetMessagingTimeout`：设置跨进程消息超时（秒）：传 systemwide 元素 = 对**本进程全局**生效（此后所有元素的同步 AX 调用按此时长超时）；传其它元素只对该元素自身生效，不传播到相等的其它对象。设 0：systemwide 重置回系统默认，其它元素回落到全局值。超时后同步调用以 kAXErrorCannotComplete 返回，不再无限等待远端应用。返回非 0 表示设置失败（如非法值）。

## is_process_trusted

- 当前进程是否已获辅助功能授权（TCC）；纯查询，无前置条件。

## is_process_trusted_prompting

- 当前进程是否已获辅助功能授权；未授权时顺带弹系统授权引导对话框（内含“打开系统设置”入口），返回值仍是查询时刻的授权状态。
- 引导对话框每次调用至多弹一个（系统对同一进程的重复请求会自行合并），调用方应把调用频次控制在启动预检这类低频节点上。
- kCFType 字典回调会对键 retain，key 的守卫已覆盖调用全程；不得再对 `key.string_ref()` 建第二个守卫（会双重释放）。

## create_system_wide

- 创建 systemwide AX 元素；系统异常时返回 NULL（调用方判空），否则是 +1 引用。
- Safety：无前置条件；非 NULL 返回值必须恰好释放一次（交给 [`super::cf::CfGuard`]）。

## copy_attribute

- 复制元素的一个属性值：成功时把 +1 引用写进 `out`，失败时不写入。
- Safety：`element` 必须是有效的 AX 元素引用，`attribute` 必须是有效的 CFString 引用；`out` 必须指向可写内存；返回 0 时其中是必须恰好释放一次的 +1 引用。

## set_bool_attribute

- 把元素的布尔属性置为 true：kCFBooleanTrue 是系统框架导出的常量对象指针，按借用传递，无所有权转移；宿主不支持该属性时返回属性不支持错误且不产生任何修改。
- Safety：`element` 必须是有效的 AX 元素引用，`attribute` 必须是有效的 CFString 引用。

## pid_of

- 查询元素所属进程的 pid：成功时写入 `out`。
- Safety：`element` 必须是有效的 AX 元素引用，`out` 必须指向可写的 i32。

## set_messaging_timeout

- 设置跨进程消息超时（秒）：传 systemwide 元素 = 对本进程全局生效；传其它元素只对该元素自身生效。超时后同步调用以 kAXErrorCannotComplete 返回，不再无限等待远端应用。返回非 0 表示设置失败（如非法值）。
- Safety：`element` 必须是有效的 AX 元素引用。
