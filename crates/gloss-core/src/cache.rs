//! Cache 端口的内存实现：moka LRU + TTL。
//!
//! key 由 [`cache_key`] 统一派生（kind, input, options, model 的规范化序
//! 列化摘要）——同一文本在不同任务类型/模型下不共享缓存。
//! 进程内缓存即可满足 M3-MVP，摘要用 std `DefaultHasher`（SipHash）：
//! 只要求进程内稳定，不要求跨版本持久稳定。

use std::hash::{Hash, Hasher};
use std::time::Duration;

use crate::ports::Cache;
use crate::task::{Task, TaskOutcome};

/// 缓存条目存活时长：任务产物在会话内重复触发收益明显，超过后过期腾位置。
/// `pub(crate)` 供 `config` 的出厂默认用例对齐——`Config::cache_ttl_secs`
/// 的注释声称与此一致，两侧都写字面量时改一边不会有人红。
pub(crate) const DEFAULT_TTL: Duration = Duration::from_secs(60 * 60);

/// 容量上限（条目数）：词条卡/翻译卡体积小，256 条足够覆盖高频场景，
/// 超出由 moka 按 LRU（TinyLFU）逐出。
const MAX_ENTRIES: u64 = 256;

/// 派生缓存 key：模型 id 参与哈希——同任务换模型（配置切换）不得命中旧
/// 产物。序列化失败（非有限浮点等）退回 `Debug` 文本哈希，保证 key 恒
/// 可得且不同任务间碰撞概率不因回退路径上升。
pub fn cache_key(task: &Task, model: &str) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    match serde_json::to_vec(task) {
        Ok(bytes) => bytes.hash(&mut hasher),
        // 序列化失败路径：Debug 表示对同值任务稳定，仍是有效的规范化输入。
        Err(_) => format!("{task:?}").hash(&mut hasher),
    }
    model.hash(&mut hasher);
    hasher.finish()
}

/// [`Cache`] 端口的 moka 内存实现：线程安全，`get`/`set` 可从任意线程
/// 调用（tokio 侧与事件线程共用同一实例）。
///
#[derive(Debug)]
pub struct MokaCache {
    inner: moka::sync::Cache<u64, TaskOutcome>,
}

impl MokaCache {
    /// 默认缓存：1 小时 TTL、256 条上限。
    pub fn new() -> Self {
        Self::with_ttl(DEFAULT_TTL)
    }

    /// 自定义 TTL（测试用短 TTL 验证过期路径）。
    pub fn with_ttl(ttl: Duration) -> Self {
        let inner = moka::sync::Cache::builder()
            .time_to_live(ttl)
            .max_capacity(MAX_ENTRIES)
            .build();
        Self { inner }
    }

    /// 强制处理逐出/过期（生产代码无需调用；测试用它同步过期判定）。
    pub fn run_pending_tasks(&self) {
        self.inner.run_pending_tasks();
    }
}

impl Default for MokaCache {
    fn default() -> Self {
        Self::new()
    }
}

impl Cache for MokaCache {
    fn get(&self, key: u64) -> Option<TaskOutcome> {
        self.inner.get(&key)
    }

    fn set(&self, key: u64, value: TaskOutcome) {
        self.inner.insert(key, value);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{MokaCache, cache_key};
    use crate::model::Lang;
    use crate::ports::Cache;
    use crate::task::{
        InputHint, OutcomeStructured, Task, TaskInput, TaskKind, TaskOptions, TaskOutcome,
    };

    fn text_task(kind: TaskKind, text: &str) -> Task {
        Task {
            kind,
            input: TaskInput::Text {
                text: text.into(),
                hint: None,
            },
            options: TaskOptions::default(),
        }
    }

    fn outcome(body: &str) -> TaskOutcome {
        TaskOutcome {
            kind: TaskKind::TranslateWord,
            body: body.into(),
            structured: OutcomeStructured::Plain { title: None },
        }
    }

    #[test]
    fn same_text_different_kinds_do_not_share_cache() {
        let word = cache_key(&text_task(TaskKind::TranslateWord, "gloss"), "m1");
        let sentence = cache_key(&text_task(TaskKind::TranslateSentence, "gloss"), "m1");
        assert_ne!(word, sentence, "cache key must be kind-scoped");

        let cache = MokaCache::new();
        cache.set(word, outcome("词卡产物"));
        assert!(
            cache.get(sentence).is_none(),
            "another kind must not see this entry"
        );
        assert_eq!(cache.get(word).map(|o| o.body), Some("词卡产物".into()));
    }

    #[test]
    fn model_id_participates_in_key() {
        let a = cache_key(&text_task(TaskKind::TranslateWord, "gloss"), "m1");
        let b = cache_key(&text_task(TaskKind::TranslateWord, "gloss"), "m2");
        assert_ne!(a, b);
    }

    #[test]
    fn input_and_options_participate_in_key() {
        let mut hinted = text_task(TaskKind::ExplainCode, "fn main() {}");
        hinted.input = TaskInput::Text {
            text: "fn main() {}".into(),
            hint: Some(InputHint::CodeLanguage("rust".into())),
        };
        assert_ne!(
            cache_key(&text_task(TaskKind::ExplainCode, "fn main() {}"), "m1"),
            cache_key(&hinted, "m1")
        );

        let mut ja = text_task(TaskKind::TranslateSentence, "hello");
        ja.options.target_lang = Some(Lang::Ja);
        assert_ne!(
            cache_key(&text_task(TaskKind::TranslateSentence, "hello"), "m1"),
            cache_key(&ja, "m1")
        );
    }

    #[test]
    fn ttl_expiry_takes_effect() {
        let cache = MokaCache::with_ttl(Duration::from_millis(60));
        let key = cache_key(&text_task(TaskKind::TranslateWord, "gloss"), "m1");
        cache.set(key, outcome("soon gone"));
        assert!(cache.get(key).is_some(), "fresh entry must be readable");

        std::thread::sleep(Duration::from_millis(120));
        cache.run_pending_tasks();
        assert!(cache.get(key).is_none(), "expired entry must be gone");
    }

    #[test]
    fn cache_key_falls_back_when_serialization_fails() {
        let audio = |hint: Option<f32>| Task {
            kind: TaskKind::TranslateSentence,
            input: TaskInput::Audio {
                bytes: std::sync::Arc::from(&b"au"[..]),
                duration_hint: hint,
            },
            options: TaskOptions::default(),
        };
        let nan = cache_key(&audio(Some(f32::NAN)), "m1");
        let nan_again = cache_key(&audio(Some(f32::NAN)), "m1");
        assert_eq!(nan, nan_again, "fallback path must be deterministic");
        assert_ne!(nan, cache_key(&audio(Some(1.5)), "m1"));
    }
}
