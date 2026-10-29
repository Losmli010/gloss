# model.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），代码内不留指向本文件的指针。

## GlossError::JsonModeUnsupported（2026-10-11，契约校验与重试方案 §2/§7）

- 新变体而不是复用 `EngineResponse(诊断)`：失败卡要给「打开设置」出口与专属文案（「当前模型服务不支持结构化输出模式」），动作映射表按变体分派——协议错误的串载荷做不到按类别给出口，加变体是唯一的类型化通道。
- 变体不带诊断载荷：这是「环境问题」类目（同 `UnsupportedModality`），重发无益、重试按钮不出现，服务端文案对用户没有行动价值；具体诊断可从日志侧查。
- 变体语义：模型服务不支持 JSON 输出模式——请求恒带 `response_format` json_object（不做能力协商），端点以 4xx 拒绝且诊断文本提到它时按此建档——换支持该模式的模型服务是唯一出路，重试无益。

## Lang

- 任务的**产物**语言（译文给谁看）：变体名即 ISO 639-1 语言码，`Zh` 指简体。
- 2026-10-30 语言收敛（语言规范统一阶段四 D5）：变体收敛为 `Zh`/`En` 两态并改 `Copy`/`Default`（缺省中文）；日语、韩语、法语与手填名（`Other`）退役。存量配置里的退役值由 `Config` 反序列化时有痕降级到缺省（见 config_comment.md 的 `deserialize_target_lang`），枚举不再承载兜底变体——「未知语言」在系统里没有合法产物形态，有痕降级比跳过判定的并集策略诚实。

## Locale

- 应用自身的**界面**语言：prompt 模板与界面文案的选表依据。与 `Lang` 分工不同——`Lang` 决定译文语言，`Locale` 决定应用跟用户说什么话，两者解耦。
- `Config::language` 的 `System` 在壳侧按系统语言落定后只剩 `Zh`/`En` 两个具体值，所以这里没有第三态；`Zh` 出厂默认。

## ScreenPoint

- 屏幕逻辑坐标点。多显示器下坐标可为负，按手势/系统返回值原样传递。

## GlossError

- 全链路统一错误：状态机 Error 态与重试策略都按变体分支，不允许 panic 逃出主循环。
- `SelectionUnavailable`：选区读不到（应用不支持选区属性或 AX/兜底通道失败）。
- `SelectionEmpty`：选区为空（应用支持选区属性、如实报告了空值）。与「读不到」分开建档：状态机对手势触发静默收回。
- `AccessibilityDenied`：辅助功能权限缺失。
- `ImageUnavailable`：剪贴板图片读不出（板上无可用 flavor、粘贴授权被拒、粘贴板不可观察）或解不了码。与 `SelectionUnavailable` 分开建档：图像任务没有可选的文本，失败指引是「重新复制」。
- `ImageTooLarge`：剪贴板图片超出字节总量或像素面积上限（超限报错，不截断）。
- `UnsupportedModality`：任务所需模态与配置的模型能力不匹配（如图像任务未配视觉模型），或任务类型与输入模态的组合非法（见 `Task::new`）。
- `EngineNetwork`：网络错误，可重试。`EngineAuth`：API key 无效或过期，不可重试。`EngineRateLimited`：触发限流，可退避重试。
- `EmptyCompletion`：传输层收到「成功收尾但一个增量都没有」的响应：坏**回复**而非坏请求，任务段据此转 `Violation::EmptyReply` 进修复轮。独立变体而非复用 `EngineResponse` 的文本约定的原因：以错误文本内容作带内信号时，任何同文本路径都会被误判为坏回复。
- `EngineResponse(String)`：协议或响应解析异常，携带诊断文本（可能含服务端/用户原文——见 `Display` 实现的安全注释：载荷不进日志）。
- `Config(String)`：配置异常，携带诊断文本（同 `EngineResponse`，载荷不进日志）。

## GlossError 的 Display 实现

- 载荷可能携带远端或用户原文（服务端 `error.message`、解析不出的协议内容），而 `Display` 是日志出口（`error = %error` 遍布各 crate），把载荷打出来就是把敏感文本写进日志——违反 AGENTS.md「敏感文本不写进日志」的安全红线。
- 载荷只在 `Debug`（`?error`）与失败卡（`i18n::for_error` 显式解构变体，不走 Display）使用；`Debug` 同样不得进日志——日志宏禁止用 `?error` 打印 GlossError，它只留给诊断与测试断言。
