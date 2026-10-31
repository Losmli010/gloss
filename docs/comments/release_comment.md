# release.yml 决策记录

> 按符号锚定（不写 job/步骤名，不写行号，防漂移）。改到相关配置时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## Ad-hoc sign 步骤

- 2026-09-18 用 ad-hoc 签名替代正式的 Developer ID 签名（免费，无需证书）；用户首次打开需右键 → 打开，绕过 Gatekeeper。后续上 Developer ID 时，替换为 `codesign --sign "Developer ID Application: ..."` + notarytool。

## Smoke test 步骤

- 2026-09-18 冒烟只能跑与 runner 同架构的产物：macos-latest 是 arm64，x86_64 产物没有 Rosetta 就跑不起来，留给本机与 Intel runner 走查。

## Package artifacts 步骤

- 2026-09-18 .app 必须经 ditto 打包（保留签名与资源叉），上传目录散文件会让 Release 页变成一堆 Contents/... 碎片；.dmg 由系统 hdiutil 压制。

## Release 正文改为 --latest 一节（2026-09-30）

- 缘由：CHANGELOG.md 文件删除后，release workflow 的正文生成从
  `git cliff -o CHANGELOG.md`（整本历史）改为 `git cliff --latest -o
  release-notes.md`（只渲染当前 tag 的一节）。此前每个 Release 页都会重
  复携带更早版本的说明，与“Release 页是唯一来源”相悖——唯一来源意味着
  每个版本页恰好承载自己的说明。release-notes.md 只存在于 CI 工作区，
  不进仓库。
- cliff.toml 同步去掉文件式 header（“本文件记录…”），渲染产物直接以
  `## [版本] - 日期` 开头，作为 Release 正文更干净。

## 站点不留变更日志链接（2026-10-07）

- 站点“更新日志”区的 CHANGELOG 卡片原直链仓库 `CHANGELOG.md`，但该文件已按
  “Release 页是唯一来源”决策不落仓库，链接恒 404。修法取舍：部署时渲染整本
  `CHANGELOG.md` 进 Pages 产物，或站点不留变更日志链接；用户取后者——站点去掉
  该卡片，逐版本说明只留在 GitHub Releases（Release 页唯一来源决策不动）。
