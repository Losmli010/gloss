"""中文区规则的单测：字符面派生判罚、数字附随邻接、盘古之白相邻、简体判
定、表情事实对账、注记括号单语与配对、锚点连贯软约束、指令段豁免。

被测入口是规则函数，直接传路径、行号与表述文本，返回带规则标识的违规条目，
不落进程；断言取条目消息核对判罚片段；用法见注释门禁与提交信息门禁的单测
入口。
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parents[1]))
sys.path.insert(0, str(Path(__file__).parents[1] / "comments"))

import check_rust_comments  # noqa: E402

from language_spec import chinese, coderef, english  # noqa: E402


def messages(findings: list[chinese.gate_report.Finding]) -> list[str]:
    return [finding.message for finding in findings]


def test_gloss_allowed() -> None:
    assert chinese.body_problems("t", 1, "中文头借用图形库（wgpu）渲染。", "header") == []


def test_bare_word_flagged() -> None:
    problems = chinese.body_problems("t", 1, "中文头说明 wgpu 渲染。", "header")
    assert any("outside the derived whitelist" in p for p in messages(problems))


def test_nested_brackets_pass_and_stay_single_language() -> None:
    assert chinese.body_problems("t", 1, "主线程（窗口库（winit）循环）。", "header") == []


def test_nested_brackets_judged_per_level() -> None:
    problems = chinese.body_problems("t", 1, "主线程（窗口库（winit 循环）驱动）。", "header")
    assert any("mixes" in p for p in messages(problems))


def test_unpaired_bracket_flagged() -> None:
    problems = chinese.body_problems("t", 1, "中文头（缺右括号。", "header")
    assert any("unpaired" in p for p in messages(problems))


def test_stray_close_bracket_flagged() -> None:
    problems = chinese.body_problems("t", 1, "中文头）多出单边括号。", "header")
    assert any("unpaired" in p for p in messages(problems))


def test_unclosed_bracket_content_checked_as_body() -> None:
    problems = chinese.body_problems("t", 1, "版本（v1.2 缺右括号。", "header")
    assert any("unpaired" in p for p in messages(problems))
    assert any("outside the derived whitelist" in p for p in messages(problems))


def test_decimal_point_allowed() -> None:
    assert chinese.chinese_zone_problems("t", 1, "版本 0.31 发布", "doc") == []


def test_halfwidth_punct_flagged() -> None:
    assert chinese.chinese_zone_problems("t", 1, "hello, world", "doc")


def test_gloss_pure_foreign_term_allowed() -> None:
    assert chinese.body_problems("t", 1, "登录页（login）校验。", "header") == []


def test_gloss_mixed_runs_flagged() -> None:
    problems = chinese.body_problems("t", 1, "中文头（用 wgpu 渲染）说明。", "header")
    assert any("mixes" in p for p in messages(problems))


def test_gloss_single_letter_run_passes() -> None:
    assert chinese.body_problems("t", 1, "缓存（L 值）语义。", "header") == []


def test_gloss_kana_counts_as_foreign() -> None:
    problems = chinese.body_problems("t", 1, "渲染（テスト 混排）失败。", "header")
    assert any("mixes" in p for p in messages(problems))


def test_gloss_inner_content_skips_body_whitelist() -> None:
    assert chinese.body_problems("t", 1, "阈值（好@例子）达标。", "header") == []


def test_pure_chinese_parenthetical_allowed() -> None:
    assert chinese.body_problems("t", 1, "吞吐（每秒笔数）达标。", "header") == []


def test_directive_segment_exempt() -> None:
    assert chinese.chinese_zone_problems("t", 1, "noqa: E402", "doc") == []


def test_chinese_punct_allowed() -> None:
    assert chinese.chinese_zone_problems("t", 1, "快、清、小、稳——“克制”。", "doc") == []


# ---- 数字附随符号：三族邻接放行 ----


def test_attached_measures_and_currency_pass_by_digits() -> None:
    line = "温度 25℃，夹角 45°，比例 50‰，花费 €5 或 ￥100。"
    assert chinese.chinese_zone_problems("t", 1, line, "text") == []


def test_attached_between_digits_pass() -> None:
    assert chinese.chinese_zone_problems("t", 1, "区间 1,000 至 3-4 天、2.5 倍。", "text") == []


def test_attached_symbols_flag_without_digit_neighbor() -> None:
    line = "占比 50 ％偏高，花费 € 5，货币 ￥ 写法。"
    assert chinese.chinese_zone_problems("t", 1, line, "text")


def test_scene_attached_members_pass_in_chinese_zone() -> None:
    # 分场拆分后的中文场景专属成员：摄氏华氏与万分率随数字放行，全角人民
    # 币符前导数字放行；共用面成员（千分率）照旧
    line = "气温 25℃ 与 72℉，汇率 5‰，万分率 30‱，价 ￥5。"
    assert chinese.chinese_zone_problems("t", 1, line, "text") == []


def test_latin_letter_after_degree_sign_flags_in_chinese_zone() -> None:
    # 英文排印形态（度符号加拉丁字母）在中文区判外文：（C）是拉丁字母，
    # 度符号本身随数字附随放行
    problems = chinese.chinese_zone_problems("t", 1, "气温 25°C 偏高。", "text")
    assert any(p.rule == chinese.RULE_PROSE_FOREIGN for p in problems)


def test_two_dot_ellipsis_flags_in_chinese_zone() -> None:
    # 家族豁免收紧后两点形态出局：中文省略号是双省略号，两点形态落白名单外
    problems = chinese.chinese_zone_problems("t", 1, "省略号两点‥形态要判。", "text")
    assert any(p.rule == chinese.RULE_PROSE_FOREIGN for p in problems)


def test_fullwidth_yen_flags_in_english_zone_by_width_net() -> None:
    # 英文区不查标点集与附随面，只查宽度网：全角人民币符在网内即拦（既有
    # 行为补用例锁定）；摄氏符不在网内属已知盲区，明示不补（宁漏勿误）
    problems = english.english_zone_problems("t", 1, "costs ￥5 in total", "doc")
    assert any(p.rule == english.RULE_PROSE_ENGLISH for p in problems)


def test_fullwidth_equals_percent_tilde_out_of_whitelist() -> None:
    problems = chinese.chinese_zone_problems(
        "t", 1, "降级＝有痕迹，范围 3～5 天，币种 ￥。", "text"
    )
    assert problems


def test_derived_whitelist_carries_all_three_faces() -> None:
    # 场景标点、破折号省略号家族与中性数学面各取其一，钉住并集派生；
    # 全角等号、百分号与浪纹线不在任何数据面
    for symbol in "、…≤":
        assert symbol in chinese.WHITELIST
    for symbol in "＝％～":
        assert symbol not in chinese.WHITELIST


# ---- 可运行命令段豁免 ----


def test_runnable_command_segment_exempt() -> None:
    line = "python3 scripts/hooks/check_demo.py <repo-root>，缺省为本脚本的上一级目录。"
    assert chinese.chinese_zone_problems("t", 1, line, "text") == []


def test_command_with_flags_and_placeholders_exempt() -> None:
    line = "cargo run -p gloss-eval --bin eval -- live [args ...]"
    assert chinese.chinese_zone_problems("t", 1, line, "text") == []


def test_command_mixed_with_chinese_still_flagged() -> None:
    problems = chinese.chinese_zone_problems(
        "t", 1, "python3 scripts/hooks/check_demo.py 仓库根", "text"
    )
    assert any("outside the derived whitelist" in m for m in messages(problems))


def test_english_prose_without_structural_char_flagged() -> None:
    # 以工具词开头、无命令形态结构性记号的英文散文句不豁免
    problems = chinese.chinese_zone_problems("t", 1, "just run the tests here", "text")
    assert problems


def test_english_sentence_with_period_flagged() -> None:
    problems = chinese.chinese_zone_problems("t", 1, "make sure it works.", "text")
    assert any("outside the derived whitelist" in m for m in messages(problems))


def test_math_operators_allowed_in_chinese_body() -> None:
    assert chinese.body_problems("t", 1, "满足 3 + 4 = 7 且 5 ≠ 6。", "text") == []


def test_greek_and_extended_math_flagged() -> None:
    problems = chinese.chinese_zone_problems("t", 1, "收敛于 ∞，夹角 θ。", "text")
    assert problems


def test_extended_han_ranges_pass_in_body() -> None:
    # 兼容区取例的字形也在简繁对串内：区段放行与字形判罚两层正交，本例只断
    # 言区段与相邻两层不罚，字形层随简体判定另查
    line = "扩展甲区（㐀）与兼容区（豈）随三段口径放行。"
    problems = chinese.body_problems("t", 1, line, "text")
    assert all(p.rule != chinese.RULE_PROSE_FOREIGN for p in problems)
    assert all(p.rule != chinese.RULE_PROSE_PANGU for p in problems)


def test_extended_han_counts_as_cjk_wise() -> None:
    # 区段口径统一后，扩展区与兼容区汉字进宽度启发面：英文区与行内代码不再漏判
    assert chinese.CJKWISE.search("㐀")
    assert chinese.CJKWISE.search("豈")


def test_latin_variable_still_flagged_without_markup() -> None:
    problems = chinese.body_problems("t", 1, "当 x = 1 时成立。", "text")
    assert any("outside the derived whitelist" in p for p in messages(problems))


def test_markdown_collision_signs_still_flagged() -> None:
    problems = chinese.body_problems("t", 1, "斜杠 / 与竖线 | 不放行。", "text")
    assert any("outside the derived whitelist" in p for p in messages(problems))


def test_nbsp_and_ideographic_space_allowed() -> None:
    assert chinese.chinese_zone_problems("t", 1, "列\u00a0宽\u3000撑住", "doc") == []


def test_control_character_still_flagged() -> None:
    assert chinese.chinese_zone_problems("t", 1, "混入\u0007响铃控制符", "doc")


# ---- 盘古之白：汉字与拉丁字母或数字相邻判罚 ----


def test_pangu_flags_han_digit_and_han_latin_adjacency() -> None:
    # 数字半边照罚盘古；字母半边随管辖收窄改判外文罚（相邻证据随消息下发）
    problems = chinese.chinese_zone_problems("t", 1, "版本3发布由通道2收尾。", "text")
    assert any("without a space" in m for m in messages(problems))
    problems = chinese.body_problems("t", 1, "中文word相连要补空格。", "header")
    assert any(p.rule == chinese.RULE_PROSE_FOREIGN for p in problems)
    assert all(p.rule != chinese.RULE_PROSE_PANGU for p in problems)
    assert any("joined without a space" in m for m in messages(problems))


def test_pangu_passes_with_space_or_punctuation_between() -> None:
    assert chinese.chinese_zone_problems("t", 1, "版本 3 发布，通道 2 收尾。", "text") == []
    assert chinese.body_problems("t", 1, "图形库（wgpu）渲染。", "header") == []


def test_pangu_does_not_judge_bracket_container_content() -> None:
    # 容器内部不判盘古之白，与生产侧的正文区间口径一致
    assert chinese.body_problems("t", 1, "通道（v3）下发。", "header") == []


def test_pangu_flags_digit_adjacency_only() -> None:
    # 盘古之白管辖收窄：数字相邻照罚，字母相邻不再单独出盘古罚——字母入正
    # 文必吃外文罚，补空格改法对字母段永不收敛，管辖归外文判罚
    problems = chinese.chinese_zone_problems("t", 1, "版本3发布由通道2收尾。", "text")
    assert any(p.rule == chinese.RULE_PROSE_PANGU for p in problems)
    problems = chinese.body_problems("t", 1, "中文word相连要补空格。", "header")
    assert all(p.rule != chinese.RULE_PROSE_PANGU for p in problems)
    assert any(p.rule == chinese.RULE_PROSE_FOREIGN for p in problems)
    assert chinese.PANGU_REMEDY == "keep one halfwidth space between Chinese and digits"


def test_foreign_finding_carries_adjacency_evidence() -> None:
    # 字母相邻证据并入外文判罚消息（信息不丢，锚点注记改法一并消除相邻）
    problems = chinese.body_problems("t", 1, "由wgpu驱动。", "header")
    foreign = [p for p in problems if p.rule == chinese.RULE_PROSE_FOREIGN]
    assert foreign
    assert "joined without a space (由w u驱)" in foreign[0].message
    assert all(p.rule != chinese.RULE_PROSE_PANGU for p in problems)


def test_foreign_remedy_prescribes_anchor_or_translation() -> None:
    # 外文判罚的定稿改法四句五路：锚点、译出、标点正当形、机器记号进反引
    # 号、贴附数字；只括注的旧路径（预设专名，谓词性外文词无锚点可挂）不再
    # 进指引
    remedy = chinese.FOREIGN_REMEDY
    assert "anchor or translate foreign terms" in remedy
    assert "give each punctuation mark its proper form" in remedy
    assert "put machine symbols in inline code" in remedy
    assert "reattach detached symbols to their digits" in remedy
    assert "proper nouns in fullwidth bracket annotations" not in remedy


def test_foreign_message_groups_hints_by_character_class() -> None:
    # 外文判罚消息按字母、标点、误置、异体、记号、附随六类给各自指引，各组
    # 只见自己的片段；误置与异体的替换映射逐项精确，串级组合（三点省略、度
    # 符号组合）先于单字符——句点不落全角句号、字母不落锚点指引
    message = chinese.chinese_zone_problems("t", 1, "中文word相连。", "text")[0].message
    assert "anchor or translate the letters (word)" in message
    assert "joined without a space (文w d相)" in message
    assert "fullwidth twins" not in message

    message = chinese.chinese_zone_problems("t", 1, "好!使用,逗号。", "text")[0].message
    assert "fullwidth twins (!->！ ,->，)" in message
    assert "anchor or translate" not in message

    message = chinese.chinese_zone_problems("t", 1, "占比 50％与＄5。", "text")[0].message
    assert "misplaced marks (％->% ＄->￥)" in message
    message = chinese.chinese_zone_problems("t", 1, "范围 3～5 天。", "text")[0].message
    assert "misplaced marks (～->—)" in message

    message = chinese.chinese_zone_problems("t", 1, "「词」与 1∶2 对版。", "text")[0].message
    assert "variant marks (「->“ 」->” ∶->：)" in message
    message = chinese.chinese_zone_problems("t", 1, "他想了很久...才说完。", "text")[0].message
    assert "...->……" in message
    assert "->。" not in message
    message = chinese.chinese_zone_problems("t", 1, "气温 25°C 偏高。", "text")[0].message
    assert "°C->℃" in message
    assert "anchor or translate" not in message

    message = chinese.chinese_zone_problems("t", 1, "加@前缀与列|列对版。", "text")[0].message
    assert "hold machine symbols in inline code or rewrite the wording (@ |)" in message

    message = chinese.chinese_zone_problems("t", 1, "占比 50 %偏高。", "text")[0].message
    assert "reattach the detached symbols to their digits (%)" in message

    # 附随面放行的合法字符不进消息组：数字间的句点与冒号（小数、时分）是合
    # 法形态，不指改全角孪生——指到全角句号过门禁而破句，指到全角冒号与数
    # 字间冒号的放行开口相抵
    message = chinese.chinese_zone_problems("t", 1, "版本 15.5 由wgpu驱动。", "text")[0].message
    assert "->。" not in message
    assert "fullwidth twins" not in message
    message = chinese.chinese_zone_problems("t", 1, "会议 15:23 由wgpu驱动。", "text")[0].message
    assert "->：" not in message
    assert "fullwidth twins" not in message


def test_unpaired_remedy_lands_clean() -> None:
    # 不配对括号按定稿改法补全后零判罚；删除选路不再进指引（内容落回正文
    # 含拉丁时派生新判罚）
    problems = chinese.body_problems("t", 1, "版本（v1.2 缺右括号。", "header")
    assert any(p.rule == chinese.RULE_PROSE_GLOSS_UNPAIRED for p in problems)
    assert chinese.body_problems("t", 1, "版本（v1.2）缺右括号。", "header") == []
    assert chinese.GLOSS_UNPAIRED_REMEDY == "complete the paired annotation bracket"


def test_mixed_remedy_lands_clean() -> None:
    # 混语括号按组合改法一步落地后零判罚；裸移回的半截选路删除
    problems = chinese.body_problems("t", 1, "中文头（用 wgpu 渲染）说明。", "header")
    assert any(p.rule == chinese.RULE_PROSE_GLOSS_MIXED for p in problems)
    assert chinese.body_problems("t", 1, "中文头（wgpu）渲染说明。", "header") == []
    assert chinese.GLOSS_MIXED_REMEDY == (
        "hold the foreign term alone in the bracket; move the Chinese wording back into the body"
    )


# ---- 改法收敛夹具表：机械层准入的不变量进门禁 ----


# 违规句、按定稿改法机械落地句与判定面三列成表，落地句同面复跑零判罚；
# 本表先收改法收敛阶段一的规范形（盘古收窄、六类替换映射、数字间冒号、
# 数字形态收窄、段内容贯通、孤反引号补配对、围栏注释整词删除与文件头注
# 记改写），后续阶段增行；今后新规则落地必须同批补行，改法不收敛当场红。
REMEDY_FIXTURES = (
    ("body", "圖形渲染管线由wgpu驱动。", "图形渲染管线由（wgpu）驱动。"),
    ("body", "中文头（用 wgpu 渲染）说明。", "中文头（wgpu）渲染说明。"),
    ("body", "版本（v1.2 缺右括号。", "版本（v1.2）缺右括号。"),
    ("body", "我们要fallback到旧版本。", "我们要（fallback）到旧版本。"),
    ("body", "中文word相连要补空格。", "中文（word）相连要补空格。"),
    ("body", "版本3发布、通道2收尾。", "版本 3 发布、通道 2 收尾。"),
    ("body", "好!使用,半角逗号。", "好！使用，半角逗号。"),
    ("body", "降级＝有痕迹，占比 50％，＄5。", "降级=有痕迹，占比 50%，￥5。"),
    ("body", "北京－上海，范围 3～5、3~5。", "北京—上海，范围 3—5、3—5。"),
    ("body", "「词」等等‥他想了很久...。", "用“词”标注等等……他想了很久……。"),
    ("body", "他想在 25°C 停，比例 1∶2。", "他想在 25℃ 停，比例 1：2。"),
    ("body", "占比 50 %偏高。", "占比 50%偏高。"),
    ("markdown", "使用$5与列|列对版。", "使用`$5`与列`|`列对版。"),
    ("markdown", "会议在 `15:23` 开始。", "会议在 15:23 开始。"),
    ("markdown", "退出码为 `0` 落盘。", "退出码为 0 落盘。"),
    ("markdown", "重试 `3` 次后停止。", "重试 3 次后停止。"),
    ("markdown", "版本 15.5 由wgpu驱动。", "版本 15.5 由（wgpu）驱动。"),
    ("markdown", "性能达标由`wgpu合成，通道 3 收尾。", "性能达标由`wgpu`合成，通道 3 收尾。"),
    ("markdown", "```bash\n# 参考 `just fmt` 运行。\n```", "```bash\n# 参考运行。\n```"),
    (
        "rust",
        "//!本文件負責`wgpu`渲染。\nfn main() {}\n",
        "//!本文件负责渲染，图形库（wgpu）。\nfn main() {}\n",
    ),
)


def test_remedy_fixture_table_rechecks_clean() -> None:
    # 夹具表双向断言：违规句在面内真违规（表行不腐化），落地句同面复跑零判
    # 罚——机械收敛层的判定口径
    for surface, violation, remedy in REMEDY_FIXTURES:
        if surface == "body":
            assert chinese.body_problems("t", 1, violation, "text"), violation
            assert chinese.body_problems("t", 1, remedy, "text") == [], remedy
        elif surface == "rust":
            assert check_rust_comments.check("demo.rs", violation), violation
            assert check_rust_comments.check("demo.rs", remedy) == [], remedy
        else:
            assert coderef.markdown_problems("d.md", violation + "\n"), violation
            assert coderef.markdown_problems("d.md", remedy + "\n") == [], remedy


# ---- 简体判定：繁体独有字形判罚，对串与生产侧共享 ----


def test_traditional_glyphs_flag_in_body_and_brackets() -> None:
    problems = chinese.chinese_zone_problems("t", 1, "這是繁體寫法。", "text")
    assert any("traditional-only glyphs" in m for m in messages(problems))
    problems = chinese.body_problems("t", 1, "软件（軟體）对照。", "header")
    assert any("bracket contains traditional-only glyphs" in m for m in messages(problems))


def test_simplified_text_passes_the_shared_pair_table() -> None:
    assert chinese.chinese_zone_problems("t", 1, "这是简体写法，软件设定不变。", "text") == []


def test_traditional_table_derives_from_the_shared_pair_file() -> None:
    # 对串单源：繁体字形取每对前字，与生产侧共享同一数据文件
    assert "這" in chinese._TRADITIONAL
    assert "这" not in chinese._TRADITIONAL


# ---- 表情事实：自数据源装入，白名单天然全覆盖 ----


def test_emoji_facts_stay_outside_the_whitelist() -> None:
    # 机制不变：表情判罚骑白名单（表情在白名单外），事实与生产侧同源互指
    samples = set(chinese.EMOJI_BARE) | set(chinese.EMOJI_VARIANTS)
    samples |= {chr(start) for start, _ in chinese.EMOJI_RANGES}
    assert samples
    assert all(ch not in chinese.WHITELIST for ch in samples)


def test_emoji_chars_flag_as_foreign_in_body() -> None:
    problems = chinese.chinese_zone_problems("t", 1, "好评\U0001f44d再见了。", "text")
    assert any("outside the derived whitelist" in m for m in messages(problems))


# ---- 括号锚点连贯：软约束候选，警告不计违规 ----


def test_anchor_candidate_is_advisory_with_rule_marker() -> None:
    problems = chinese.body_problems(
        "t", 1, "我们先（initialize the model and train it）再评估。", "header"
    )
    anchor = [p for p in problems if p.rule == chinese.RULE_PROSE_ANCHOR]
    assert anchor
    assert all(p.advisory for p in anchor)


def test_anchor_fires_on_function_words_and_long_multiword_content() -> None:
    assert chinese.anchor_reasons("initialize the model and train it")
    assert chinese.anchor_reasons("we first initialize model weights")


def test_anchor_stays_silent_for_terms_paths_and_single_words() -> None:
    # 宁漏勿误：术语清单、路径与单词塞入形态不出候选，归人工清查兜底
    for gloss in [
        "wgpu",
        "wgpu-xxx",
        "Kotlin/Swift/Dart/Elixir/Julia/TOML/TypeScript",
        "scripts/hooks/language_spec/chinese.py",
        "fallback",
    ]:
        assert chinese.anchor_reasons(gloss) == [], gloss
        assert chinese.body_problems("t", 1, "启用（%s）补位。" % gloss, "header") == []


# ---- 对账断言（语言规范统一阶段五）：两侧派生各与基准双向相等 ----

GOLDEN_ZH_PUNCT = "，。、；：？！“”‘’《》（）·／"
GOLDEN_EN_PUNCT = ",.:;!?\"'()-"
GOLDEN_MATH = "=+<>^±×÷≤≥≠≈→"
GOLDEN_DASH_ELLIPSIS = "—–…"
GOLDEN_ATTACHED_AFTER_DIGIT = "%‰°′″"
GOLDEN_ZH_ATTACHED_AFTER_DIGIT = "℃℉‱"
GOLDEN_ATTACHED_BEFORE_DIGIT = "€£¥₩"
GOLDEN_ZH_ATTACHED_BEFORE_DIGIT = "￥"
GOLDEN_ATTACHED_BETWEEN_DIGITS = ".,-:"
GOLDEN_EMOJI_BARE = "✅✨❌❤⚡⭐"
GOLDEN_EMOJI_VARIANTS = "\ufe0e\ufe0f"
GOLDEN_EMOJI_RANGES = (
    (0x1F000, 0x1F02B),
    (0x1F0A0, 0x1F0FF),
    (0x1F100, 0x1F1FF),
    (0x1F200, 0x1F2FF),
    (0x1F300, 0x1F5FF),
    (0x1F600, 0x1F64F),
    (0x1F680, 0x1F6FF),
    (0x1F900, 0x1F9FF),
    (0x1FA00, 0x1FAFF),
)
GOLDEN_RANGES = {
    "latin": ((0x00C0, 0x024F), (0x1E00, 0x1EFF), (0x2C60, 0x2C7F), (0xA720, 0xA7FF)),
    "han": ((0x3400, 0x4DBF), (0x4E00, 0x9FFF), (0xF900, 0xFAFF)),
    "kana": ((0x3041, 0x30FF), (0x31F0, 0x31FF), (0xFF66, 0xFF9D)),
    "hangul": ((0x1100, 0x11FF), (0x3130, 0x318F), (0xAC00, 0xD7A3)),
    "cyrillic": ((0x0400, 0x052F),),
    "greek": ((0x0370, 0x03FF), (0x1F00, 0x1FFF)),
}
GOLDEN_PAIR_COUNT = 3152
GOLDEN_PAIR_HEAD = "㑯㑳㑶㓨㗲㘚㜄㜏㜢㠏㠣㥮"
GOLDEN_PAIR_TAIL = "𨦫𨧀𨧜𨨏𨭆𨭎𨯅𩞯𩠴𩣑𩶘𰻞"


def test_char_faces_derive_bidirectionally_from_the_shared_golden() -> None:
    # 开发侧标点面与生产中文允许表的标点成分同源同集（生产侧基准锁在核心
    # 库测试），白名单按登记公式等于标点面并家族面并中性面，双向集合相等；
    # 附随两向取共用面并中文场景面的并集（场景差异成员住场景面）
    assert chinese.ZH_PUNCT == frozenset(GOLDEN_ZH_PUNCT)
    assert chinese.WHITELIST == (
        frozenset(GOLDEN_ZH_PUNCT) | frozenset(GOLDEN_DASH_ELLIPSIS) | frozenset(GOLDEN_MATH)
    )
    assert chinese.ATTACHED_AFTER_DIGIT == (
        frozenset(GOLDEN_ATTACHED_AFTER_DIGIT) | frozenset(GOLDEN_ZH_ATTACHED_AFTER_DIGIT)
    )
    assert chinese.ATTACHED_BEFORE_DIGIT == (
        frozenset(GOLDEN_ATTACHED_BEFORE_DIGIT) | frozenset(GOLDEN_ZH_ATTACHED_BEFORE_DIGIT)
    )
    assert chinese.ATTACHED_BETWEEN_DIGITS == frozenset(GOLDEN_ATTACHED_BETWEEN_DIGITS)
    assert chinese.EMOJI_BARE == frozenset(GOLDEN_EMOJI_BARE)
    assert chinese.EMOJI_VARIANTS == frozenset(GOLDEN_EMOJI_VARIANTS)
    assert chinese.EMOJI_RANGES == GOLDEN_EMOJI_RANGES


def test_ranges_derive_bidirectionally_from_the_shared_golden() -> None:
    # 区段三段口径（汉字含扩展甲区与兼容区）与六类区段逐对双向相等——扩展区
    # 汉字两侧同判的事实基座
    for name, golden in GOLDEN_RANGES.items():
        assert chinese._RANGES[name] == golden, name


def test_zh_and_en_punct_faces_stay_disjoint() -> None:
    # 中西允许表互斥（开发侧同断言，生产侧锁在核心库测试）
    assert chinese.ZH_PUNCT.isdisjoint(frozenset(GOLDEN_EN_PUNCT))


def test_traditional_set_derives_from_the_shared_pair_file() -> None:
    # 对串派生取每对前字：基数与首尾字形钉住派生方向，对串内容逐字基准锁
    # 在生产侧核心库测试（同一数据文件，两侧互指）
    assert len(chinese._TRADITIONAL) == GOLDEN_PAIR_COUNT
    for glyph in GOLDEN_PAIR_HEAD + GOLDEN_PAIR_TAIL:
        assert glyph in chinese._TRADITIONAL, glyph
    assert "这" not in chinese._TRADITIONAL


def test_ranges_reversed_token_raises_with_token_context() -> None:
    try:
        chinese._ranges("30-1F", "ranges.han")
    except ValueError as error:
        assert "reversed" in str(error) and "30-1F" in str(error)
    else:
        raise AssertionError("reversed range must raise with the token in context")
