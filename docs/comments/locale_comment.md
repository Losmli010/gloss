# locale.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关符号时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## 模块存在理由（R1，2026-09-22）

- 为什么新开模块而不是塞进 `appearance` 或 `storage`：这两个模块各自实现一个端口（图标、配置存储），系统语言不是端口——它没有替身必要（结果只影响模板语言，错了也只是回到出厂中文模板），因此是一支自由函数 + 一个纯映射函数，独立成模块比借住别的模块干净。
- 备选：`sys-locale` crate。它的价值在跨平台，而本仓库当前只发 macOS（`gloss-platform` 已是平台专属适配层，用了 objc2/security-framework 等系统绑定）；为一次调用引入新依赖要过许可证与审计门禁，还多一份上游维护面。改用已在依赖树里的 `objc2-foundation` 的 `NSLocale`，代价是给该 crate 增开 `NSLocale`/`NSArray`/`NSString` 三个特性（见 gloss-platform_Cargo_comment.md）。

## system_locale

- 为什么读 `NSLocale.preferredLanguages` 的首项而不是 `NSLocale.currentLocale.languageCode`：偏好语言列表才是“用户想用什么语言”；`currentLocale` 是区域设置（会受地区影响，如 `zh_CN` 的机器上可能是 `en_US` 的区域格式）。首项即用户排序里最想要的语言。
- 为什么在 `main.rs` 的组装点调用而不是惰性调用：系统语言是进程级常量（改了要重启），启动期读一次并记日志（真机走查看得到实际落定值），比每个任务触发时调一次系统 API 更省、也更好排障。
- 函数名（任务 09 由 `system_prompt_locale` 改为 `system_locale`）：返回值现在同时喂 prompt 模板与 UI 文案表两条选表路径，“prompt”限定词已不成立。

## locale_for（已随共享化上移）

- 档位映射原住本文件，2026-11-14（方案 A7 随 P2）与系统（Windows）侧的
  同构映射一并上移平台中立共享层（common）：映射只写一份，两侧
  `system_locale` 只保留各自的原语读取面（首选语言列表首项对用户默认
  区域名）。判定依据（`zh*` 前缀、探测失败归英文、不入核心库、字体加载
  复用同一口径）随映射迁入共享语言记录。

## 符号契约（自代码注释迁入）

- `system_locale`：读系统首选语言列表的首项交共享档位映射（原为本地映射
  函数 `locale_for`，共享化后映射住平台中立共享层）。
