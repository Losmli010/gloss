# pasteboard.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## 常量与类型

- `CLIPBOARD_NAME`：系统剪贴板的注册名（kPasteboardClipboard 的字符串值）。
- `K_PASTEBOARD_MODIFIED`：kPasteboardModified：自上次经本地引用访问以来全局粘贴板已被修改；标志在 Synchronize 调用时被消费，探针侧需闩锁。
- `PasteboardRef`：粘贴板句柄：CF 不透明类型，Create 返回 +1 引用。
- `PasteboardItemID`：粘贴板条目标识符：客户端自定义的不透明指针，同一次快照内唯一即可。

## extern 声明契约

- `PasteboardCreate`：创建指向指定名称全局粘贴板的本地引用（+1），失败返回非零状态码。
- `PasteboardSynchronize`：与全局粘贴板同步，返回标志集（含 kPasteboardModified）。
- `PasteboardGetItemCount`：返回粘贴板条目数，失败返回非零状态码。出参是 ItemCount（MacTypes.h 的 unsigned long，Darwin LP64 下 8 字节）。
- `PasteboardGetItemIdentifier`：取条目标识符（index 从 1 起，见 Pasteboard.h），失败返回非零状态码。
- `PasteboardCopyItemFlavors`：拷贝条目的全部 flavor 名（CFArray，元素为 CFString，数组 +1 引用），失败返回非零状态码。
- `PasteboardCopyItemFlavorData`：拷贝条目指定 flavor 的原始数据（+1 引用）；数据尚未物化（promised）时失败，失败返回非零状态码。
- `PasteboardClear`：清空粘贴板全部条目，失败返回非零状态码。
- `PasteboardPutItemFlavor`：向条目写入 flavor 数据；不接管 `data` 引用，调用方保活至调用返回。
