# gpu_comment 决策记录

> 按符号锚定(不写行号,防漂移)。改到相关符号时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库）,文件内不留指向本文件的指针。

## FONT_ATLAS_MAX_SIDE / DESIRED_MAX_FRAME_LATENCY(2026-10-16,内存优化方案)

## 决策记录

> 按符号锚定(不写行号,防漂移)。改到相关符号时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库）,文件内不留指向本文件的指针。

- 为什么字体图集从 8192 收到 2048:图集是 CPU+GPU 各一份的常驻纹理,边长上限直接放大它的高度上限(8192 档理论上限 256MB/上下文);划词卡片单帧字形量远填不满 2048 档,epaint 在 fill_ratio>0.8 时自动重建 Fonts,满档只是重排一帧的代价。设备级 max_texture_dimension_2d 保持 8192:它管的是用户纹理(经位附件图),与图集上限无关。

## - 为什么 desired_maximum_frame_latency 从默认 2 收到 1:划词浮层是低帧率 UI,1 帧在途已吃满 vsync;每省一帧少一张全窗尺寸 drawable 常驻(高分屏数 MB 级)。 决策记录

> 按符号锚定(不写行号,防漂移)。改到相关符号时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库）,文件内不留指向本文件的指针。

### MAX_TEXTURE_DIMENSION / CLEAR_COLOR

- `MAX_TEXTURE_DIMENSION`：egui 要把整个窗口画出来，纹理边长上限按 4K 屏取（与 egui-wgpu 自身的取值一致）。
- `CLEAR_COLOR`：浮层卡片之外的底色留空：窗口是透明的，卡片由 egui 自己画，圆角外露见桌面。

### GpuContext（设备级资源）

- instance / adapter / device / queue 全进程一份，所有窗口共用。
- `new`：同步等待 adapter 与 device 请求（pollster 就是为这一步引入的）。不指定 compatible_surface：设备先于窗口建立。可呈现的适配器与这里选到的是同一个。
- 访问器（`instance` / `adapter` / `device` / `queue`）：全进程共享。

### GpuSurface（一个窗口的渲染目标）

- surface + 配置 + egui 渲染器。egui 渲染器绑定 target format，而 format 由 surface 决定，所以渲染器随 surface 各持一份，不放在设备级；同一 adapter 下多个窗口选到的 format 相同，重复持有的只是几块缓冲区。
- `needs_configure`：尺寸已变但还没重新配置，在下一帧取帧前补上。
- `new`：为窗口创建 surface 并按当前尺寸完成首次配置。wgpu 自带的默认配置省去逐个字段填值，只覆盖我们关心的几项。
- `resize`：窗口尺寸变化。宽高为 0（最小化）时 wgpu 拒绝配置，等有真实尺寸再配。
- `adapter_name`：GPU 适配器名，供性能数据的环境记录使用。
- `render`：画一帧：应用 egui 的纹理增量 → 提交顶点与索引 → 渲染 → 呈现。取不到当帧（窗口被遮挡或取帧超时）或 surface 配置过期时静默跳过。纹理上传不依赖当帧能否取到：字体/图标增量若被跳过，下一帧就会引用不存在的纹理；epaint 另有 Drop 断言，增量必须显式消费掉。窗口被遮挡或取帧超时时跳过本帧：隐藏的浮层正属于前者，这里不能报错，否则每次隐藏都会刷一条日志。
- `upload_texture_deltas`：本帧的纹理增量上传（set 部分），跳帧与否都要做。
- `take_frame`：取当帧：正常与次优返回帧（后者标记待重配）；取不到（配置过期、被遮挡、超时、surface 不可用）返回 None，由调用方跳帧。
- `draw_and_present`：录制并呈现一帧：encoder 上登记缓冲区与 render pass，submit 后才释放 free 列表的纹理（这批命令可能还在用它们），最后 present。egui 的 renderer 要求 'static 生命周期的 pass；交出生命周期后本作用域内不再碰 encoder，运行时也不会有别名操作。释放必须排在 submit 之后：这批命令可能还在用它们。
- `discard_textures`：跳帧路径的增量收尾：set 已上传，free 的纹理没有在途命令，可直接释放。set 已应用，这里清空以满足 epaint 的 Drop 断言。

### pick_alpha_mode（透明合成模式选择）

- 透明窗口要选一个带 alpha 的合成模式，否则圆角外会露出黑边。预乘优先：egui 输出的是预乘 alpha 的颜色。

### GpuError

- GPU 初始化与 surface 失败：`NoAdapter` 没有可用的图形适配器；`Device` 请求逻辑设备失败；`Surface` 创建 surface 或选择其格式失败；`UnsupportedSurface` 该适配器不支持这个窗口的 surface。
