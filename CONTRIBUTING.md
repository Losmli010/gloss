# 参与开发

人类贡献者与 AI 代理都先读 [AGENTS.md](./AGENTS.md)——架构约束、评审门禁、测试分层都在那里，本文只管「跑起来、交上去」。

## 环境初始化

clone 后跑一次（幂等，重复跑无副作用）：

```bash
just setup
```

它校验 Rust 工具链（与 CI 一致的 1.96.1）、清点可选工具（cargo-bundle / cargo-llvm-cov / cargo-audit / cargo-deny）、安装 git hooks。缺什么会给出安装命令，不替你做全局安装。

## 日常命令

| 命令 | 用途 |
| --- | --- |
| `just run` | 跑开发版 |
| `just check` | 全量门禁（precommit 全部 + test），本地要跑测试时用 |
| `just precommit` | 提交前静态检查（pre-commit 钩子跑的就是它） |
| `just selftest` | L3 显隐自检（需要窗口服务与 GPU） |
| `just logs` | 跟随最新日志 |

## 分支与提交

- **每任务一分支一 PR**：分支名带任务号或域（如 `feat/m4-t6-settings-ui`、`fix/m4-t8-mouse-tap-sigill`）。
- **提交信息**遵循 Conventional Commits：type 白名单（feat / fix / docs / style / refactor / perf / test / build / ci / chore / revert）与标题长度由 `scripts/check-commit-msg.sh` 校验（CI 逐条跑；本地可用 `just lint-commit <file>` 手动自查，commit message 规范不做本地钩子校验——钩子跑的时候消息还没落盘）。
- **合并方式**：squash merge，PR 标题即进 CHANGELOG，写得像给人看的变更条目。
- **分支落后 main**：用 merge main 解（不 rebase 已推送的提交）。

## 发布

发布由 `v*` tag 触发 release workflow：构建双架构 → ad-hoc 签名 → aarch64 冒烟 → 产出 zip + dmg → 自动生成 Release。发版只需三步：

```bash
# 1. 把根 Cargo.toml [workspace.package] 的 version 改成发版号
# 2. 打 tag（本地先自检版本一致性）
just release-check v0.1.0 && git tag v0.1.0 && git push origin v0.1.0
# 3. workflow 跑完，GitHub Releases 页收产物
```

当前是 ad-hoc 签名（首次打开需右键 → 打开）。上 Developer ID 后，把 release workflow「Ad-hoc sign」步骤的 `codesign -` 换成正式身份，并接 notarytool 公证。

## 真机测试（opt-in）

辅助功能与真实凭据类测试不进 CI，本机跑：

```bash
cargo test -p gloss-platform -- --ignored
```

首次需在「系统设置 → 隐私与安全性 → 辅助功能」给终端 App 授权；LLM live 测试还要设 `GLOSS_LIVE_API_KEY` 等环境变量，缺项时测试会直接给出可照抄的命令。
