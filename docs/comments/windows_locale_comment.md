# windows_locale 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## system_locale 桩（2026-11-05，Windows 移植 P0）

- 恒回落 `Locale::En`：与 macOS 实现“读不到系统偏好即英文”的降级语义一致；`GetUserDefaultLocaleName` 读取随 P2 落地。

## system_locale 落地（2026-11-07，Windows 移植 P2④）

- 读用户默认区域名（`GetUserDefaultLocaleName`）：单次调用进定长缓冲（`LOCALE_NAME_MAX_LENGTH`），返回长度截断缓冲、按首个空终止收口（兼容“含空终止计数”与“不含”两种返回口径），映射逻辑与系统（macOS）同构——`zh` 前缀（大小写不敏感）落中文档位，其余与读不到区域名（返回 0）都落英文档位，与系统实现的降级语义一致。进程内启动期读一次不变。
