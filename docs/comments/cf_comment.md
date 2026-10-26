# cf.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## CfGuard

- CF 对象（+1 引用）的所有权守卫：出作用域即 CFRelease。
- `new`：接管一个 +1 引用；`NULL` 合法（Drop 时判空跳过）。
- `raw`：借用为底层 CF 引用（对任意 CF 对象类型通用，类型语义由调用方保证）。
- `string_ref`：借用为 CFString 引用（调用方保证该对象确实是 CFString）。
- `data_ref`：借用为 CFData 引用（调用方保证该对象确实是 CFData）。
- Drop 语义见代码内 `// SAFETY:` 注释：守卫是唯一释放点，恰好归还一次。

## cf_string

- 按 C 字符串构造 CFString（+1 引用，由守卫释放）；分配失败返回 `None`。

## cf_data

- 按字节构造 CFData（+1 引用，由守卫释放）；分配失败返回 `None`。
- 以 `#[cfg(test)]` 门控：仅测试路径需要从字节构造 CFData，生产构建零死代码。

## string_from

- 把 CF 对象转成 Rust 字符串：非 CFString、编码失败或 UTF-8 字节数超过 `max_bytes` 都返回 `None`（上界在分配缓冲之前判定）。
- Safety：`value` 是有效的 CF 对象引用或 NULL（内部先验类型）；+1 引用仍由调用方持有。
