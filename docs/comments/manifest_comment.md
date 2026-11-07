
## 平台键一键化（2026-11-07，Windows 移植 P3/D8.3）

- **schema 保持 1**：清单升级绝不换 schema 号——`Some(other)` 一律
  `UnknownSchema` fail-closed，升号会让已部署在外的旧版本更新检查立即报错
  （定案 D8.1/§7-C1）。Windows 扩展只是在 stable 表里多一个键。
- **强制键位从“darwin 双键”改“编译期平台 triple 一键”**：`parse` 校验
  稳定通道必须含 `current_arch()` 对应的条目。应用按编译目标只发布三个
  triple（双 darwin + Windows x64），编译期即可判定本机归属；其余目标
  （无发布）`current_arch()` 返回空键——任何清单都过不了矩阵，更新检查
  稳定落清单失败（诚实的未支持行为）。
- **双向兼容锁定**（两条内联测试）：新解析器读旧清单（darwin 双键、无
  Windows 条目）在 macOS 过、在无本机键的平台按 `Invalid` 拒；新清单
  （三键）解析后 darwin 双键仍在条目表里——老解析器与新解析器的差异只在
  键位强制，条目级校验同一份代码，故“解析过 + 双 darwin 键在场”即等价
  于老解析器放行（老版本字段全忽略，Windows 键不碍事）。

## into_version 所有权移交（本地门禁第 3 轮，2026-10-13）

- `UpdateManifest` 增加 `into_version(self) -> Version`：状态机锁定更新
  目标时版本随清单所有权移交，替代 `version().clone()` 的复制。读取型
  访问器 `version()` 保留给比较/展示路径。

## Artifact / UpdateManifest（清单数据形状）

- `Artifact`：平台条目的下载事实：zip 为客户端校验对象（size/sha256 都对它算），`dmg_url` 供站点使用、客户端不读（按未知字段忽略，且仅 darwin 平台的条目携带）。字段：`url` 整包 zip 的直链；`size` zip 字节数：响应体长度上限与 size 校验的依据；`sha256` zip 的 sha256（64 位十六进制小写）。
- `UpdateManifest`：校验通过的更新清单：只保留客户端要用的字段，架构条目展平存表。`version` 清单版本（已解析为 semver；相等或低于本地即无更新）；`artifacts` stable 频道的架构条目，键为 target 三元组。
- `version()`：清单版本。
- `into_version()`：取走清单版本：锁定更新目标时随所有权移交，不再复制。
- `artifact()`：按架构三元组取条目（矩阵保证本机平台键在场，`None` 属防御分支）。

## ManifestError（清单失败的三类形态）

- 后果一致（设置页显示“清单不可用”），分形变体只为日志能各说各话。
- `Unparsable`：body 不是合法 JSON，或字段缺失/类型不符（serde 拒收）。
- `UnknownSchema`：`schema` 值未知：fail-closed，拒绝解析。
- `Invalid`：通过 JSON 解析但校验矩阵不符（版本不可解析、本机平台条目缺、url/sha256 形态不符）。

## ManifestDoc（serde 视图）

- 只声明矩阵要核对的字段，其余（含 `dmg_url` 与展示用字段）一律忽略。

## current_version / current_arch / is_newer / parse / is_sha256

- `current_version`：本地当前版本（编译期常量）：Cargo 在构建期拒绝非法 semver 的 `version` 字段，回落分支不可达（有单测锁定）。
- `current_arch`：当前进程的架构键：编译期平台 triple（macOS 按 target_arch 细分双 darwin，Windows 取 x64，见平台键一键化条目）。
- `is_newer`：版本比较（semver crate 的全序）：清单版本高于本地才视为有更新，相等或更低一律无更新。
- `parse`：解析并校验清单 body：先 fail-closed 核 `schema`，再走校验矩阵（含本机平台键强制）。
- `is_sha256`：64 位十六进制小写（清单样例承诺的形态；混入大写或非 hex 按不符处理）。
