# present/mod.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## 再导出（pub use render::…）

- 帧管线的对外入口只此四项（根包 L3 自检与生产 init 消费）；渲染细节留在 crate 内，render_frame 的返回类型一并可见。
