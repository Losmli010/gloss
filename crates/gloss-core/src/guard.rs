//! 敏感信息防护：触发前的场景闸门与发送前的内容启发式。
//!
//! 两条闸门都是“纯逻辑”：事实（安全输入态、前台应用标识、选区文本）由
//! 调用方从平台取来，本模块只判定——因此能在第一层以任意组合驱动，
//! 不需要真机，也不需要网络。
//!
//! 两者都是“无条件拦截”：命中即这次请求不成（场景闸门让触发不发生，
//! 内容闸门让取材产物止步于状态机），没有开关、没有名单编辑、也没有
//! “仍然继续”这一路出口——防护不交由用户控制。判据只有类别出去：命中的
//! 原文既不进日志也不进错误消息，只留敏感类别与拦截记录。
//!
//! 内容闸门只认“形状”：已知前缀加像凭据的主体、密钥块（PEM）、过卡号
//! 校验（Luhn）的卡号、高熵串。形状判定必须容得下“用户实际选中的样子”
//! ——引号、括号、对象记法（JSON）、查询串里嵌着的令牌、汉字紧贴的令牌、
//! 手写的短假密钥、复制时混进零宽字符的令牌、卡号后面跟着有效期或
//! 安全码（CVC），都算命中；谈论前缀的散文不算。前缀令牌的形状下限分两档
//! （见前缀形状判定）：长档只看长度，短档还要求含数字或大写——普通小写
//! 复合词被挡在短档之外，够长的那些算中（写明的取舍）。

use std::fmt;

const SENSITIVE_APPS: [&str; 7] = [
    "com.1password.1password",
    "com.agilebits.onepassword7",
    "com.bitwarden.desktop",
    "com.lastpass.LastPass",
    "com.dashlane.dashlane",
    "com.apple.keychainaccess",
    "com.apple.Passwords",
];

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FrontApp {
    pub bundle_id: Option<String>,
    pub name: Option<String>,
    pub is_self: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SceneFacts {
    pub secure_input: bool,
    pub front_app: Option<FrontApp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerBlock {
    SecureInput,
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

pub fn trigger_block(facts: &SceneFacts) -> Option<TriggerBlock> {
    if facts.secure_input {
        return Some(TriggerBlock::SecureInput);
    }
    let front = facts.front_app.as_ref()?;
    matched_entry(front).map(TriggerBlock::BlockedApp)
}

fn matched_entry(front: &FrontApp) -> Option<&'static str> {
    let identities = [front.bundle_id.as_deref(), front.name.as_deref()];
    SENSITIVE_APPS.iter().copied().find(|entry| {
        identities
            .iter()
            .flatten()
            .any(|identity| identity.eq_ignore_ascii_case(entry))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensitiveKind {
    Token,
    PrivateKey,
    HighEntropy,
    CardNumber,
}

pub fn detect_sensitive(text: &str) -> Option<SensitiveKind> {
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

const TRIMMED: &[char] = &[
    '"', '\'', '`', '(', ')', '[', ']', '{', '}', '<', '>', ',', ';', ':', '!', '?', '*',
    '\u{3002}', '\u{ff0c}', '\u{3001}',
];

const INVISIBLE: [char; 6] = [
    '\u{200b}', '\u{200c}', '\u{200d}', '\u{2060}', '\u{feff}', '\u{00ad}',
];

const TOKEN_PREFIXES: [&str; 10] = [
    "sk-", "sk_live_", "sk_test_", "ghp_", "gho_", "ghs_", "ghu_", "ghr_", "xoxb-", "xoxp-",
];

const MIN_TOKEN_BODY: usize = 16;
const SHORT_TOKEN_BODY: usize = 4;
const MIN_BEARER_BODY: usize = 20;
const AWS_KEY_ID_BODY: usize = 16;
const ENTROPY_MIN_LEN: usize = 32;
const ENTROPY_MIN_BITS: f64 = 4.5;
const MIN_CARD_DIGITS: usize = 13;
const MAX_CARD_DIGITS: usize = 19;

fn has_private_key_block(text: &str) -> bool {
    text.contains("-----BEGIN") && text.contains("PRIVATE KEY")
}
fn has_token(text: &str) -> bool {
    let mut previous: Option<&str> = None;
    for raw in text.split_whitespace() {
        let word = raw.trim_matches(TRIMMED);
        if is_prefixed_token(word) {
            return true;
        }
        if is_bearer_token(previous, word) {
            return true;
        }
        previous = Some(word);
    }
    false
}

fn is_bearer_token(previous: Option<&str>, word: &str) -> bool {
    previous.is_some_and(|prev| after_assignment(prev).eq_ignore_ascii_case("bearer"))
        && is_token_body(word, MIN_BEARER_BODY)
}

fn after_assignment(word: &str) -> &str {
    word.rsplit_once('=').map_or(word, |(_, value)| value)
}

fn is_prefixed_token(candidate: &str) -> bool {
    for (at, _) in candidate.char_indices() {
        let at_token_start = at == 0
            || !candidate[..at]
                .chars()
                .next_back()
                .is_some_and(|prev| prev.is_ascii_alphanumeric());
        if !at_token_start {
            continue;
        }
        let rest = &candidate[at..];
        if let Some(body) = rest.strip_prefix("AKIA")
            && aws_body_run(body).len() >= AWS_KEY_ID_BODY
        {
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

fn body_run(rest: &str) -> &str {
    let end = rest
        .char_indices()
        .find(|&(_, ch)| !is_prefixed_body_char(ch))
        .map_or(rest.len(), |(at, _)| at);
    &rest[..end]
}

fn is_prefixed_body_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-' | '+' | '/' | '=')
}

fn aws_body_run(rest: &str) -> &str {
    let end = rest
        .char_indices()
        .find(|&(_, ch)| !(ch.is_ascii_uppercase() || ch.is_ascii_digit()))
        .map_or(rest.len(), |(at, _)| at);
    &rest[..end]
}

fn is_token_body(body: &str, min_len: usize) -> bool {
    body.len() >= min_len
        && body
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

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

fn has_all_three_classes(word: &str) -> bool {
    let lower = word.bytes().any(|byte| byte.is_ascii_lowercase());
    let upper = word.bytes().any(|byte| byte.is_ascii_uppercase());
    let digit = word.bytes().any(|byte| byte.is_ascii_digit());
    lower && upper && digit
}

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

fn has_card_number(text: &str) -> bool {
    let mut digits = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            if digits.len() >= MAX_CARD_DIGITS {
                digits.clear();
            }
            digits.push(ch);
        } else if matches!(ch, ' ' | '-') && !digits.is_empty() {
            if luhn_ok(&digits) {
                return true;
            }
        } else {
            if luhn_ok(&digits) {
                return true;
            }
            digits.clear();
        }
    }
    luhn_ok(&digits)
}

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
            is_self: false,
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
            trigger_block(&SceneFacts {
                secure_input: true,
                front_app: Some(front("com.1password.1password", "1Password")),
            }),
            Some(TriggerBlock::SecureInput),
            "when both facts hold, secure input is the reason that comes back"
        );
        assert_eq!(
            trigger_block(&SceneFacts::default()),
            None,
            "no facts and no reason to block"
        );
    }

    #[test]
    fn entry_matching_covers_every_identity_an_app_can_offer() {
        for (facts, expected, message) in [
            (
                in_app("com.acme.vault", "Acme Vault"),
                None,
                "an app outside the built-in list matches nothing",
            ),
            (
                in_app("com.apple.Passwords", "Passwords"),
                Some(TriggerBlock::BlockedApp("com.apple.Passwords")),
                "the bundle id matches verbatim, mixed case included",
            ),
            (
                in_app("COM.APPLE.KEYCHAINACCESS", "Keychain Access"),
                Some(TriggerBlock::BlockedApp("com.apple.keychainaccess")),
                "matching is ASCII case-insensitive, and the list row comes back verbatim",
            ),
            (
                in_app("com.acme.vault", "1password"),
                None,
                "a display name only matches if the list carries that very name",
            ),
            (
                SceneFacts {
                    secure_input: false,
                    front_app: Some(FrontApp {
                        bundle_id: None,
                        name: None,
                        is_self: false,
                    }),
                },
                None,
                "an app with no identity cannot match",
            ),
        ] {
            assert_eq!(trigger_block(&facts), expected, "{message}");
        }
    }

    #[test]
    fn the_self_flag_is_not_consumed_by_the_sensitive_list() {
        let mut facts = in_app("com.example.editor", "Editor");
        facts
            .front_app
            .as_mut()
            .expect("front helper sets an app")
            .is_self = true;
        assert_eq!(
            trigger_block(&facts),
            None,
            "is_self routes anti-mistouch in the trigger decision, it is not a sensitive-app hit"
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
            format!("AUTH=Bearer {body}"),
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
            "密钥是sk-1234567890abcd".to_owned(),
            "密钥：sk-1234567890abcd".to_owned(),
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
            "sk-\u{00ad}1234567890abcd",
            "sk-\u{2060}123456",
            "sk-\u{200c}1234567890abcd",
            "sk-\u{200d}1234567890abcd",
            "sk-\u{feff}123456",
        ] {
            assert_eq!(
                detect_sensitive(text),
                Some(SensitiveKind::Token),
                "{text} carries an invisible character inside the token"
            );
        }
        assert_eq!(
            detect_sensitive("卡号 4111\u{200b}1111 1111 1111"),
            Some(SensitiveKind::CardNumber),
            "the stripping happens before every detector, not just the token one"
        );
        assert_eq!(
            detect_sensitive(&format!("key \u{200b}{}", entropy_filler(40))),
            Some(SensitiveKind::HighEntropy),
            "an invisible character inside a random string is stripped too"
        );
    }

    #[test]
    fn bearer_and_aws_key_ids_keep_their_own_bounds() {
        let long = filler(24);
        assert_eq!(
            detect_sensitive(&format!("AKIA{}=", "A1".repeat(8))),
            Some(SensitiveKind::Token),
            "base64 padding after an access key id is not part of the id"
        );
        assert_eq!(
            detect_sensitive(&format!("Bearer {long}+/==")),
            None,
            "the Bearer branch keeps the narrow charset: a standard-base64 body is out of its shape"
        );
        for text in [
            "AKIA2024",
            &format!("AKIA{}{}", "A1".repeat(5), "A1"),
            &format!("akia{}", "A1".repeat(8)),
            &format!("AKIA{}-{}", "ABCDEFGH", "IJKLMNOP"),
        ] {
            assert_eq!(
                detect_sensitive(text),
                None,
                "{text} is too short, uses a lower-case prefix, or carries a character outside the id charset"
            );
        }
    }

    #[test]
    fn the_long_tier_of_prefixed_bodies_ignores_shape() {
        for text in ["sk-learn-scikit", "sk-abcdefgh"] {
            assert_eq!(
                detect_sensitive(text),
                None,
                "{text} is below the short tier's floor and looks like an ordinary word"
            );
        }
        for text in ["sk-learn-scikit-learn", "sk-abcdefghijklmnop"] {
            assert_eq!(
                detect_sensitive(text),
                Some(SensitiveKind::Token),
                "{text} is long enough to be taken for a credential: the long tier only checks length, which is the written-off trade"
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
        for text in [
            "4111 1111 1111 1111 12/26",
            "4111 1111 1111 1111 123",
            "4111  1111  1111  1111",
        ] {
            assert_eq!(
                detect_sensitive(text),
                Some(SensitiveKind::CardNumber),
                "{text} carries a card number next to other digits"
            );
        }
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
        let near: String = "Aa1Bb2Cc3Dd4Ee5Ff6Gg7"
            .chars()
            .flat_map(|ch| [ch, ch])
            .collect();
        for text in ["Aa1".repeat(12), "aB3.".repeat(10), near] {
            assert_eq!(
                detect_sensitive(&text),
                None,
                "{text} has all three character classes and the length, so only the entropy threshold can reject it"
            );
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
            detect_sensitive(&format!("sk-{}\n4111 1111 1111 1111", filler(24))),
            Some(SensitiveKind::Token),
            "a token outranks a card number"
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
