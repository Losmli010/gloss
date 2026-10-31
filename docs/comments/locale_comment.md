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

## locale_for

- 为什么只看 `zh*` 前缀：模板与界面文案目前只有中英两档，判据就只需回答“是不是中文”。`zh`/`zh-Hans`/`zh_CN`/`ZH-TW` 都命中（大小写不敏感），其余语言（含 `ja`/`ko`）归英文——非中文用户读英文比读中文更接近习惯。
- 为什么拿不到偏好语言（列表为空）时归英文而不是 `Locale::default()`（中文）：这里是**探测**而非**缺省**；探测不到答案时给国际默认值（英文）比给项目母语更保守，也避免“因为读不到所以突然变中文”这种随环境漂移的行为。
- 为什么不把这段判据放进 core：core 不得依赖平台栈，且这条规则的价值只在“平台读到了什么”这一侧；core 侧对应的是 `Language::resolve` 这条纯映射（见 config_comment.md）。
- 这条“非中文即英文”的口径同时是 R5“纯英文 locale 下跳过 CJK 字体加载”的判据来源：R5 复用同一份探测结果，不另开第二处系统语言读取。

## 符号契约（自代码注释迁入）

- `locale_for`：只有 `zh*` 归中文，其余语言（含读不到偏好）一律英文——文案只有中英两套。
