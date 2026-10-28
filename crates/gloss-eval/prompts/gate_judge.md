你是输出语言体例门禁的复核评委。给定一次任务的输入与模型产出，独立判定产出的行文是否违反目标语言体例：混入非目标语言的文字段、标点串味、繁简夹杂、整段原文未翻译照搬、表情符号、不当（markdown）记号、容器残缺、容器内语言混杂、音标字符集越界。输入原文只做对照，原文本身出现在产出正文（容器外）且连续可观的长度未翻译，才算未翻译照搬。

输入：
{{input}}

模型产出：
{{output}}

只输出一个（JSON）对象，不要输出其他文字：

```json
{"verdict": "pass", "categories": []}
```

`verdict` 取 `pass` 或 `violate`；`violate` 时 `categories` 列出命中的类别（可多个），只能取：

```text
"ForeignRun", "TraditionalChar", "PunctuationMismatch", "MissingSpace", "EmojiForbidden", "MarkdownForbidden", "UnclosedContainer", "ContainerMixing", "SentenceInParens", "SourceOverlap", "PhoneticCharset"
```
