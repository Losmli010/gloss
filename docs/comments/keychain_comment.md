# keychain.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## 进程内缓存的代价

- 2026-09-19 缓存把钥匙串授权弹窗从每任务收敛到每进程一次；代价是进程运行期间绕开本实例的外部改动（如钥匙串 App 里手动删除）会读到旧值，重启后可见。

## 符号契约（自代码注释迁入）

- `SERVICE`：keychain 条目的服务名：与打包标识一致，避免与其他应用的条目串扰。
- `ERR_SEC_INVALID_OWNER_EDIT`：errSecInvalidOwnerEdit：条目所有权按创建二进制的代码签名归属，查询式 `SecItemDelete` 对非本二进制创建的条目以此拒绝删除。security-framework 系 crate 未导出该常量，本地定义。
- `KeychainSecret`：密钥存储：以 `account`（即配置里的 `keychain_id`）定位条目，服务名固定。仅做存取，不理解 key 语义——provider 与 key 的映射在 `Config` 侧维护。`Clone` 出的副本经 `Arc` 共享同一份缓存：组装点只建一个实例、克隆的 Arc 分发给引擎与设置页，读写因此天然汇合到同一缓存。
  - `service` 字段：以服务名定位条目，与应用条目隔离（换名入口 `with_service` 仅测试可用）。
  - `cache` 字段：读取缓存：值含 `None`（条目不存在也被缓存），写删路径同步更新。
  - `read_gates` 字段：未命中缓存首读的按 key 闸门：ACL 授权框阻塞首读期间（预热与任务请求可能并发读同一条目），后来的读者在闸门上等首读回填缓存，而不是各自撞一次系统授权框——「读弹窗每进程至多一次」因此对并发读取也成立。闸门只包住 keychain 读本身；写删路径不经它（键数由配置里的 keychain_id 决定，个位数级别，map 进程内只增不减不构成无界）。
  - `raw_reads` 字段：底层读计数：闸门的载荷契约（并发首读只发生一次底层读）在真 keychain 上不可观测，仅测试构建累计，测试据此断言。
  - `new`：用出厂服务名构造。
  - `with_service`：用指定服务名构造（测试隔离用）。
  - `cached`：取缓存；锁中毒（持锁期间 panic）取内值继续——缓存的临界区只做 HashMap 读写，中毒意味着进程已处于未定义状态，不能因此拒读密钥。
  - `read_gate`：取某 key 的首读闸门（无则建，克隆 Arc 出去再锁，避免持 map 锁等闸门）；锁中毒口径同缓存。
  - `get`：读密钥；`None` 表示条目不存在（明确区别于读取失败）。API key 约定为 UTF-8 文本；非 UTF-8 字节按 replacement 降级，不视为错误——密钥内容只该由本应用写入。未命中缓存的首读可能被 ACL 授权框阻塞到用户应答；在闸门上等待的并发读者是同步阻塞（不在 await 点上，途中的取消不生效，见模块内闸门文档）。回填让并发写入胜出：读期间发生的 set/delete 已把缓存更新到更新的状态，按缓存值作答、本读的旧值作废。先取闸门再上锁，锁内双检：等闸门期间并发的首读可能已回填缓存。
  - `set`：写入（或覆盖）密钥；成功才更新缓存：失败时缓存保持旧值。走「取引用删除 + 重建」：条目 ACL 按创建时的代码签名信任访问方（dev 构建每次重编译都换签名，旧条目不认新二进制），原地修改会触发系统授权弹窗；取引用删除不经过条目 ACL 与所有权（见 `delete_underlying` 文档），重建出的新条目归当前二进制所有，本构建内的后续读写免弹窗。删除成功而重建失败时，尽力用缓存里的旧值重建原条目（重建路径不再需要授权，失败多为环境性）；连重建也失败则如实上报，由设置页的既有失败通知兜底。残余风险：本进程从未成功读过该条目（缓存为空）时回滚无源，旧值不可恢复——不删除前补读是为了不把读路径的授权弹窗重新引进写路径。回滚尽力而为（值取自进程内缓存，与写入前状态一致）；回滚自身的失败只留痕——上报以最初的写入失败为准。
  - `delete`：删除密钥；条目本就不存在时视为成功（幂等，方便清理路径直调）。
  - `delete_underlying`：删除底层 keychain 条目（含同名重复项）；条目不存在视为成功。走「取引用 + `SecKeychainItemDelete`」而不是查询式 `SecItemDelete`：keychain 条目所有权按创建二进制的代码签名归属，查询式删除对非本二进制创建的条目返回 errSecInvalidOwnerEdit 拒删——dev 构建每次重编译都换签名，旧条目对当前二进制必然是外部所有者；引用式删除不受条目所有权限制，同名重复项也一并清掉。取引用只按 service/account 定位条目、不解密，不触发授权弹窗。该查询形态（generic_password + load_refs）上游只会交回 keychain item 引用，其它形态按上游契约不可达；真发生说明系统行为越出预期，留痕后跳过，不静默吞掉。

## 并发首读闸门（2026-10-08，issue #134 验收「同一进程内至多一次」）

- 原进程内缓存只对顺序读取保证每进程一框：预热（spawn_blocking）与任务请求（引擎每请求直查）并发未命中时各自进 Security framework，各自撞一次授权框。
- 备选：拿住缓存 map 锁包住整个 keychain 读（实现最简，但设置页保存路径的 set 也要拿同一把锁，授权框悬着时主线程的保存会被拖住）；改为按 key 闸门（`read_gates`），只串行未命中首读，写删路径与已缓存读取不经闸门。
- 闸门锁内双检：等闸门期间并发的首读可能已回填缓存。
- 回填语义（code review 意见落点）：`entry().or_insert` 让并发 set/delete 胜出——读期间发生写删时缓存已是更新状态，按缓存值作答、本读旧值作废；原先的 `insert` 会用旧值覆盖并发写入。
- 测试接缝（code review 意见落点）：真 keychain 无注入点，底层读次数不可观测；加 `#[cfg(test)] raw_reads` 计数字段，竞态测试断言 4 线程并发首读下底层读恰好 1 次（计数在闸门临界区内自增，join 后读数确定）。
- 已知边界（记录在案，不另改）：任务路径的 `resolve_api_key` 同步读密嵌在 async 块里，撞上未回填首读会在闸门/授权框上阻塞且取消不生效（阻塞段不在 await 点）——阻塞上界是一次授权框应答，闸门把并发首读收敛为一框；根治需把首读挪进 spawn_blocking，属后续优化。已在消费桥 PrewarmSecret 臂注释与 `send_secret_prewarm` 文档注明。

## 写删路径改引用式删除（2026-10-14，修「keychain delete failed: Invalid attempt to change the owner of this item」）

- 故障：设置页清除密钥或改配置保存，`set`/`delete` 的删除步骤报 errSecInvalidOwnerEdit（-25244）。实机 login keychain 里存在 2026-09-23 由旧构建创建的条目；当时实现假设「删除不受条目 ACL 管」，走查询式 `SecItemDelete`——该假设不成立：macOS 条目所有权按创建二进制的代码签名（partition ID）归属，查询式删除对非本二进制创建的条目以 -25244 拒删（dev 构建每次重编译都换签名，旧条目对当前二进制必然是外部所有者；未签名的 `target/debug` 二进制同理）。读路径有授权弹窗兜底，删除路径没有，于是直接报错。
- 实测依据（che-keychain issue #5 的系统行为探测）：外部所有者条目，`SecItemDelete` 返回 -25244/-25244 族错误；`SecKeychainItemDelete`（引用式）能删；取引用（`SecItemCopyMatching` 带 returnRef）不解密、不触发授权框。本机复现同型：`security` CLI 建的条目，旧代码删除路径失败、引用式删除成功。
- 方案：`delete_underlying` 统一走「`ItemSearchOptions` 取引用（`Limit::All` 连同名重复项一起清）+ `SecKeychainItemDelete`」，`set`/`delete` 共用；条目不存在（errSecItemNotFound）视为成功，幂等不变。
- 备选与取舍：① 查询删除失败再退回引用删除（少一次正常路径的引用查询，但两条路径都要养、且外部条目永远多打一次失败调用，不值）；② 改原地 `SecItemUpdate` upsert（che-keychain 实测外部条目可改值，但 ACL 不归当前二进制、读弹窗照旧，且不清重复项，放弃重建带来的「新条目归当前二进制、本构建内免弹窗」收益，不取）；③ 只把 -25244 映射成指引文案让用户手删（治标，每次重编译都要手删一次）。取引用式删除根治；-25244 仍映射为带操作指引的文案兜底（引用式删除理论上不再触达，留作系统行为变化的防线）。
- 依赖代价：`security-framework-sys`（`SecKeychainItemDelete` 与 `errSecItemNotFound`，security-framework 未封装删引用）与 `core-foundation` 0.10（`TCFType::as_concrete_TypeRef` 还原裸引用，版本须与 security-framework 3.x 内用的一致）升为直接依赖；原先「不值得为常量引入 sys」的注释随之删除。
- 回归锁定：ignored 测试 `foreign_owned_entry_is_removed_by_reference_delete` 经 `security` CLI 预置真外部所有者条目再删（登记 bdd.md 人工测试节）。
- 残余边界（记录在案，不另改）：外部所有者条目的**读**路径授权弹窗（每重编译一框）不因本次改动消失，仍是既有 dev 痛点；iCloud 同步产生的异地双条目若引用式删除后仍可能被同步拉回，属系统行为，指引文案已覆盖「再删一次」的操作路径。
