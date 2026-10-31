# imp.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## `load_cjk`

- 依候选顺序查系统 CJK 字体，命中第一个就返回它的文件字节与面序号。

## `load_family`

- 依候选顺序定位第一个可用的系统字体家族；全部落空只留 debug 痕
  （宋楷是排版偏好，缺席不是故障）。

## `load_backing`

- 装载一个字体文件的字节背衬：mmap 文件背页优先，egui 登记的借用
  切片恒指回文件页、字节不进进程私有堆；映射失败（空文件、系统调用
  失败）留 warn 后回退 `fs::read` 整包拷贝——降级带痕迹、不阻塞启
  动。两条路都失败才报错。

## `select_regular_face`

- 家族集合里选常规面：先比风格修正位（粗斜宽窄，越少越常规），再比
  权重离常规（0）的距离——与 font-kit 时代 Weight NORMAL + Style
  NORMAL 的取向一致。集合缺席或为空按无此族处理。
- 查询走字体集合（只扫**已安装**字体，font-kit 同款形状）：描述符
  匹配会命中系统的“可下载字体”目录，查询缺席家族（未随机型的楷
  体等）会弹字体下载确认框。

## `face_index`

- 目标面在字体文件内的序号：单面文件恒 0；ttc 集合按头部面数逐面
  解析 name 表找 postscript 名的命中——序号完全由文件自身回答（与
  egui 侧解析器同源同约定），找不到即失败降级。不做 CoreText 的
  文件枚举：那在无窗口服务的 CI runner 上每次要挂十几分钟（fontd
  病态），纯字节解析是微秒级。

## `is_collection`

- 文件是否为 TrueType 集合（ttc/otc 共用的 `ttcf` 魔数）。

## `num_fonts`

- ttc 头部的面数（大端 u32，+8）；头部损坏按零面处理。

## `face_offset`

- ttc 头部第 `index` 个面的目录偏移（大端 u32，从 +12 起）。

## `locate_name_table`

- 在 `face` 处的 sfnt 表目录里定位 name 表。TTC 的表偏移按规范从
  文件头量起（实测 Apple 系统 ttc 即如此），与 egui 侧字形解析器
  （skrifa）支持的唯一约定一致——面相对等其它约定一概不认，免得
  复核通过却登记一张解析不出的面。

## `face_postscript_name`

- 解析 `face` 目录指向那面的 name 表，取 postscript 名（nameID 6；
  优先 Windows 平台 UTF-16BE，退回 Macintosh ASCII）。只为
  [`face_index`] 的序号定位服务，解析不了返回 `None`。
