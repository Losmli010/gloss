//! 端口桩：本包测试用到的配置存储、场景探针与平台能力预置替身。

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use gloss_core::config::Config;
use gloss_core::guard::SceneFacts;
use gloss_core::model::{GlossError, Locale};
use gloss_core::ports::{
    AppIcon, ClipboardWriter, ConfigStore, DesktopShortcut, MenuController, PermissionProbe,
    SceneProbe, SecretStore,
};
use gloss_platform::bootstrap::Platform;

use super::lock_or_recover;

#[derive(Default)]
pub struct MemoryConfigStore {
    secrets: Mutex<HashMap<String, String>>,
    config: Mutex<Option<Config>>,
    load_failure: Mutex<Option<GlossError>>,
    save_failure: Mutex<Option<GlossError>>,
    secret_failure: Mutex<Option<GlossError>>,
}

impl MemoryConfigStore {
    pub fn with_load_failure(self, error: GlossError) -> Self {
        *lock_or_recover(&self.load_failure) = Some(error);
        self
    }

    pub fn with_save_failure(self, error: GlossError) -> Self {
        *lock_or_recover(&self.save_failure) = Some(error);
        self
    }

    pub fn with_secret_failure(self, error: GlossError) -> Self {
        *lock_or_recover(&self.secret_failure) = Some(error);
        self
    }
}

impl ConfigStore for MemoryConfigStore {
    fn load(&self) -> Result<Config, GlossError> {
        if let Some(err) = lock_or_recover(&self.load_failure).clone() {
            return Err(err);
        }
        Ok(lock_or_recover(&self.config).clone().unwrap_or_default())
    }

    fn save(&self, config: &Config) -> Result<(), GlossError> {
        if let Some(err) = lock_or_recover(&self.save_failure).clone() {
            return Err(err);
        }
        *lock_or_recover(&self.config) = Some(config.clone());
        Ok(())
    }
}

impl SecretStore for MemoryConfigStore {
    fn secret(&self, key: &str) -> Result<Option<String>, GlossError> {
        if let Some(err) = lock_or_recover(&self.secret_failure).clone() {
            return Err(err);
        }
        Ok(lock_or_recover(&self.secrets).get(key).cloned())
    }

    fn set_secret(&self, key: &str, value: &str) -> Result<(), GlossError> {
        lock_or_recover(&self.secrets).insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete_secret(&self, key: &str) -> Result<(), GlossError> {
        lock_or_recover(&self.secrets).remove(key);
        Ok(())
    }
}

#[derive(Default)]
pub struct StubSceneProbe {
    facts: Mutex<SceneFacts>,
}

impl StubSceneProbe {
    pub fn with_facts(facts: SceneFacts) -> Self {
        Self {
            facts: Mutex::new(facts),
        }
    }

    pub fn set_facts(&self, facts: SceneFacts) {
        *lock_or_recover(&self.facts) = facts;
    }
}

impl SceneProbe for StubSceneProbe {
    fn facts(&self) -> SceneFacts {
        lock_or_recover(&self.facts).clone()
    }
}

pub struct StubAppIcon;

impl AppIcon for StubAppIcon {
    fn install(&self, _png: &[u8]) -> Result<(), GlossError> {
        Ok(())
    }
}

pub struct StubDesktopShortcut;

impl DesktopShortcut for StubDesktopShortcut {
    fn ensure(&self) -> Result<(), GlossError> {
        Ok(())
    }
}

pub struct StubMenuController;

impl MenuController for StubMenuController {
    fn install(
        &self,
        _locale: Locale,
        _on_open_settings: Box<dyn Fn() + Send>,
        _on_quit: Box<dyn Fn() + Send>,
    ) -> bool {
        false
    }
}

pub struct StubClipboardWriter;

impl ClipboardWriter for StubClipboardWriter {
    fn copy_text(&self, _text: &str) -> Result<(), String> {
        Ok(())
    }
}

pub struct StubPermissionProbe;

impl PermissionProbe for StubPermissionProbe {
    fn preflight(&self) -> bool {
        true
    }

    fn request(&self) -> bool {
        true
    }

    fn watch(&self, _grant_pending: bool) -> Box<dyn FnMut() -> bool + Send> {
        Box::new(|| false)
    }
}

pub fn stub_platform(store: &Arc<MemoryConfigStore>, scene: Arc<dyn SceneProbe>) -> Platform {
    Platform {
        scene,
        config_store: Arc::clone(store) as Arc<dyn ConfigStore>,
        secret_store: Arc::clone(store) as Arc<dyn SecretStore>,
        system_locale: Locale::Zh,
        app_icon: Box::new(StubAppIcon),
        desktop_shortcut: Box::new(StubDesktopShortcut),
        menu: Box::new(StubMenuController),
        clipboard: Box::new(StubClipboardWriter),
        permission: Box::new(StubPermissionProbe),
        gesture: None,
        gesture_degraded: Arc::new(AtomicBool::new(false)),
        event_thread: None,
    }
}
