# manifest_hygiene_test.py（约束门禁·清单卫生域单测）记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md「注释纪律」节），测试的验收意图与辅助契约记录在此。

## mut_dev_dep_enables_test_util（历史用例名）

- 2026-09-18 dev-dependencies 里开 test-util 是合法用法：测试要拿桩，生产不需要。（用例随检查退役后此处仅作沿革。）

## mut_dep_default_features_on_next_line

- 2026-09-18 default-features 写在跨行内联表的后半段：只看首行会误报，必须拼全条目再判定。

## unclosed_dep_entry_at_eof_still_emitted（旧实现分歧）

- 2026-10-24 原注细节：旧 bash/awk 版实现没有 END 块，依赖条目在文件尾仍待定（花括号未闭合）时会静默丢弃判罚；Python 版有意补发——这是审慎分歧，别「对齐」回旧行为。

## 死条目的证据形态矩阵

- 路径引用（含根清单的源码目录）、重导出与别名导入、宏调用、测试目录引用各自一条绿用例；连字符键转下划线匹配有一条红一条绿；dev 依赖段与根清单各有红用例；放行标记一条绿用例。
