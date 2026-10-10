//! 任务产物缓存：moka LRU + TTL，全部缓存逻辑的唯一落点（core 零缓存）。
//!
//! key 由 [`cache_key`] 统一派生：`cache_key(input, options)`——输入与
//! 选项共同决定产物，kind 不参与（分类在 LLM 层内完成，同一输入在
//! 同一选项下判定的 kind 唯一，无需单独去重）。模型 id 在 `options`
//! 里——同文本换模型不得命中旧产物。摘要用 std `DefaultHasher`
//! （SipHash）：只要求进程内稳定，不要求跨版本持久稳定。输入按变体
//! 直接喂哈希器（图像字节流式写入，不经 serde 展开临时缓冲）；选项
//! 仍走 serde 全量参与。
//!
//! 缓存恒存**完成产物**（[`TaskOutcome`]）：只有成功解析的整卡才写入，
//! 半截流与失败不入缓存。TTL 来自配置（`Config::cache_ttl_secs`，组装点
//! 换算成 `Duration` 注入 [`TaskCache::with_ttl`]）。

use std::hash::{Hash, Hasher};
use std::time::Duration;

use gloss_core::task::{TaskInput, TaskOptions, TaskOutcome};

/// 缓存条目存活时长：任务产物在会话内重复触发收益明显，超过后过期腾位置。
/// `pub(crate)` 供 gloss-core `Config` 的出厂默认用例对齐——`Config::cache_ttl_secs`
/// 的注释声称与此一致，两侧都写字面量时改一边不会有人红。
pub(crate) const DEFAULT_TTL: Duration = Duration::from_secs(60 * 60);

/// 容量上限（条目数）：词条卡/翻译卡体积小，256 条足够覆盖高频场景，
/// 超出由 moka 按 LRU（TinyLFU）逐出。
const MAX_ENTRIES: u64 = 256;

/// 派生缓存 key：输入与选项全量参与（含模型 id 与 prompt 模板语言）。
/// 输入按变体直接喂哈希器——Text 哈希字符串；Image 流式哈希 png 字节与
/// region（`None` 也参与），字节不展开成临时缓冲；Audio 哈希字节与时长的
/// 位形态（`f32` 非 `Hash`，bits 表达对同值稳定）。选项走 serde 全量
/// 参与；其字段均为可直接序列化的标量，序列化失败分支只为 `Result` 完整
/// 性保留（`Debug` 文本哈希对同值稳定）。
pub fn cache_key(input: &TaskInput, options: &TaskOptions) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    hash_input(input, &mut hasher);
    match serde_json::to_vec(options) {
        Ok(bytes) => bytes.hash(&mut hasher),
        Err(_) => format!("{options:?}").hash(&mut hasher),
    }
    hasher.finish()
}

/// 输入的变体化哈希：模态以 `discriminant` 区分，载荷逐字段喂入。
fn hash_input(input: &TaskInput, hasher: &mut std::hash::DefaultHasher) {
    std::mem::discriminant(input).hash(hasher);
    match input {
        TaskInput::Text { text } => text.hash(hasher),
        TaskInput::Image { png, region } => {
            png.as_ref().hash(hasher);
            region.hash(hasher);
        }
        TaskInput::Audio {
            bytes,
            duration_hint,
        } => {
            bytes.as_ref().hash(hasher);
            duration_hint.map(f32::to_bits).hash(hasher);
        }
    }
}

/// 任务产物缓存：线程安全的 moka 内存实现，`get`/`set` 可从任意线程
/// 调用（tokio 侧组装、主线程无需访问）。
#[derive(Debug)]
pub struct TaskCache {
    inner: moka::sync::Cache<u64, TaskOutcome>,
}

impl TaskCache {
    /// 默认缓存：1 小时 TTL、256 条上限。
    pub fn new() -> Self {
        Self::with_ttl(DEFAULT_TTL)
    }

    /// 自定义 TTL（生产由配置换算注入；测试用短 TTL 验证过期路径）。
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            inner: moka::sync::Cache::builder()
                .time_to_live(ttl)
                .max_capacity(MAX_ENTRIES)
                .build(),
        }
    }

    /// 取缓存产物。
    pub fn get(&self, key: u64) -> Option<TaskOutcome> {
        self.inner.get(&key)
    }

    /// 写缓存产物。
    pub fn set(&self, key: u64, value: TaskOutcome) {
        self.inner.insert(key, value);
    }

    /// 强制处理逐出/过期（生产代码无需调用；测试用它同步过期判定）。
    pub fn run_pending_tasks(&self) {
        self.inner.run_pending_tasks();
    }
}

impl Default for TaskCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::{DEFAULT_TTL, TaskCache, cache_key};
    use gloss_core::config::DEFAULT_MODEL;
    use gloss_core::model::Lang;
    use gloss_core::model::Locale;
    use gloss_core::model::ScreenRect;
    use gloss_core::task::{OutcomeStructured, TaskInput, TaskKind, TaskOptions, TaskOutcome};

    fn text_input(text: &str) -> TaskInput {
        TaskInput::Text { text: text.into() }
    }

    fn outcome(note: &str) -> TaskOutcome {
        TaskOutcome {
            kind: TaskKind::TranslateWord,
            note: note.into(),
            code_language: None,
            structured: OutcomeStructured::Plain {
                examples: Vec::new(),
            },
        }
    }

    #[test]
    fn different_inputs_get_different_keys() {
        assert_ne!(
            cache_key(&text_input("gloss"), &TaskOptions::default()),
            cache_key(&text_input("gloss2"), &TaskOptions::default()),
            "text"
        );
        assert_ne!(
            cache_key(
                &TaskInput::Audio {
                    bytes: Arc::from(&b"au"[..]),
                    duration_hint: Some(1.5),
                },
                &TaskOptions::default()
            ),
            cache_key(&text_input("au"), &TaskOptions::default()),
            "modality"
        );
    }

    #[test]
    fn different_options_get_different_keys() {
        let base = cache_key(&text_input("hello"), &TaskOptions::default());

        let ja = TaskOptions {
            target_lang: Some(Lang::Ja),
            ..TaskOptions::default()
        };
        assert_ne!(base, cache_key(&text_input("hello"), &ja), "target lang");

        let english = TaskOptions {
            prompt_locale: Some(Locale::En),
            ..TaskOptions::default()
        };
        assert_ne!(base, cache_key(&text_input("hello"), &english), "locale");

        let model = TaskOptions {
            model: "other-model".into(),
            ..TaskOptions::default()
        };
        assert_ne!(base, cache_key(&text_input("hello"), &model), "model id");
        assert_ne!(
            model.model, DEFAULT_MODEL,
            "the test must not alias the factory model"
        );
    }

    #[test]
    fn key_derivation_is_stable_and_non_finite_audio_hashes_by_bits() {
        let base = cache_key(&text_input("gloss"), &TaskOptions::default());
        assert_eq!(
            base,
            cache_key(&text_input("gloss"), &TaskOptions::default()),
            "same input and options must derive the same key"
        );

        let nan = |hint: Option<f32>| TaskInput::Audio {
            bytes: Arc::from(&b"au"[..]),
            duration_hint: hint,
        };
        let nan_key = cache_key(&nan(Some(f32::NAN)), &TaskOptions::default());
        assert_eq!(
            nan_key,
            cache_key(&nan(Some(f32::NAN)), &TaskOptions::default()),
            "non-finite durations hash deterministically by bit form"
        );
        assert_ne!(
            nan_key,
            cache_key(&nan(Some(1.5)), &TaskOptions::default()),
            "non-finite and finite durations must not alias"
        );
    }

    #[test]
    fn image_keys_follow_png_bytes_and_region() {
        let options = TaskOptions::default();
        let image = |bytes: &[u8], region: Option<ScreenRect>| TaskInput::Image {
            png: Arc::from(bytes),
            region,
        };
        let rect = ScreenRect {
            x: 1,
            y: 2,
            width: 3,
            height: 4,
        };
        let base = cache_key(&image(b"png-bytes", None), &options);
        assert_eq!(
            base,
            cache_key(&image(b"png-bytes", None), &options),
            "identical bytes must derive the same key"
        );
        assert_ne!(
            base,
            cache_key(&image(b"png-bytes-2", None), &options),
            "different bytes must not alias"
        );
        assert_ne!(
            base,
            cache_key(&image(b"png-bytes", Some(rect)), &options),
            "None and Some region must not alias"
        );
        assert_eq!(
            cache_key(&image(b"png-bytes", Some(rect)), &options),
            cache_key(&image(b"png-bytes", Some(rect)), &options),
            "identical bytes and region must derive the same key"
        );
        assert_ne!(
            cache_key(&text_input("png-bytes"), &options),
            base,
            "modality is hashed even when payloads coincide"
        );
    }

    #[test]
    fn entries_store_and_isolate_outcomes() {
        let cache = TaskCache::new();
        let hit = cache_key(&text_input("gloss"), &TaskOptions::default());
        let miss = cache_key(&text_input("gloss2"), &TaskOptions::default());
        cache.set(hit, outcome("词卡产物"));
        assert_eq!(cache.get(hit).map(|o| o.note), Some("词卡产物".into()));
        assert!(
            cache.get(miss).is_none(),
            "unrelated key must not see entry"
        );
    }

    #[test]
    fn ttl_expiry_takes_effect() {
        let cache = TaskCache::with_ttl(Duration::from_millis(60));
        let key = cache_key(&text_input("gloss"), &TaskOptions::default());
        cache.set(key, outcome("soon gone"));
        assert!(cache.get(key).is_some(), "fresh entry must be readable");

        std::thread::sleep(Duration::from_millis(120));
        cache.run_pending_tasks();
        assert!(cache.get(key).is_none(), "expired entry must be gone");
    }

    #[test]
    fn factory_ttl_matches_the_config_default() {
        assert_eq!(
            Duration::from_secs(gloss_core::config::Config::default().cache_ttl_secs),
            DEFAULT_TTL
        );
    }
}
