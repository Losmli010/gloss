# main.js 注释记录

- 下载契约细节：同源 manifest.json（CI 注入）提供 channels.stable[<triple>] 的
  dmg_url / url / size 三个字段；校验走 schema === 1。
- 键盘契约出处：ARIA menu 模式（方向键循环、Home/End 跳转、Esc 关闭回焦、
  Tab 走默认顺序并收起菜单）。
- 架构识别的原口径：渲染器名含 "Apple M2"/"Apple GPU" 判新架构，含
  "Intel/AMD/NVIDIA" 判旧架构；UA-CH 为次选（architecture/platform 熵值）。

## 三平台扩展（2026-11-07，Windows 移植 P3/D8）

- **ARCHES 扩第三平台**：`x86_64-pc-windows-msvc`（标签“Windows x64”，
  短名 `win` 供下载卡元素 id 拼接）；短名从三元组比较改为 ARCHES 表内
  `short` 字段（三平台不再是二选一的三元）。
- **manifestLooksValid 放宽**：条目校验只要求 `url`（https）与 `size`，
  `dmg_url` 不再必查——Windows 条目按发布矩阵不带镜像字段；darwin 条目
  仍带 `dmg_url`，镜像直链缺失时各处 `(dmg_url || url || releases)` 逐级
  兜底（主按钮取 `dmg_url || url`，Windows 访客选中后直链落 zip）。
- **detectArch 加 Windows 一级**：UA 含 Windows 直接落
  `x86_64-pc-windows-msvc`（首发只发 x64，ARM 未决见移植方案 §6）；darwin
  细分的 WebGL/UA-CH 两级原样保留。
- **keychainNote 分平台**：hero 密钥说明随当前架构切换——darwin 报
  “本机钥匙串（Keychain）”，Windows 报“Windows 凭据管理器”；说明词
  函数化（`keychainNote`）避免渲染路径里散落三元。
- **指引切换**：`render()` 按当前架构切 `guide-macos`/`guide-windows` 的
  `hidden`（darwin 判定用 `-apple-darwin` 后缀）。无脚本时两块都可见（渐进
  增强，2026-11-07 评审遗留 F2 改准）：HTML 不再预置 `hidden`，脚本首段
  （延迟脚本执行点，先于清单与探测）按缺省架构先收起不匹配块——收起对
  无脚本访客不生效，也不再依赖标记语言里的初始隐藏属性；指引块在折叠
  之下，同步收起无可感知闪烁。取元素经文档接口（getElementById）直取
  而非后文的取元素助手：本段先于助手声明执行，经助手会踩进声明前的
  暂时性死区、整个脚本当场中止（2026-11-07 评审修正：首版经助手调用
  即此后果，真实无头浏览器复现后改直取）。
- 测试口径：页内行为用浏览器自动化验证（三卡渲染、清单注入后的直链与
  体积、三种 UA 的识别、指引切换与 manifestLooksValid 三例），登记见
  `docs/tests/bdd.md` 的 `pages_site_point_check`。
