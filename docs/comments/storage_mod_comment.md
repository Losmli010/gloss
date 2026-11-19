# storage_mod 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## 沿革（2026-11-14，架构重构 A3）

- 本文件原为配置存储的整份宿主（文档半边、密钥半边与组合体同住一体，2026-09-19 落地，2026-10-30 端口拆分后 `SecretStore` 独立成块）；A3 把文档半边与组合体上移平台中立共享层（`common::storage`，决策与沿革见 storage 记录），本文件收窄为系统（macOS）的密钥半边绑定。密钥端口 `SecretStore` 的出厂实现落在 [`keychain::KeychainSecret`] 上：`secret`/`set_secret`/`delete_secret` 转发固有方法；密钥语义（服务名、条目定位）仍由本 crate 掌控。
- 出厂组合存储以类型别名落位：共享层的组合体接上钥匙串密钥半边，`new` 经共享层的 `with_secrets` 装配（配置文件在标准目录，密钥在出厂服务名下）。组装点（bootstrap）经本模块重导出的 `CompositeConfigStore::new` 注入，引用面不随共享层迁移变化。

## keychain 模块

- 密钥半边的系统安全存储实现（钥匙串读写）与进程内读取缓存见 keychain 记录；本文件只承载其端口实现，不复制密钥语义。
