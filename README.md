# Gloss

划词翻译工具 —— 鼠标选中文字，即弹出 LLM 翻译结果。

> **Gloss**：本义「注解 / 旁注 / 释义」，暗藏二次元梗（日语「グロス」）与「光泽」双关。外行觉得专业，内行品得出梗。

## 技术栈

- **语言**：纯 Rust
- **GUI**：egui（即时模式）
- **渲染**：wgpu / WebGPU（Metal / Vulkan / DX12）
- **翻译引擎**：大语言模型（LLM，推荐 DeepSeek，接口兼容 OpenAI）
- **异步**：tokio + reqwest

## 设计原则

划词翻译是「高频打扰式」交互，体验命门是 **克制** —— 快、清、小、稳。

## 开发状态

- [x] 技术方案选型（纯 Rust + WebGPU + egui）
- [x] UI 设计与品牌命名
- [x] CI/CD 基建（justfile + GitHub Actions + Dependabot + git-cliff）
- [ ] 项目脚手架（winit + wgpu + egui 最小渲染闭环）
- [ ] 划词取词（跨应用选区读取）
- [ ] LLM 流式翻译
- [ ] 翻译浮层（置顶透明窗口）
- [ ] 配置与缓存

## 工程规范

- **提交信息**：遵循 [Conventional Commits](https://www.conventionalcommits.org/zh-hans/)，由本地 git hooks + CI 双重校验。
- **质量门禁**：`just check`（fmt + clippy + test），CI 在 PR 上强制通过。
- **依赖更新**：Dependabot 每周自动检查 cargo 与 GitHub Actions 依赖。
- **变更日志**：由 git-cliff 从提交历史自动生成（`just changelog`）。

## License

MIT
