# download.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## DownloadError（下载失败形态）

- 后果一致（落下载步失败、错误卡可重试），分形变体供日志与文案各说各话。
- `Network`：请求发出失败、非 200/206 状态、Range 语义不符等传输层故障。
- `TooLarge`：响应体累计超过清单 `size`（无界磁盘占用禁入）。
- `SizeMismatch`：传输提前结束：到手的字节少于清单 `size`（`.partial` 保留可续传）。字段：`expected` 清单声明的整包字节数；`actual` 实际到手的字节数。
- `ShaMismatch`：全量传输完成但 sha256 与清单不符（`.partial` 已丢弃）。
- `Io`：本地文件系统故障。
- `Cancelled`：下载被取消（`.partial` 保留，进程内重试可续传）。

## download（下载并校验一个整包）

- 成功返回最终 zip 路径（`.partial` 已原子改名）。
- `work_dir` 是下载工作目录（`.partial` 与最终文件同卷，改名才是原子的）；`resume` 为真时基于遗留 `.partial` 续传，为假时先清除遗留 `.partial` 全量重下。取消令牌在流式循环内逐块响应，取消后 `.partial` 保留在途进度。
- 服务端 206 才续传；200（含不支持 Range 的服务端）一律从头整体重下。

## plan_resume_start（续传起点）

- `resume` 为真且遗留 `.partial` 值得续时从残料长度续起，否则从头并清掉残料。取消在发出任何请求前核对。
- 残料只有「非空且未满」才值得续；满了或空了都当损坏从头重下。

## request_partial（请求与响应校验）

- 发出下载请求并校验响应：非 200/206、Range 语义不符（asked 起点与 Content-Range 对不上）都按传输层故障拒绝。

## open_partial / stream_body / flush

- `open_partial`：按 `offset` 打开 `.partial` 供写入：从头下载时清空旧内容，续传时保留并定写到续点。
- `stream_body`：流式落盘：取消逐块响应（在途进度留在 `.partial`），累计字节数（含续传起点）超清单 `size` 即断，写失败的现场交调用方冲刷。传输中断：在途进度留在 .partial，进程内重试可续传。

## verify_and_rename（校验与落位）

- 传输完成后的校验与落位：先 size 后 sha256（前者流式常量时间，后者要整读一遍文件）；size 偏小保留残料供续传，sha 不符丢弃残料，校验通过才原子改名到最终路径。
- 少于清单 size 是传输提前收尾，等同中断：残料保留供续传。

## artifact_file_name / sha256_hex_file

- `artifact_file_name`：产物文件名：取 URL 路径末段（清单 url 已过 https 校验，这里只防形态异常的段名进文件系统）。
- `sha256_hex_file`：文件的 sha256（64 位十六进制小写）：分块读入，不整文件进内存。
