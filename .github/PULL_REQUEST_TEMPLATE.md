<!-- 标题遵循 Conventional Commits：type(scope): 描述
     （type 白名单与长度限制见 scripts/check-commit-msg.sh，CI 会逐条校验） -->
## 改了什么

<!-- 一段话说清改动内容；涉及里程碑任务请带上任务号（如 M4-T6） -->

## 为什么

<!-- 动机 / 背景；行为变更要说明取舍 -->

## 怎么验证的

<!-- 本地跑过哪些配方、哪些场景实测过；快照基线变化必须单独说明原因 -->

- [ ] `just precommit` 通过
- [ ] `just check` 通过（动了逻辑时）
- [ ] 快照基线变化已说明（动了 UI 时）
- [ ] 相关文档已同步（AGENTS.md / docs，动了配方或架构时）
