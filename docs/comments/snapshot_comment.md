# snapshot.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## OpenPasteboard

- 打开的系统粘贴板：引用与名称字符串一并用守卫释放。Drop 语义见代码内 `// SAFETY:` 注释：唯一释放点，恰好归还一次。

## open_pasteboard / open_pasteboard_reported

- `open_pasteboard`：打开系统剪贴板的本地引用。
- `open_pasteboard_reported`：同 `open_pasteboard`，但带回失败状态码（名称构造失败记 -1）：恢复路径的诊断消息需要真实错误码定位粘贴板服务故障。

## SavedFlavor / SavedItem / SavedContent

- `SavedFlavor`：快照的单个 flavor：flavor 名（Retain 自持，守卫释放）与原始字节。
- `SavedItem`：快照的单个条目：标识符原样保留，恢复时按它把 flavor 归并回同一条目。
- `SavedContent`：原剪贴板的完整快照：空 Vec 即原剪贴板为空（恢复 = 清空后不写入）。

## PendingRestore

- 待执行的恢复：受控路径经 `restore_now` / `skip_restore` 显式收口，panic 展开由 Drop 兜底按快照恢复原剪贴板。
- `new`：待执行的恢复：`skip` 只由收口路径显式置位，构造即武装 Drop 兜底。
- `restore_now`：按快照恢复原剪贴板并取消 Drop 兜底；返回实际清板数（供调用方计入写盘归因），恢复失败已留 error 痕（环节与状态码），不影响调用方的主结果。
- `skip_restore`：跳过恢复（Drop 兜底一并取消）。
- Drop：只有 panic 展开会走到这里：受控路径都已显式收口。

## snapshot_pasteboard

- 快照原剪贴板全部条目与 flavor 数据。两类失败整体返回 `None`（调用方据此放弃注入，剪贴板保持原样）：结构读不出（板、条目数、条目标识、flavor 名单）；板上条目的 flavor 全部读不出。单个 flavor 读不出数据（promised 等）只跳过该 flavor，读得出的部分照常保全。
- 板上确有条目却无一 flavor 可恢复（全部读不出）：注入后原内容无法如实写回，恢复轮会把剪贴板清空——按整体放弃收口，剪贴板保持原样；空 items 在这里与「原剪贴板为空」的合法空快照严格区分（后者 count == 0）。

## snapshot_items

- 逐条目快照：条目标识与 flavor 名单任一结构读数失败即整体放弃（返回 `None`）；全部 flavor 都读不出的条目跳过——恢复时写一个空条目毫无意义。

## snapshot_flavors

- 逐 flavor 快照：单个 flavor 读不出数据只跳过该 flavor，结构失败整体 `None`（边界见 `snapshot_pasteboard` 文档）。单个 flavor 读不出（promised 数据等）跳过而非整轮放弃：读不出意味着我们同样无法如实恢复它，整轮放弃只会让兜底通道在含 promises 的板上整批失效；可读 flavor 照常保全，读不出的在恢复时缺席。
