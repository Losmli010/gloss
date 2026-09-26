//! 敏感信息防护：触发前的场景闸门与发送前的内容启发式。
//!
//! 两条闸门都是**纯逻辑**：事实（安全输入态、前台应用标识、选区文本）由
//! 调用方从平台取来，本模块只判定——因此能在 L1 层以任意组合驱动，不需要
//! 真机，也不需要网络。
//!
//! 两者都是**无条件拦截**：命中即这次请求不成（场景闸门让触发不发生，
//! 内容闸门让取材产物止步于状态机），没有开关、没有名单编辑、也没有
//! 「仍然继续」这一路出口——防护不交由用户控制。判据只有类别出去：命中的
//! 原文既不进日志也不进错误消息，只留 [`SensitiveKind`] / [`TriggerBlock`]。
//!
//! 内容闸门只认**形状**：已知前缀 + 像凭据的主体、PEM 块、过 Luhn 的卡号、
//! 高熵串。形状判定必须容得下「用户实际选中的样子」——引号/括号/JSON/查询
//! 串里嵌着的令牌、手写的短假密钥、复制时混进零宽字符的令牌，都算命中；
//! 反过来，普通小写复合词（`sk-learn-scikit`）与谈论前缀的散文不算。

use std::fmt;

/// 触发被场景闸门拦下的前台应用名单：密码管理器与钥匙串访问——这些应用
/// 里的划词几乎只会是凭据（主密码、生成的口令）。
///
/// 内建常量而非配置项：名单一旦可被编辑，防护就有了旁路出口。匹配对象是
/// 前台应用的 Bundle ID 或应用名（不区分 ASCII 大小写），见 [`matched_entry`]。
const SENSITIVE_APPS: [&str; 7] = [
    "com.1password.1password",
    "com.agilebits.onepassword7",
    "com.bitwarden.desktop",
    "com.lastpass.LastPass",
    "com.dashlane.dashlane",
    "com.apple.keychainaccess",
    "com.apple.Passwords",
];

/// 前台应用标识：敏感应用名单的匹配对象。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FrontApp {
    /// `CFBundleIdentifier`，如 `com.1password.1password`。
    pub bundle_id: Option<String>,
    /// 面向用户的本地化应用名。
    pub name: Option<String>,
}

/// 触发前的场景事实：壳在取材前取一次，交状态机判定。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SceneFacts {
    /// 系统是否处于安全输入态（密码框聚焦）。
    pub secure_input: bool,
    /// 前台应用；探针取不到时为 `None`，闸门按「无事实」放行。
    pub front_app: Option<FrontApp>,
}

/// 触发被场景闸门拦下的原因：只记类别与命中的名单条目，不含选区内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerBlock {
    /// 安全输入态（密码框聚焦）。
    SecureInput,
    /// 前台应用命中敏感应用名单，携带命中的名单条目（原样，供日志对照）。
    BlockedApp(&'static str),
}

impl fmt::Display for TriggerBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SecureInput => write!(f, "secure input active"),
            Self::BlockedApp(entry) => write!(f, "frontmost app matches blocked entry {entry}"),
        }
    }
}

/// 触发前的场景闸门：`None` 即放行，`Some` 即不取材不触发。
///
/// 判定顺序与两个事实无关，只为「先判不依赖前台应用的那条」：安全输入态
/// 与前台是谁无关，先判它才不会因为拿不到前台应用而漏掉。
pub fn trigger_block(facts: &SceneFacts) -> Option<TriggerBlock> {
    if facts.secure_input {
        return Some(TriggerBlock::SecureInput);
    }
    let front = facts.front_app.as_ref()?;
    matched_entry(front).map(TriggerBlock::BlockedApp)
}

/// 名单匹配：Bundle ID 或应用名与 [`SENSITIVE_APPS`] 的条目逐字相等（ASCII
/// 大小写不敏感）即命中，返回名单里那一条（名单目前全是 bundle id）。
fn matched_entry(front: &FrontApp) -> Option<&'static str> {
    let identities = [front.bundle_id.as_deref(), front.name.as_deref()];
    SENSITIVE_APPS.iter().copied().find(|entry| {
        identities
            .iter()
            .flatten()
            .any(|identity| identity.eq_ignore_ascii_case(entry))
    })
}

/// 内容命中的敏感信息类别。类别进日志，命中的原文不进。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensitiveKind {
    /// 已知前缀的密钥/令牌（`sk-`、`ghp_`、`AKIA`…）或 `Bearer` 后接长令牌。
    Token,
    /// PEM 私钥块。
    PrivateKey,
    /// 高熵长随机串。
    HighEntropy,
    /// 通过 Luhn 校验的银行卡号。
    CardNumber,
}

/// 发送前的内容闸门：命中返回类别，未命中返回 `None`。
///
/// 只做高置信度模式，判定顺序与命中概率无关，只为「更确定的类别先报」：
/// 私钥块 → 密钥/令牌 → 卡号 → 高熵串（前者命中就不再往下猜）。
pub fn detect_sensitive(text: &str) -> Option<SensitiveKind> {
    // 网页与编辑器复制时常把不可见字符混进令牌中间，前面的词首判定会被它
    // 挡住。只在真含有它们时才拷贝副本（选区是热路径）。
    let cleaned;
    let text = if text.contains(&INVISIBLE[..]) {
        cleaned = text.replace(&INVISIBLE[..], "");
        cleaned.as_str()
    } else {
        text
    };
    if has_private_key_block(text) {
        return Some(SensitiveKind::PrivateKey);
    }
    if has_token(text) {
        return Some(SensitiveKind::Token);
    }
    if has_card_number(text) {
        return Some(SensitiveKind::CardNumber);
    }
    if has_high_entropy_token(text) {
        return Some(SensitiveKind::HighEntropy);
    }
    None
}

/// 词两端剥掉的标点：密钥常贴着引号、括号、markdown 强调符被复制走。
const TRIMMED: &[char] = &[
    '"', '\'', '`', '(', ')', '[', ']', '{', '}', '<', '>', ',', ';', ':', '!', '?', '*', '。',
    '，', '、',
];

/// 复制时会切断令牌的不可见字符：零宽空格/连接符、词连接符、BOM、软连字符。
const INVISIBLE: [char; 6] = [
    '\u{200b}', '\u{200c}', '\u{200d}', '\u{2060}', '\u{feff}', '\u{00ad}',
];

/// 已知密钥/令牌前缀（逐字匹配，大小写敏感——这些前缀本身区分大小写）。
/// `sk_live_` / `sk_test_` 是 Stripe 的两档 secret key，`sk-proj-` 那一类
/// 已被 `sk-` 覆盖。
const TOKEN_PREFIXES: [&str; 10] = [
    "sk-", "sk_live_", "sk_test_", "ghp_", "gho_", "ghs_", "ghu_", "ghr_", "xoxb-", "xoxp-",
];

/// 前缀之后算令牌的主体长度上限档：长过它一律算令牌。
const MIN_TOKEN_BODY: usize = 16;
/// 短主体档的下限：手写的假密钥（`sk-123456`、`sk-abc123`）就在这一档，
/// 长度够了还得**含数字或大写**——普通小写复合词（`sk-learn-scikit`）不能中。
const SHORT_TOKEN_BODY: usize = 4;
/// `Bearer` 之后的最小令牌长度。
const MIN_BEARER_BODY: usize = 20;
/// AWS access key id 的主体长度（`AKIA` 后 16 位）。
const AWS_KEY_ID_BODY: usize = 16;
/// 高熵串的最小长度与最小香农熵（bit/字符）。
const ENTROPY_MIN_LEN: usize = 32;
const ENTROPY_MIN_BITS: f64 = 4.5;
/// 卡号的位数区间（ISO/IEC 7812 的现行区间）。
const MIN_CARD_DIGITS: usize = 13;
const MAX_CARD_DIGITS: usize = 19;

/// PEM 私钥块：`-----BEGIN` 与 `PRIVATE KEY` 同现即算（`RSA`/`EC`/
/// `OPENSSH`/`PGP … BLOCK` 各变体都含这两段，不必逐个枚举）。
fn has_private_key_block(text: &str) -> bool {
    text.contains("-----BEGIN") && text.contains("PRIVATE KEY")
}
/// 密钥/令牌：按空白切词，逐词扫描前缀；`Bearer <token>` 是两词，单独判。
fn has_token(text: &str) -> bool {
    if text
        .split_whitespace()
        .any(|raw| is_prefixed_token(raw.trim_matches(TRIMMED)))
    {
        return true;
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    words.windows(2).any(|pair| {
        after_assignment(pair[0].trim_matches(TRIMMED)).eq_ignore_ascii_case("bearer")
            && is_token_body(pair[1].trim_matches(TRIMMED), MIN_BEARER_BODY)
    })
}

/// 赋值形态（`AUTH=Bearer ...`）的取值半边：切**最后一个**等号——令牌主体
/// 自己的字符集里没有等号，取最后一个更稳。
fn after_assignment(word: &str) -> &str {
    word.rsplit_once('=').map_or(word, |(_, value)| value)
}

/// 词内扫描已知前缀与 `AKIA`：命中要求前缀**前面不是字母数字**（词首，或
/// 前面是 `=` `:` `"` `/` 这类分隔符）——`{"key":"sk-xxx"}`、`key=sk-xxx`、
/// `“sk-xxx”`、`/sk-xxx` 因此都能命中，而英文复合词（`disk-space-2024`、
/// `risk-managed-portfolio`）不会。
fn is_prefixed_token(candidate: &str) -> bool {
    for (at, _) in candidate.char_indices() {
        let at_token_start = at == 0
            || !candidate[..at]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric);
        if !at_token_start {
            continue;
        }
        let rest = &candidate[at..];
        if rest.starts_with("AKIA") && is_aws_key_id(body_run(&rest["AKIA".len()..])) {
            return true;
        }
        if TOKEN_PREFIXES.iter().any(|prefix| {
            rest.strip_prefix(prefix)
                .is_some_and(|body| is_prefixed_body(body_run(body)))
        }) {
            return true;
        }
    }
    false
}

/// 前缀令牌的主体形状：两档长度（见 [`MIN_TOKEN_BODY`] / [`SHORT_TOKEN_BODY`]）。
fn is_prefixed_body(body: &str) -> bool {
    let len = body.chars().count();
    if len >= MIN_TOKEN_BODY {
        return true;
    }
    len >= SHORT_TOKEN_BODY
        && body
            .chars()
            .any(|ch| ch.is_ascii_digit() || ch.is_ascii_uppercase())
}

/// 前缀之后连续的主体字符：遇到别的字符即止——`"sk-abc123"` 的收尾引号、
/// `sk-abc123}` 的右花括号不该让前面的令牌作废。
fn body_run(rest: &str) -> &str {
    let end = rest
        .char_indices()
        .find(|&(_, ch)| !is_prefixed_body_char(ch))
        .map_or(rest.len(), |(at, _)| at);
    &rest[..end]
}

/// 前缀令牌的主体字符集：URL-safe 与标准 base64 都在内（`+` `/` `=` 常见于
/// 手写的假密钥，`Bearer` 与高熵那两条仍用窄字符集）。
fn is_prefixed_body_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-' | '+' | '/' | '=')
}

/// AWS access key id 的主体：长度够且全为大写字母或数字（它自己的字符集，
/// 与前缀表其余项不同）。
fn is_aws_key_id(body: &str) -> bool {
    body.len() >= AWS_KEY_ID_BODY
        && body
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
}

/// 令牌主体：长度够且字符全在 `[A-Za-z0-9._-]` 内（URL-safe base64、十六
/// 进制、JWT 段都在此列）。
fn is_token_body(body: &str, min_len: usize) -> bool {
    body.len() >= min_len
        && body
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

/// 高熵长随机串：单个无空白长词、字符全在 `[A-Za-z0-9._-]` 内、长度 ≥ 32、
/// 小写/大写/数字三类字符齐全、香农熵 ≥ 4.5 bit/字符。
///
/// 「三类字符齐全」是排除伪阳性的主力：长标识符（全小写加下划线）、路径
/// （含 `/`）、中文长句（非 ASCII）都过不了它。命中即拦截、没有旁路出口，
/// 所以这里宁可漏报：漏报只是这一次照常发送，误报却会让用户彻底失去这条
/// 路径的可用性。
fn has_high_entropy_token(text: &str) -> bool {
    text.split_whitespace().any(|raw| {
        let word = raw.trim_matches(TRIMMED);
        word.len() >= ENTROPY_MIN_LEN
            && word
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
            && has_all_three_classes(word)
            && shannon_bits_per_char(word) >= ENTROPY_MIN_BITS
    })
}

/// 小写、大写、数字三类字符是否齐全。
fn has_all_three_classes(word: &str) -> bool {
    let lower = word.bytes().any(|byte| byte.is_ascii_lowercase());
    let upper = word.bytes().any(|byte| byte.is_ascii_uppercase());
    let digit = word.bytes().any(|byte| byte.is_ascii_digit());
    lower && upper && digit
}

/// 香农熵（bit/字符）：字符分布的均匀度。随机串显著高于自然语言与命名，
/// 是「像不像随机生成」的量化判据。
fn shannon_bits_per_char(word: &str) -> f64 {
    let mut counts = [0usize; 128];
    let mut total = 0usize;
    for byte in word.bytes() {
        if let Some(slot) = counts.get_mut(usize::from(byte)) {
            *slot += 1;
            total += 1;
        }
    }
    if total == 0 {
        return 0.0;
    }
    let total = total as f64;
    counts
        .iter()
        .filter(|&&count| count > 0)
        .map(|&count| {
            let share = count as f64 / total;
            -share * share.log2()
        })
        .sum()
}

/// 卡号：13–19 位数字（组间允许单个空格或连字符）、过 Luhn 校验、且至少
/// 出现两种不同数字（全同数字串是填充号，不是卡号）。
fn has_card_number(text: &str) -> bool {
    let mut digits = String::new();
    let mut separated = false;
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            if digits.len() >= MAX_CARD_DIGITS {
                digits.clear();
            }
            digits.push(ch);
            separated = false;
        } else if matches!(ch, ' ' | '-') && !digits.is_empty() && !separated {
            separated = true;
        } else {
            if luhn_ok(&digits) {
                return true;
            }
            digits.clear();
            separated = false;
        }
    }
    luhn_ok(&digits)
}

/// Luhn 校验：从右往左，偶数位（0 基）翻倍、超 9 减 9，总和整除 10。
fn luhn_ok(digits: &str) -> bool {
    if !(MIN_CARD_DIGITS..=MAX_CARD_DIGITS).contains(&digits.len()) || distinct_digits(digits) < 2 {
        return false;
    }
    let mut sum = 0u32;
    for (index, byte) in digits.bytes().rev().enumerate() {
        let mut value = u32::from(byte - b'0');
        if index % 2 == 1 {
            value *= 2;
            if value > 9 {
                value -= 9;
            }
        }
        sum += value;
    }
    sum.is_multiple_of(10)
}

/// 数字串里出现过的不同数字个数（位掩码）。
fn distinct_digits(digits: &str) -> u32 {
    let mut mask = 0u16;
    for byte in digits.bytes() {
        mask |= 1 << (byte - b'0');
    }
    mask.count_ones()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filler(len: usize) -> String {
        "aB3".repeat(len).chars().take(len).collect()
    }

    fn entropy_filler(len: usize) -> String {
        const ALPHABET: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        const STEP: usize = 7;
        (0..len)
            .map(|index| {
                let slot = (index * STEP) % ALPHABET.len();
                char::from(ALPHABET.as_bytes()[slot])
            })
            .collect()
    }

    fn front(bundle_id: &str, name: &str) -> FrontApp {
        FrontApp {
            bundle_id: Some(bundle_id.to_owned()),
            name: Some(name.to_owned()),
        }
    }

    fn secure_input() -> SceneFacts {
        SceneFacts {
            secure_input: true,
            front_app: None,
        }
    }

    fn in_app(bundle_id: &str, name: &str) -> SceneFacts {
        SceneFacts {
            secure_input: false,
            front_app: Some(front(bundle_id, name)),
        }
    }

    #[test]
    fn scene_gate_blocks_secure_input_and_listed_apps() {
        assert_eq!(
            trigger_block(&secure_input()),
            Some(TriggerBlock::SecureInput)
        );
        assert_eq!(
            trigger_block(&in_app("com.1password.1password", "1Password")),
            Some(TriggerBlock::BlockedApp("com.1password.1password")),
            "the built-in list already carries the password managers"
        );
        assert!(
            TriggerBlock::SecureInput
                .to_string()
                .contains("secure input"),
            "the reason is read by humans in the log"
        );
        assert!(
            TriggerBlock::BlockedApp("com.apple.keychainaccess")
                .to_string()
                .contains("com.apple.keychainaccess"),
            "the blocked entry is echoed so the log names the list row that matched"
        );
        assert_eq!(
            trigger_block(&in_app("com.example.editor", "Editor")),
            None,
            "an app outside the list is not a scene to block"
        );
        assert_eq!(
            trigger_block(&SceneFacts::default()),
            None,
            "no facts and no reason to block"
        );
    }

    #[test]
    fn entry_matching_covers_every_identity_an_app_can_offer() {
        assert_eq!(
            matched_entry(&front("com.acme.vault", "Acme Vault")),
            None,
            "an app outside the built-in list matches nothing"
        );
        assert_eq!(
            matched_entry(&front("com.apple.Passwords", "Passwords")),
            Some("com.apple.Passwords"),
            "the bundle id matches verbatim, mixed case included"
        );
        assert_eq!(
            matched_entry(&front("COM.APPLE.KEYCHAINACCESS", "Keychain Access")),
            Some("com.apple.keychainaccess"),
            "matching is ASCII case-insensitive, and the list row comes back verbatim"
        );
        assert_eq!(
            matched_entry(&front("com.acme.vault", "1password")),
            None,
            "a display name only matches if the list carries that very name"
        );
        assert_eq!(
            matched_entry(&FrontApp {
                bundle_id: None,
                name: None,
            }),
            None,
            "an app with no identity cannot match"
        );
    }

    #[test]
    fn token_prefixes_are_detected() {
        let body = filler(24);
        for text in [
            format!("sk-{body}"),
            format!("ghp_{body}"),
            format!("xoxb-{body}"),
            format!("AKIA{}", "A1".repeat(8)),
            format!("Bearer {body}"),
            format!("\"sk-{body}\""),
            format!("GITHUB_TOKEN=ghp_{body}"),
        ] {
            assert_eq!(
                detect_sensitive(&text),
                Some(SensitiveKind::Token),
                "{text} must be reported as a token"
            );
        }
    }

    #[test]
    fn tokens_embedded_in_structured_text_are_detected() {
        let long = filler(24);
        let mut cases = vec![
            r#"{"token":"sk-1234567890abcd"}"#.to_owned(),
            r#"{"key":"ghp_1234567890abcd","env":"prod"}"#.to_owned(),
            "https://example.test/v1?token=sk-1234567890abcd&model=x".to_owned(),
            "https://platform.example.com/keys/sk-1234567890abcd".to_owned(),
            "OPENAI_API_KEY=sk-1234567890abcd".to_owned(),
            "export GITHUB_TOKEN=ghp_1234567890abcd".to_owned(),
            "```\nsk-1234567890abcd\n```".to_owned(),
        ];
        cases.push(format!(r#"{{"token":"sk-{long}"}}"#));
        for text in cases {
            assert_eq!(
                detect_sensitive(&text),
                Some(SensitiveKind::Token),
                "{text} hides a token that must still be reported"
            );
        }
    }

    #[test]
    fn short_dummy_keys_are_detected() {
        for text in [
            "sk-123456",
            "sk-abc123",
            "sk-XXXXXXXX",
            "sk-AbCdEf12",
            "sk_live_1234abcd",
            "sk_test_1234abcd",
            "ghp_1234abcd",
            "sk-1234567890abcd",
            "sk-1234567890abcd\nefghijklmnopqrst",
        ] {
            let kind = detect_sensitive(text);
            assert_eq!(
                kind,
                Some(SensitiveKind::Token),
                "{text} is a hand-written dummy key and must be reported, got {kind:?}"
            );
        }
    }

    #[test]
    fn invisible_characters_do_not_hide_a_token() {
        for text in [
            "sk-\u{200b}1234567890abcd",
            "sk-1234567890\u{00ad}abcd",
            "\u{feff}sk-123456",
        ] {
            assert_eq!(
                detect_sensitive(text),
                Some(SensitiveKind::Token),
                "{text} carries an invisible character inside the token"
            );
        }
    }

    #[test]
    fn prose_about_tokens_is_not_a_hit() {
        for text in [
            "密钥前缀 sk- 与 AKIA 是最常见的两种形态",
            "文档里提到 sk-abc 这种过短的写法",
            "把占位符写成 Bearer <token> 这样的形式",
            "Prefix sk- is fine as long as nothing follows it",
        ] {
            assert_eq!(
                detect_sensitive(text),
                None,
                "{text} is prose, not a secret"
            );
        }
    }

    #[test]
    fn hyphenated_words_are_not_mistaken_for_prefixed_tokens() {
        for text in [
            "disk-space-2024",
            "risk-managed-portfolio",
            "task-completed-2024",
            "sk-learn-scikit",
            "sk-abcdefgh",
            "mask-the-answer",
        ] {
            assert_eq!(
                detect_sensitive(text),
                None,
                "{text} is an ordinary hyphenated word, not a credential"
            );
        }
    }

    #[test]
    fn private_key_blocks_are_detected_before_tokens() {
        let pem = format!(
            "-----BEGIN {} PRIVATE KEY-----\n{}\n-----END {} PRIVATE KEY-----",
            "RSA",
            filler(24),
            "RSA"
        );
        assert_eq!(detect_sensitive(&pem), Some(SensitiveKind::PrivateKey));
        assert_eq!(
            detect_sensitive(&format!("sk-{}\n{pem}", filler(24))),
            Some(SensitiveKind::PrivateKey),
            "when both match, the more certain kind wins"
        );
    }

    #[test]
    fn card_numbers_pass_luhn_only() {
        assert_eq!(
            detect_sensitive("卡号 4111 1111 1111 1111"),
            Some(SensitiveKind::CardNumber)
        );
        assert_eq!(
            detect_sensitive("卡号 4111 1111 1111 1112"),
            None,
            "one digit off fails the checksum"
        );
        assert_eq!(
            detect_sensitive("0000 0000 0000 0000"),
            None,
            "an all-zero run passes the checksum but repeats a single digit"
        );
        assert_eq!(
            detect_sensitive("订单号 4111111111111111"),
            Some(SensitiveKind::CardNumber),
            "an unseparated run works too"
        );
    }

    #[test]
    fn high_entropy_strings_are_detected() {
        assert_eq!(
            detect_sensitive(&entropy_filler(52)),
            Some(SensitiveKind::HighEntropy)
        );
        assert_eq!(
            detect_sensitive(&format!("key: {}", entropy_filler(40))),
            Some(SensitiveKind::HighEntropy),
            "being surrounded by prose must not hide it"
        );
    }

    #[test]
    fn identifiers_and_prose_stay_below_the_entropy_threshold() {
        for text in [
            "a_very_long_snake_case_identifier_that_nobody_types_by_hand",
            "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz",
            "这是一段足够长的中文说明文字，用来确认非 ASCII 的长句不会被当成随机串。",
            "SCREAMING_SNAKE_CASE_CONSTANT_WITHOUT_ANY_DIGITS_AT_ALL",
        ] {
            assert_eq!(detect_sensitive(text), None, "{text} is not a credential");
        }
    }

    #[test]
    fn detection_prefers_the_more_certain_kind() {
        assert_eq!(
            detect_sensitive(&format!("4111 1111 1111 1111\n{}", entropy_filler(40))),
            Some(SensitiveKind::CardNumber),
            "a card number outranks an entropy hit"
        );
        assert_eq!(
            detect_sensitive(&format!("-----BEGIN {} PRIVATE KEY-----", "EC")),
            Some(SensitiveKind::PrivateKey),
            "a key block outranks every other heuristic"
        );
    }

    #[test]
    fn ordinary_text_is_never_flagged() {
        for text in [
            "今天下午三点开会，记得带上笔记本。",
            "The quick brown fox jumps over the lazy dog.",
            "fn main() { println!(\"hello\"); }",
            "https://example.com/docs/getting-started",
            "let model = resolved_model(TaskKind::TranslateWord);",
            "The disk-space-2024 report is ready.",
            "let key = \"sk-test\";",
            "这段文字里出现了 sk- 这个前缀。",
        ] {
            assert_eq!(detect_sensitive(text), None, "{text} is ordinary text");
        }
    }
}
