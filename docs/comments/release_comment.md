# release.yml 决策记录

> 按符号锚定（不写 job/步骤名，不写行号，防漂移）。改到相关配置时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## Python 包安装放开外部管理限制（2026-11-03）

- 缘由：托管平台（GitHub）运行器镜像更新后，macOS 镜像的系统 Python 转为外部
  管理，工作流里的 `pip install pytest` 在装包一步即失败（v0.2.8 及以前正常）。
- 修法：构建作业的环境变量加 `PIP_BREAK_SYSTEM_PACKAGES`，仅放行持续集成环境；
  取舍过改用虚拟环境（venv），但两个测试步骤都要改命令且与脚本侧其它工作流的
  写法分叉，环境变量是单点最小修复。

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

## 构建矩阵扩系统（Windows）与打包分叉（2026-11-07，Windows 移植 P3a）

- 构建矩阵加 `x86_64-pc-windows-msvc`（windows-latest）；cargo-bundle 不支持系统（Windows），其专属步骤（装包、Bundle、签名）按运行器系统加条件守卫，系统（macOS）步骤与产物不变。
- 系统（Windows）产物只有便携压缩包（zip）：`package_zip.py` 把可执行文件加旁置动态库（dll）压进包根；上传步骤按运行器系统分叉（镜像（dmg）只在系统（macOS）面存在，合一步会让 `if-no-files-found: error` 在系统（Windows）必炸）。冒烟不进系统（Windows）：托管运行器无窗口服务的可行性与预算有效性未验证（方案 §6），不进必跑门禁。
- 部署页作业的产物拷贝（glob `gloss-*.zip`）随矩阵扩入系统（Windows）包，更新清单的 triple 扩展归 D8（与站点改造同批发布），本批清单仍只含双 darwin 条目——Windows 产物先行上发布页但不进清单引用。

## 构建腿统一解释器与步骤外壳（2026-11-07，评审修复）

- 缘由：构建矩阵扩入系统（Windows）腿后，三个共用步骤未做外壳与解释器适配——运行器默认外壳在系统（Windows）是 pwsh，共享步骤里的写法（美元花括号输出文件重定向、脚本直调）按 PowerShell 语义必错；裸解释器命令名跨腿也不可靠（`python3` 在系统（Windows）运行器是商店占位、系统（macOS）镜像不带非版本号命令名，两镜像文档分别只登记 Python3 与 python）。
- 修法：三步显式钉外壳（bash）——重定向与输出文件写法在 bash 下按文档语义成立；解释器统一经安装动作（actions/setup-python）钉 3.12（与本地配方 py 单点同钉）并显式以 `python` 调用，脚本不再依赖解释器直调（直调经解释器首行寻解释器，系统（Windows）同样会落空）。
- 放开外部管理限制的环境变量随解释器统一而移除：安装动作装的是工具缓存解释器，无外部管理限制；该环境变量原为 macOS 系统解释器设，留着会误导读者以为仍在用系统解释器。
- 验证口径：三步的运行器级实跑借临时手动触发（workflow_dispatch）在分支引用上完成——版本一致性步对非标签引用按设计失败，但三腿的检出、装解释器与两个单测步全程绿，解释器与外壳适配在真实运行器上得证；输出文件写法与打包步的运行器级验证仍待下一次真实标签发布。
