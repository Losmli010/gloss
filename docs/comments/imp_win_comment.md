# imp_win.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## 发现器分派与形态（2026-11-06，Windows 移植 P2②）

- 本文件是系统（Windows）的真实发现器，接替占位桩（imp_stub.rs，现只服务其余
  目标）；类型面（`FamilyFont`/`FontBacking`/`load_backing`）与平台（macOS）实现
  同构镜像，`from_bytes` 仅测试构造入口，镜像缘由见 imp_stub 记录。
- **DirectWrite 枚举优先、固定候选文件探测退化**（方案 §2 映射表定案）：枚举路
  经系统字体集合按家族名定位常规面，面序号由 `GetIndex` 直接交出——免去
  macOS 侧的 ttc PostScript 名解析整套原语；文件探测路只在枚举整体不可用
  （字体服务异常等）时兜底中日韩后备，命中微软雅黑（`msyh.ttc` 0 号面）与
  宋体（`simsun.ttc` 0 号面），面序号按文件内已知布局写死在候选表。
- **候选名单仍由父模块传入**（P0 定案不变式：签名两平台一致，分派点零 cfg）；
  Windows 家族名（微软雅黑、雅黑 UI、宋体）追加在父模块中日韩名单尾部，
  macOS 前序候选在真机上恒命中，故 macOS 行为不变。
- **家族名用英文名查 `FindFamilyName`**：DirectWrite 自带 locale 回退
  （en-US 兜底），不随系统显示语言漂移；常规面按
  `GetFirstMatchingFont(NORMAL/NORMAL/NORMAL)` 取，与 macOS 侧“风格修正位加
  权重漂移”的选择取向一致。
- **文件引用还原路径不假设引用键的内存布局**：经本地字体文件加载器的
  `GetFilePathLengthFromKey`/`GetFilePathFromKey` 还原完整路径，而不是把引用
  键直接当 UTF-16 串解读——后者依赖加载器实现细节。
- **失败只降级不报错**：工厂或集合不可用、单家族装载失败都留 warn/debug 痕后
  继续或交回父模块的既有降级路径（候选全缺席时命名字体族兜到内置字形），
  与 macOS 侧同形；枚举耗尽转探测路时留一条 debug 痕标记退化发生。

## 字节背衬装载（`load_backing`）

- 与 macOS 实现逐字同构：mmap 文件背页优先（egui 登记的借用切片恒指回文件
  页，字节不进进程私有堆），映射失败留 warn 后整包拷贝兜底，两路都失败才报
  错交由调用方降级。SAFETY 不变量同款：系统字体文件进程内只读、映射长度取
  打开时的文件长度。
