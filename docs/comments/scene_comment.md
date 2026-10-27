# scene.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关符号时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），代码内不留指向本文件的指针。

## SystemSceneProbe

- 2026-09-24 场景事实的两半来自两个不同的系统接口：安全输入来自 Carbon 的
  `IsSecureEventInputEnabled`（见 carbon_comment.md），前台应用来自
  AppKit 的 `NSWorkspace`。两者都包在这个适配器里，core 只看见
  `SceneFacts` 这个纯数据——判定（要不要拦）在 core，读取（怎么读）在这里。
- **读取发生在主线程**：`NSWorkspace::frontmostApplication` 与 UI 同属
  AppKit 的主线程亲和面，而触发判定恰好也在主线程（`drain_platform_events`）。
  因此不把这个探针挂到平台事件线程上去，省掉一层线程跳跃与随之而来的
  同步风险；代价是每次划词触发多一次主线程上的两趟系统调用，量级是微秒。
- 为什么 `SceneProbe` **没有失败面**（`facts()` 直接返回 `SceneFacts`，不返回
  `Result`）：读不到事实时的正确行为就是「当作没有敏感场景」——防护的默认
  姿态是不打扰，而不是把用户锁在门外（若读不到就拦，一次系统 API 变更就能
  让划词彻底失效）。这与 `SelectionReader`（读不到选区 = 任务失败，必须
  告诉用户）是两种不同性质的失败，所以两个端口的形态不同。
- 为什么前台应用的 bundle id 与显示名都是 `Option`：`NSRunningApplication`
  在少数情况下给不出身份（无 bundle 的进程、系统组件），此时 `matched_entry`
  只能靠能拿到的那个字段匹配；两个字段都缺则视为「无从匹配」，闸门放行。

## 测试面拆两半（2026-09-26 评审整改）

- CI 里那条只断「快照自洽」（报出前台应用则身份字段非空），**不断言安全
  输入的当前取值**：`IsSecureEventInputEnabled` 是全系统量，任何进程（钥匙串
  弹窗、登录窗、终端开着安全键盘输入）持有它都为真，断言「为假」等于断言
  环境事实，随时可红。Carbon 侧同理，改断「连续两次查询一致」。
- `NSWorkspace` 取前台应用这条路径在 CI 里跑不到 `Some` 分支（断言不能强求
  有前台应用），所以另加一条 `#[ignore]` 的 L4 用例
  `scene_probe_reports_the_frontmost_app`：给定有前台应用的图形会话，断言真
  报出应用与身份字段非空。CI 只留「调用不崩、快照自洽」，真机面归 L4。

## 符号契约（自代码注释迁入）

- `SystemSceneProbe`：每次 `facts()` 现读一次：安全输入态与前台应用都可能在两次触发之间变化，缓存会把旧状态带进闸门判定。
- `frontmost_app`：前台应用的标识：Bundle ID 与本地化名各取一次，无 Info.plist 的进程没有 Bundle ID；两项都取不到时返回 `None`。`is_self` 按进程号比对，用于让 Gloss 自己在前台时不响应划词手势。
