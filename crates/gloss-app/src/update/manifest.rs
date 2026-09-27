//! 更新清单：模型、schema 校验矩阵、版本比较与按架构选包。
//!
//! 校验矩阵（schema=1，见分发设计 §3.1）：`version` 必须可解析为 semver；
//! `channels.stable` 必须含双架构条目；每条目必含 `url`/`size`/`sha256`
//! （sha256 为 64 位十六进制小写，`url` 仅接受 https——信任根是 GitHub
//! Pages 的 TLS）。缺字段或类型不符、未知 `schema`、不可解析的 body 都按
//! 清单失败处理；未知字段一律忽略（同 schema 内的前向兼容）。

use std::collections::BTreeMap;

use semver::Version;
use serde::Deserialize;

/// 双架构条目的下载事实：zip 为客户端校验对象（size/sha256 都对它算），
/// `dmg_url` 供站点使用、客户端不读（按未知字段忽略）。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Artifact {
    /// 整包 zip 的直链。
    pub url: String,
    /// zip 字节数：响应体长度上限与 size 校验的依据。
    pub size: u64,
    /// zip 的 sha256（64 位十六进制小写）。
    pub sha256: String,
}

/// 校验通过的更新清单：只保留客户端要用的字段，架构条目展平存表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateManifest {
    /// 清单版本（已解析为 semver；相等或低于本地即无更新）。
    version: Version,
    /// 发布时间（ISO 8601，展示用，客户端不校验格式）。
    published_at: Option<String>,
    /// 版本说明页（GitHub Release 页，设置页可作链接出口）。
    notes_url: Option<String>,
    /// stable 频道的架构条目，键为 target 三元组。
    artifacts: BTreeMap<String, Artifact>,
}

impl UpdateManifest {
    /// 清单版本。
    pub fn version(&self) -> &Version {
        &self.version
    }

    /// 发布时间（未携带时为 `None`）。
    pub fn published_at(&self) -> Option<&str> {
        self.published_at.as_deref()
    }

    /// 版本说明页（未携带时为 `None`）。
    pub fn notes_url(&self) -> Option<&str> {
        self.notes_url.as_deref()
    }

    /// 按架构三元组取条目（矩阵保证双架构齐备，`None` 属防御分支）。
    pub fn artifact(&self, arch: &str) -> Option<&Artifact> {
        self.artifacts.get(arch)
    }
}

/// 清单失败的三类形态：后果一致（设置页显示「清单不可用」），分形变体
/// 只为日志能各说各话。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    /// body 不是合法 JSON，或字段缺失/类型不符（serde 拒收）。
    Unparsable,
    /// `schema` 值未知：fail-closed，拒绝解析。
    UnknownSchema(u32),
    /// 通过 JSON 解析但校验矩阵不符（版本不可解析、架构条目缺、
    /// url/sha256 形态不符）。
    Invalid,
}

/// serde 视图：只声明矩阵要核对的字段，其余（含 `dmg_url`）一律忽略。
#[derive(Debug, Deserialize)]
struct ManifestDoc {
    schema: Option<u32>,
    version: Option<String>,
    published_at: Option<String>,
    notes_url: Option<String>,
    channels: Option<ChannelsDoc>,
}

#[derive(Debug, Deserialize)]
struct ChannelsDoc {
    stable: Option<BTreeMap<String, ArtifactDoc>>,
}

#[derive(Debug, Deserialize)]
struct ArtifactDoc {
    url: Option<String>,
    size: Option<u64>,
    sha256: Option<String>,
}

/// 本地当前版本（编译期常量）：Cargo 在构建期拒绝非法 semver 的
/// `version` 字段，回落分支不可达（有单测锁定）。
pub fn current_version() -> Version {
    match Version::parse(env!("CARGO_PKG_VERSION")) {
        Ok(version) => version,
        Err(_) => Version::new(0, 0, 0),
    }
}

/// 当前进程的架构键：应用只发布这两个 target，编译期即可判定本机归属。
pub fn current_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        ARM_DARWIN
    } else {
        X64_DARWIN
    }
}

const ARM_DARWIN: &str = "aarch64-apple-darwin";
const X64_DARWIN: &str = "x86_64-apple-darwin";

/// 版本比较（semver crate 的全序）：清单版本高于本地才视为有更新，相等
/// 或更低一律无更新。全序把构建元数据作并列时的最终裁决——比规范的
/// 优先级序（忽略构建元数据）更细，只为满足全序的可比性。
pub fn is_newer(candidate: &Version, current: &Version) -> bool {
    candidate > current
}

/// 解析并校验清单 body：先 fail-closed 核 `schema`，再走校验矩阵。
pub fn parse(body: &str) -> Result<UpdateManifest, ManifestError> {
    let doc: ManifestDoc = serde_json::from_str(body).map_err(|_| ManifestError::Unparsable)?;
    match doc.schema {
        Some(1) => {}
        Some(other) => return Err(ManifestError::UnknownSchema(other)),
        None => return Err(ManifestError::Invalid),
    }
    let version = doc
        .version
        .as_deref()
        .and_then(|v| Version::parse(v).ok())
        .ok_or(ManifestError::Invalid)?;
    let stable = doc
        .channels
        .and_then(|c| c.stable)
        .ok_or(ManifestError::Invalid)?;
    let mut artifacts = BTreeMap::new();
    for (arch, entry) in stable {
        let url = entry.url.ok_or(ManifestError::Invalid)?;
        let size = entry.size.ok_or(ManifestError::Invalid)?;
        let sha256 = entry.sha256.ok_or(ManifestError::Invalid)?;
        if !url.starts_with("https://") || !is_sha256(&sha256) {
            return Err(ManifestError::Invalid);
        }
        artifacts.insert(arch, Artifact { url, size, sha256 });
    }
    for arch in [ARM_DARWIN, X64_DARWIN] {
        if !artifacts.contains_key(arch) {
            return Err(ManifestError::Invalid);
        }
    }
    Ok(UpdateManifest {
        version,
        published_at: doc.published_at,
        notes_url: doc.notes_url,
        artifacts,
    })
}

/// 64 位十六进制小写（清单样例承诺的形态；混入大写或非 hex 按不符处理）。
fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn body(schema: &str, version: &str, arm: &str, x64: &str) -> String {
        format!(
            r#"{{"schema":{schema},"version":"{version}","channels":{{"stable":{{"aarch64-apple-darwin":{{{arm}}},"x86_64-apple-darwin":{{{x64}}}}}}}}}"#
        )
    }

    fn artifact(url: &str, size: u64, sha256: &str) -> String {
        format!(r#""url":"{url}","size":{size},"sha256":"{sha256}""#)
    }

    fn valid_artifact() -> String {
        artifact("https://pages.example/arm.zip", 15_230_000, SHA_A)
    }

    fn valid_body(version: &str) -> String {
        body(
            "1",
            version,
            &valid_artifact(),
            &artifact("https://pages.example/x64.zip", 16_810_000, SHA_B),
        )
    }

    fn parsed(body_text: &str) -> UpdateManifest {
        parse(body_text).expect("test body must validate")
    }

    #[test]
    fn valid_manifest_parses_with_all_fields() {
        let text = r#"{
  "schema": 1,
  "version": "0.2.0",
  "published_at": "2026-09-23T12:00:00Z",
  "notes_url": "https://github.com/Losmli010/gloss/releases/tag/v0.2.0",
  "channels": { "stable": {
    "aarch64-apple-darwin": { "url": "https://p/a.zip", "size": 1, "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" },
    "x86_64-apple-darwin": { "url": "https://p/x.zip", "size": 2, "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" }
  } }
}"#;
        let manifest = parsed(text);
        assert_eq!(manifest.version().to_string(), "0.2.0");
        assert_eq!(manifest.published_at(), Some("2026-09-23T12:00:00Z"));
        assert_eq!(
            manifest.notes_url(),
            Some("https://github.com/Losmli010/gloss/releases/tag/v0.2.0")
        );
        let (url, size, sha) = if current_arch() == ARM_DARWIN {
            ("https://p/a.zip", 1, SHA_A)
        } else {
            ("https://p/x.zip", 2, SHA_B)
        };
        let entry = manifest.artifact(current_arch()).expect("host entry");
        assert_eq!(entry.url, url);
        assert_eq!(entry.size, size);
        assert_eq!(entry.sha256, sha);
    }

    #[test]
    fn unknown_fields_are_ignored_for_forward_compatibility() {
        let text = r#"{
  "schema": 1,
  "version": "0.2.0",
  "future_top_level": { "anything": true },
  "channels": { "stable": {
    "aarch64-apple-darwin": { "url": "https://p/a.zip", "size": 1, "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "dmg_url": "https://p/a.dmg", "future": null },
    "x86_64-apple-darwin": { "url": "https://p/x.zip", "size": 2, "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" }
  } }
}"#;
        let manifest = parse(text).expect("unknown fields must be ignored");
        assert_eq!(manifest.version().to_string(), "0.2.0");
        assert!(manifest.artifact(current_arch()).is_some());
    }

    #[test]
    fn optional_fields_may_be_absent() {
        let manifest = parsed(&valid_body("0.2.0"));
        assert_eq!(manifest.published_at(), None);
        assert_eq!(manifest.notes_url(), None);
    }

    #[test]
    fn unknown_schema_is_fail_closed() {
        for schema in ["2", "0", "99"] {
            let text = body(
                schema,
                "0.2.0",
                &valid_artifact(),
                &artifact("https://p/x.zip", 2, SHA_B),
            );
            assert!(
                matches!(parse(&text), Err(ManifestError::UnknownSchema(n)) if n.to_string() == schema),
                "schema {schema} must be rejected without parsing further"
            );
        }
    }

    #[test]
    fn unparsable_body_is_rejected() {
        for text in ["", "not json", "[]", r#"{"schema":"1"}"#] {
            assert_eq!(parse(text), Err(ManifestError::Unparsable), "text: {text}");
        }
    }

    #[test]
    fn version_must_be_a_semver_triple() {
        for version in ["", "0.2", "v0.2.0", "0.2.x"] {
            let text = body(
                "1",
                version,
                &valid_artifact(),
                &artifact("https://p/x.zip", 2, SHA_B),
            );
            assert_eq!(
                parse(&text),
                Err(ManifestError::Invalid),
                "version: {version}"
            );
        }
    }

    #[test]
    fn both_architectures_are_required_by_the_matrix() {
        let text = format!(
            r#"{{"schema":1,"version":"0.2.0","channels":{{"stable":{{"aarch64-apple-darwin":{{{}}}}}}}}}"#,
            valid_artifact()
        );
        assert_eq!(parse(&text), Err(ManifestError::Invalid));

        let no_channels = r#"{"schema":1,"version":"0.2.0"}"#;
        assert_eq!(parse(no_channels), Err(ManifestError::Invalid));

        let empty_stable = r#"{"schema":1,"version":"0.2.0","channels":{"stable":{}}}"#;
        assert_eq!(parse(empty_stable), Err(ManifestError::Invalid));
    }

    #[test]
    fn artifact_fields_follow_the_matrix() {
        let missing_url = body("1", "0.2.0", r##""size":1,"sha256":"""##, r#""size":2"#);
        assert_eq!(parse(&missing_url), Err(ManifestError::Invalid));

        let missing_size = body(
            "1",
            "0.2.0",
            &format!(r##""url":"https://p/a.zip","sha256":"{SHA_A}""##),
            r#""url":"https://p/x.zip""#,
        );
        assert_eq!(parse(&missing_size), Err(ManifestError::Invalid));

        let missing_sha = body(
            "1",
            "0.2.0",
            r#""url":"https://p/a.zip","size":1"#,
            r#""url":"https://p/x.zip""#,
        );
        assert_eq!(parse(&missing_sha), Err(ManifestError::Invalid));

        let wrong_type = body(
            "1",
            "0.2.0",
            &artifact("https://p/a.zip", 15_230_000, SHA_A),
            r##""url":"https://p/x.zip","size":"many","sha256":"""##,
        );
        assert_eq!(parse(&wrong_type), Err(ManifestError::Unparsable));
    }

    #[test]
    fn artifact_url_must_be_https() {
        let text = body(
            "1",
            "0.2.0",
            &artifact("http://pages.example/arm.zip", 1, SHA_A),
            &artifact("https://p/x.zip", 2, SHA_B),
        );
        assert_eq!(parse(&text), Err(ManifestError::Invalid));
    }

    #[test]
    fn sha256_must_be_64_lowercase_hex() {
        for sha in [
            "aaa",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
        ] {
            let text = body(
                "1",
                "0.2.0",
                &artifact("https://p/a.zip", 1, sha),
                &artifact("https://p/x.zip", 2, SHA_B),
            );
            assert_eq!(parse(&text), Err(ManifestError::Invalid), "sha: {sha}");
        }
    }

    #[test]
    fn is_newer_follows_semver_precedence() {
        let current = Version::new(0, 1, 0);
        assert!(is_newer(
            &Version::parse("0.2.0").expect("literal semver"),
            &current
        ));
        assert!(is_newer(
            &Version::parse("0.1.1").expect("literal semver"),
            &current
        ));
        assert!(!is_newer(&current, &current), "equal is not newer");
        assert!(
            !is_newer(&Version::parse("0.0.9").expect("literal semver"), &current),
            "lower is not newer"
        );
        assert!(
            is_newer(
                &Version::parse("0.1.1-rc.1").expect("literal semver"),
                &current
            ),
            "prerelease of a higher triple outranks the lower triple"
        );
        assert!(
            !is_newer(
                &Version::parse("0.1.0-rc.1").expect("literal semver"),
                &current
            ),
            "a prerelease of the same triple ranks below the release"
        );
        assert!(
            is_newer(
                &Version::parse("0.1.0+build.2").expect("literal semver"),
                &current
            ),
            "the semver crate's total order breaks precedence ties on build metadata"
        );
    }

    #[test]
    fn arch_keys_cover_the_published_pair() {
        let mut keys = [
            current_arch(),
            if current_arch() == ARM_DARWIN {
                X64_DARWIN
            } else {
                ARM_DARWIN
            },
        ];
        keys.sort_unstable();
        assert_eq!(keys, [ARM_DARWIN, X64_DARWIN]);
    }

    #[test]
    fn current_version_matches_the_cargo_package_version() {
        let parsed = Version::parse(env!("CARGO_PKG_VERSION"));
        assert_eq!(
            current_version(),
            parsed.expect("cargo package version is valid semver"),
            "the fallback branch of current_version must stay unreachable"
        );
    }
}
