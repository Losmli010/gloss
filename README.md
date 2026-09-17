# Gloss

[![CI](https://github.com/Losmli010/gloss/actions/workflows/ci.yml/badge.svg)](https://github.com/Losmli010/gloss/actions/workflows/ci.yml)
[![Release](https://github.com/Losmli010/gloss/actions/workflows/release.yml/badge.svg)](https://github.com/Losmli010/gloss/releases)
[![Security audit](https://github.com/Losmli010/gloss/actions/workflows/security-audit.yml/badge.svg)](https://github.com/Losmli010/gloss/actions/workflows/security-audit.yml)

划词翻译工具 —— 鼠标选中文字，即弹出 LLM 翻译结果。

> **Gloss**：本义「注解 / 旁注 / 释义」，暗藏二次元梗（日语「グロス」）与「光泽」双关。外行觉得专业，内行品得出梗。

## 技术栈

- **语言**：纯 Rust
- **GUI**：egui（即时模式）
- **渲染**：wgpu / WebGPU（Metal）
- **平台**：macOS
- **翻译引擎**：大语言模型（LLM，推荐 DeepSeek，接口兼容 OpenAI）
- **异步**：tokio + reqwest

## 设计原则

划词翻译是「高频打扰式」交互，体验命门是 **克制** —— 快、清、小、稳。

## 开发状态

按里程碑严格串行推进（M1 渲染闭环 → M2 划词取材 → M3 AI 管道 → M4 配置与设置 → M5 图像任务）：

- [x] 技术方案选型（纯 Rust + WebGPU + egui）
- [x] UI 设计与品牌命名
- [x] 应用图标（方案 A「划·译」：打包 .icns + 开发期 Dock 图标，见 `assets/icons/`）
- [x] CI/CD 基建（justfile + GitHub Actions + Dependabot + git-cliff）
- [x] M1 渲染闭环（winit + wgpu + egui 最小浮层窗口）
- [x] M2 划词取材（选区读取 + 鼠标手势 + 热键）
- [x] M3 AI 管道（任务编排 + LLM 流式 + 结果浮层 + 取消/竞速）
- [x] M4 配置与设置（配置热更新 + Keychain + 设置窗口 + 错误出口）
- [ ] M5 图像任务（框选截图 → OCR / 图片解释）
- [ ] 正式分发（Developer ID 签名 + 公证，当前为 ad-hoc 签名）

## 工程规范

- **提交信息**：遵循 [Conventional Commits](https://www.conventionalcommits.org/zh-hans/)，由本地 git hooks + CI 双重校验。
- **质量门禁**：`just check`（fmt + clippy + test + 约束 + 密钥扫描），CI 在 PR 上强制通过。
- **依赖更新**：Dependabot 每周自动检查 cargo 与 GitHub Actions 依赖。
- **变更日志**：由 git-cliff 从提交历史自动生成（`just changelog`）。

## License

MIT
