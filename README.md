# Gloss

[![CI](https://github.com/Losmli010/gloss/actions/workflows/ci.yml/badge.svg)](https://github.com/Losmli010/gloss/actions/workflows/ci.yml)
[![Release](https://github.com/Losmli010/gloss/actions/workflows/release.yml/badge.svg)](https://github.com/Losmli010/gloss/actions/workflows/release.yml)
[![Security audit](https://github.com/Losmli010/gloss/actions/workflows/security-audit.yml/badge.svg)](https://github.com/Losmli010/gloss/actions/workflows/security-audit.yml)

划词翻译工具——鼠标选中文字，即弹出大语言模型（LLM）翻译结果。

> **应用名（Gloss）**：本义为“注解、旁注、释义”，与“光泽”双关，日语的外来词 `グロス` 同义——外行觉得专业，内行品得出梗。

## 技术栈

- **语言**：纯本语言（Rust）
- **界面**：界面库（egui），即时模式
- **渲染**：图形后端（wgpu）与图形标准（WebGPU），底层为系统图形框架（Metal）
- **平台**：系统（macOS）
- **翻译引擎**：大语言模型（LLM），推荐深度求索（DeepSeek），接口与业界通行规范（OpenAI）兼容
- **异步**：异步运行时（tokio）加网络库（reqwest）

## 设计原则

划词翻译是“高频打扰式”交互，体验命门是 **克制**——快、清、小、稳。

## 开发状态

按里程碑严格串行推进：渲染闭环（M1）→ 划词取材（M2）→ 智能管道（M3）→ 配置与设置（M4）→ 图像任务（M5）。

- [x] 技术方案选型：纯本语言（Rust）与图形栈（WebGPU/egui）
- [x] 界面设计与品牌命名
- [x] 应用图标，方案“划·译”：打包用图标与开发期程序坞（Dock）图标，见 `assets/icons/`
- [x] 持续集成基建：任务配方 `justfile`、托管平台动作（GitHub Actions）、依赖机器人（Dependabot）与发版说明工具（git-cliff）
- [x] 渲染闭环（M1）：窗口库（winit）、图形后端（wgpu）、界面库（egui）的最小浮层窗口
- [x] 划词取材（M2）：选区读取加鼠标手势
- [x] 智能管道（M3）：任务编排、大语言模型（LLM）流式、结果浮层与取消竞速
- [x] 配置与设置（M4）：配置热更新、钥匙串（keychain）、设置窗口与错误出口
- [ ] 图像任务（M5）：框选截图与剪贴板图片的图片解释
- [ ] 正式分发：用 `Developer ID` 签名加公证，当前为临时签名（ad-hoc）

## 首次启动与系统授权

划词功能依赖一项系统（macOS）授权（辅助功能），首次启动会弹系统级授权引导；接口密钥存放在登录钥匙串，授权完成后无需重复操作。

当前持续集成产物为临时签名（ad-hoc）：钥匙串条目的访问授权按代码签名记忆，安装新构建（签名变化）后首次读取可能再弹一次“允许访问钥匙串”，“始终允许”只对同一构建有效——正式分发以 `Developer ID` 签名加公证落地后不再重弹。

## 工程规范

- **提交信息**：遵循 [Conventional Commits](https://www.conventionalcommits.org/zh-hans/)，由版本库钩子与持续集成双重校验。
- **质量门禁**：`just check`——格式化、静态检查（Clippy）、测试、约束与密钥扫描，持续集成在拉取请求（PR）上强制通过。
- **依赖更新**：依赖机器人（Dependabot）每周自动检查构建工具（cargo）与托管平台动作（GitHub Actions）依赖。
- **发版说明**：以托管平台（GitHub）发布页（Releases）为唯一来源，由持续集成在打标签时从提交历史生成。

## License

MIT
