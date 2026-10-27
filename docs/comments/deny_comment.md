# deny.toml 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关配置时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## licenses

- 2026-09-18 商用底线决策：MIT 项目只接受宽松许可证，copyleft（GPL/AGPL/LGPL）一律不放行。新增依赖被拦时，先确认它是否真的必要，再决定是否扩白名单。

## licenses.allow（逐条来历）

- 2026-09-18 渲染栈引入（均为宽松许可，无 copyleft 义务）：BSL-1.0 —— clipboard-win / error-code（arboard 的 Windows 剪贴板路径）；OFL-1.1 + Ubuntu-font-1.0 —— epaint_default_fonts（egui 内置字体数据）。
- 2026-09-18 MPL-2.0 为文件级弱 copyleft：仅要求修改 MPL 文件本身时开源该文件，不会传染宿主项目，商用安全。引入者（均来自 font-kit）：dwrote（Windows DirectWrite 后端）、option-ext（dirs → dirs-sys 链）。

## unused-allowed-license

- 2026-09-18 白名单按「将来可能引入的依赖」预留，当下用不到的条目不算问题，设为 allow 不刷警告。

## bans（multiple-versions）

- 2026-09-18 同一 crate 多版本只警告不拦：GUI/GPU 生态（egui、wgpu）几乎必然出现多版本，硬拦会寸步难行。

## bans（wildcards）

- 2026-09-18 wildcard 检查只能放行的原因：cargo-deny 把 workspace 内部的 path 依赖也算成 wildcard（path 依赖不带 version，等价 "*"），而本项目版本单点维护在 [workspace.package]、不给 path 依赖补硬编码 version。外部依赖仍由 licenses + sources 兜底。

## 注释迁入（2026-10-24）

- 检查项文档：<https://embarkstudios.github.io/cargo-deny/checks/index.html>
- 豁免登记原表述：把 advisory id 登记进 ignore 表，并写清豁免理由与跟踪项。
