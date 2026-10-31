# settings_session.rs 测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md“注释纪律”节），测试的验收意图与辅助契约记录在此。
> 2026-09-20 由 app_tests_comment.md 按 R4 拆分迁入。

## open_settings_request_starts_an_edit_session

- 2026-09-18 验收标准（M4-T6）：设置入口走 `PlatformEvent::OpenSettingsRequested`——打开编辑会话（草稿=当前快照），不占用请求代数（gen 保持 0）。

## settings_save_writes_keychain_and_swaps_config

- 2026-09-18 验收标准（M4-T6）：设置页保存 = 密钥进 keychain（按 provider 条目 `gloss/deepseek`）+ 配置走热更新路径；成功后关闭会话。密钥永不落进配置快照。trim 是 UI 层（build_save）的契约、已在 settings 模块单测，壳收到的是裁剪后的密钥；保存的配置对下一次触发生效（模型随任务下发）。

## clearing_the_key_deletes_the_secret_on_save

- 2026-09-18 清除密钥路径：保存时删除 keychain 条目（删除与配置落盘同一次保存里发生，取消不会留下已删除的密钥）。

## failed_save_keeps_the_session_open_with_a_notice

- 2026-09-18 落盘失败：内存保持旧版本（磁盘唯一真相），会话保持打开并带上提示——用户可以改完再存。

## saving_settings_rebinds_hotkeys_from_the_new_snapshot

- 2026-09-18 验收标准（M4-T7）：设置页改热键后**立即重注册**，且重注册拿到的是刚落盘的那份快照。启动注册属于组装点、不属于 App（call_count 从 0 起算）。热键不受“下一次触发才生效”约束——注册是平台侧的即时动作，等下一次划词再换就太晚了。

## every_save_rebinds_hotkeys_not_just_the_first

- 2026-09-18 契约是**每次**保存都重注册：只断言调用过一次，会放过“重注册被某个一次性条件挡住”这类回归（例如后来加个 `if !self.rebound` 短路）。第二次保存前要重开编辑会话（保存会关掉设置会话，等同用户再点一次设置）；第二次生效的必须是第二次保存的那份，不是第一次的。

## failed_save_does_not_rebind_hotkeys

- 2026-09-18 落盘失败不得重注册：磁盘是唯一真相，运行时快照没换，按键也就不该换——否则会出现“按下去是 A、配置文件里是 B”的分叉。
