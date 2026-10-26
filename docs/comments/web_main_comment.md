# main.js 注释记录

- 下载契约细节：同源 manifest.json（CI 注入）提供 channels.stable[<triple>] 的
  dmg_url / url / size 三个字段；校验走 schema === 1。
- 键盘契约出处：ARIA menu 模式（方向键循环、Home/End 跳转、Esc 关闭回焦、
  Tab 走默认顺序并收起菜单）。
- 架构识别的原口径：渲染器名含 "Apple M2"/"Apple GPU" 判新架构，含
  "Intel/AMD/NVIDIA" 判旧架构；UA-CH 为次选（architecture/platform 熵值）。
