# bootstrap_mod 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## Platform（平台装配句柄，2026-11-05，Windows 移植 P1 · 方案 D3/D4）

- 形状：一个结构持全部平台能力——六个新端口加既有端口（场景探针、配置存储、密钥存储、图标）的契约对象，加已解析的系统语言，加事件源工厂的取材包（`EventKit`）与手势源槽位。只在进程入口组装段构造一次（`Platform::connect`），事件线程消费件取走后句柄本体移交应用层（gloss-app）持有；方向单一：入口 → 应用层，应用层不回触构造，本包不依赖应用层。
- `connect` 只写一份（跨平台共享）：各平台的同名镜像实现（组合存储、场景探针、系统语言、图标与 `SystemMenuController`／`SystemClipboardWriter`／`SystemPermissionProbe` 等适配）经入口按目标重新导出的模块路径取用——延续 P0 的“消费方引用面不随编译目标变化”，构造表达式也不随目标变化。构造只做廉价装配：不弹授权对话框、不建监听线程。
- 手势监听不进 `connect`：授权对话框与监听线程建立的时点保留在入口既有预检段，由 `spawn_gesture_source` 显式建立；建立失败时手势源保持缺席（`None` 即“无手势源”的既有降级语义）并置降级标志。
- `EventKit`（事件源工厂的取材包）：剪贴板监视源与文本取材、图片取材读取器在此互连（观察记录喂竞态比对、轮次标记喂写盘归因），平台内部的观察者与轮次类型不出包；`clipboard_watch_enabled` 共享原子由调用方传入（设置开关与应用层共用同一份）。
- 事件线程句柄由句柄本体持有，进程退出（应用层释放句柄）时汇合（`Drop`）：通道接收端（应用端点）先于句柄释放，事件线程随之退出，汇合立即返回——替代组装段在入口的显式汇合，行为等价（线程停止日志不变）。
- `EventSink` 与 `EventSources` 随本模块重新导出：消费方（组装段、应用层）经装配契约引用事件面类型，不触平台目录——`gloss_platform::` 在 `crates/gloss-app/src` 与 `src/` 各只剩装配契约一处引用（P1 验收硬指标）。
- cfg 边界：本模块只在系统（macOS）与系统（Windows）目标编译（引用平台分派的事件面类型），其余目标无装配契约——入口本就不在这些目标编译，与 P0 的 linux 占位一致。
- `event_thread` 字段为公开：应用层测试桩要整结构构造句柄；汇合语义仍由 `Drop` 承担，手动取走句柄后 `Drop` 对 `None` 无操作。

## 图标适配器构造点随动（2026-11-07，Windows 移植 P2④）

- `SystemAppIcon::new`（类型随两侧适配器统一正名，见 icon 与 windows_icon 记录）：构造点平台无关，名字不再叫 macOS 侧的旧名。

## desktop_shortcut 字段（2026-11-10，Windows 移植 P3b 后续）

- 新端口 `DesktopShortcut` 进句柄（`Box<dyn DesktopShortcut>`，紧随 `app_icon`）：桌面入口装配与图标装配同属启动期桌面集成，组装段在 `on_waker` 里与 `install_app_icon` 并列调用；构造仍只做廉价装配（`connect` 只 new 适配器，落盘在显式 `ensure` 时发生），与“构造不弹框、不建线程”的既有口径一致。
- 各平台同名镜像（`crate::shortcut::SystemDesktopShortcut`）：系统（Windows）重建快捷方式，系统（macOS）恒无操作。

## events 字段删除（2026-11-12，架构重构 A7 P0）

- 形状收口：`Platform` 不再持 `events: Option<EventKit>` 槽位——本字段自
  2026-11-05 建档以来全仓只被赋 `None`，取材包实际由静态工厂
  （`Platform::event_kit`）在入口组装段构造后直接移交平台事件线程，不经
  句柄中转；字段最初按“入口从句柄上取走取材包”的口径随句柄建立，装配
  落地走了工厂路径后它再无写读面（零行为变化，纯死字段删除）。取材包的
  互连口径见上文 `EventKit` 小节，不变。

## Platform 拆为 PlatformPorts 与 EventHost（2026-11-17，架构重构 B1/B2）

- 拆分动机（方案 B1）：应用层（gloss-app）对平台的依赖只为整只装配句柄
  一个类型，而它真正消费的只有菜单装配与剪贴板写入两个端口；退出汇合
  又隐式依赖应用壳字段声明顺序（endpoints 必须先于句柄释放）。拆分后
  `Platform` 只是入口的一次性包装（`connect` 返回 `ports` 与 `host` 两个
  公开字段，随即按消费方分发），应用层只收端口对象——gloss-app 的依赖
  清单去掉了 gloss-platform，约束门禁方向表同步收紧为 `"gloss-app":
  "gloss-core"`。
- `PlatformPorts`（纯端口与系统值，无 `Drop`）：场景探针、配置存储、密钥
  存储、系统语言、图标、桌面入口、菜单装配、剪贴板写入、权限探针九项。
  消费方分发口径：菜单与剪贴板写入进应用环境（`AppEnv`），图标、桌面
  入口与权限探针留在入口组装段（移入 `Assembly`），配置与密钥存储供配置
  装配取用（`load_config` 改收端口袋引用）。
- `EventHost`（事件宿主）：手势源、降级标志与事件线程句柄；`spawn_gesture_source`、
  `event_sink`、`spawn_event_thread` 与静态工厂 `event_kit`（原
  `Platform::event_kit`）都归它。`Drop` 汇合事件线程——退出不再依赖任何
  结构体字段的声明顺序：入口在 `gloss_app::app::run` 返回后显式
  `drop(host)`，此刻应用端点（通道②发送端）已随 run 返回而释放，事件
  线程见断连退出，汇合立即返回。锁定测试见 bootstrap_mod_tests 记录。
- `event_thread` 字段由公开改为私有：原公开的唯一理由是应用层测试桩要
  整结构构造句柄（`stub_platform`），应用层改收端口对象后该桩随 B2 删除，
  宿主只经本模块的 `connect` 构造。`gesture` 与 `gesture_degraded` 保持
  公开：入口组装段要取走手势源、克隆降级标志。
- `connect` 的构造体仍只写一份（跨平台共享）：`ports` 与 `host` 两个子
  结构在同一个构造表达式里装配，平台镜像实现经重新导出的模块路径取用，
  构造只做廉价装配的既有口径不变。
