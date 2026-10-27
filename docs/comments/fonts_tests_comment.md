# fonts.rs 测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md「注释纪律」节），测试的验收意图与辅助契约记录在此。

## 测试行为速览（BDD）

已统一迁往 docs/tests/bdd.md（git 追踪，以测试代码为唯一事实源）；本文件只留缘由与形态说明。

## 辅助契约

- `SAMPLE_FONT`：合成字节串，让「登记是借用、不是自有」可证伪（宿主无关）；真实字体的
  加载路径由 `system_cjk_font_*` 两测覆盖（依赖宿主机，bdd.md 已标注）。
- `registered_bytes`：读出 `FontDefinitions` 里登记的字节；登记为自有（`Cow::Owned`）直接
  panic——即「必须借用」这条契约的判据。

## 缘由与形态

- **为什么新增「无系统字体」一测**：`apply` 的 `None` 分支过去只在 `install` 里被跳过，
  没有任何断言；它决定「找不到系统字体时不写字体表、不动字体族」，是失败降级的唯一出口。
- **为什么共享断言分两处**：`cjk_fallback_shares_bytes_across_contexts` 用合成字节钉住
  「两份字体定义登记到同一地址」（宿主无关）；`system_cjk_font_is_loaded_once` 用真实
  字体钉住「两次取字节返回同一段内存，且两份定义仍指向同一地址」——后者才是端到端的
  「一次加载、两窗口共享」判据。变异验证：`from_static` 换成 `from_owned(bytes.to_vec())`
  时共享测红；去掉缓存每次重载时只加载一次测红（且耗时翻倍）。
- **未覆盖项**：`try_unwrap` 成功路径本身（需全局分配器计字节才能证伪，macOS 走路径加载
  时回退分支实际不可达，故回退分支加了 `debug!` 便于事后发现）；验收口径里的「日志恰好
  一条定位行」「内存约减半」由真机测量与 PR 记录承载，不进测试（AGENTS.md 禁计时类断言）。
- **候选循环的 `continue` 分支不可证伪**：`imp` 硬编码 `SystemSource::new()`，没有注入缝；
  与既有实现同形，未在本任务引入。

## CoreText 直取重写后的测试形态（2026-10-09，PR #146）

- `sample_font`：合成字节进进程内 `OnceLock`、以 `&'static` 交给 `insert_face`——生产
  字节本就出自 `OnceLock` 缓存、登记签名要求 `'static`，测试与生产同构。2026-10-13
  （P0-1）起 `FamilyFont` 字节改为 mmap 背衬枚举，合成字体经 `FamilyFont::from_bytes`
  （`#[cfg(test)]` 的整包拷贝构造入口）构造——测试仍持拷贝形态，登记借用与指针比对的
  判据不变。
- `synthetic_ttc`：最小 ttc = 16 字节头（ttcf + 版本 + numFonts=1 + 目录偏移）+ 单面
  sfnt（1 张 name 表）+ 单条 name 记录（Windows 平台 UTF-16BE 的 nameID 6）；表偏移按
  规范从文件头量起，`FACE_DIR=16`、`NAME_TABLE=44` 两个常量即布局。
- 两条负例（截断输入 / 越界表偏移）：解析器全部界检读取，任何畸形输入必须安全返回
  「无」并走候选降级，不许 panic。
- 失效说明：本文件早前关于 `try_unwrap`/`SystemSource` 注入缝的条目随 font-kit 路径
  删除而失效；`SAMPLE_FONT`/`registered_bytes` 的借用判据不变（指针比对改对
  `sample_font().bytes`，因登记源已是缓存副本而非静态字面）。
