//! 设置窗口的编辑会话生命周期：打开、保存（密钥 + 配置）、热键重注册、
//! 用户提示与关闭。

use gloss_core::config::Config;
use gloss_core::log::{info, thread, warn};

use crate::i18n::Text;
use crate::ui::settings::{KeyUpdate, SettingsNotice};

use super::GlossApp;

impl GlossApp {
    /// 打开设置窗口的统一入口（托盘/热键的 `OpenSettingsRequested` 与浮层
    /// 失败卡的「打开设置」走同一条路）。窗口已可见时只聚焦；否则以当前
    /// 快照开一个新编辑会话——未保存的草稿随旧会话一并作废。
    pub(super) fn open_settings(&mut self) {
        // 窗口标题按当前界面语言写入：窗内文案取自同一份快照语言，两者同语。
        let title = Text::get(self.locale()).gloss_app_settings_title.as_str();
        if self.settings.is_some() {
            if let Some(windows) = &self.windows {
                windows.show_settings(title);
            }
            return;
        }
        self.settings = Some(crate::ui::settings::open(&self.config.snapshot()));
        if let Some(windows) = &self.windows {
            windows.show_settings(title);
            windows.request_redraw_settings();
        }
        info!(thread = thread::UI, "settings window opened");
    }

    /// 保存设置：密钥按 [`KeyUpdate`] 处理（失败即中止，不留下「密钥换了
    /// 配置没换」的半截状态），配置走热更新路径（先落盘再换快照）；成功即
    /// 关闭窗口——「下一次任务即生效」由快照语义保证。
    pub(super) fn save_settings(&mut self, config: Config, key_update: KeyUpdate) {
        let keychain_id = config.resolved_provider().keychain_id.clone();
        let key_result = match &key_update {
            KeyUpdate::Keep => Ok(()),
            KeyUpdate::Replace(key) => self.store.set_secret(&keychain_id, key),
            KeyUpdate::Clear => self.store.delete_secret(&keychain_id),
        };
        if let Err(err) = key_result {
            warn!(thread = thread::UI, error = %err, "failed to update the api key");
            self.report_settings(SettingsNotice::KeyUpdateFailed(err));
            return;
        }
        if key_update != KeyUpdate::Keep {
            info!(
                thread = thread::UI,
                provider = %config.resolved_provider().provider,
                cleared = key_update == KeyUpdate::Clear,
                "api key updated from settings"
            );
        }
        let language = config.language;
        if let Err(err) = self.config.save(config) {
            // 密钥已经生效，配置没有：如实说清哪一半落下了。
            warn!(thread = thread::UI, error = %err, "failed to save settings");
            let notice = if key_update == KeyUpdate::Keep {
                SettingsNotice::SaveFailed(err)
            } else {
                SettingsNotice::KeyUpdatedSaveFailed(err)
            };
            self.report_settings(notice);
            return;
        }
        info!(
            thread = thread::UI,
            language = ?language,
            "settings saved, effective on the next trigger"
        );
        // 热键不受「下一次触发才生效」约束：注册是平台侧的即时动作，保存
        // 成功即按新表重注册。
        self.rebind_hotkeys();
        self.close_settings();
    }

    /// 按当前快照重注册热键：绑定读自刚换上的快照。个别绑定被
    /// 占用时按 [`HotkeyBinder`] 的降级契约告警跳过，保存不整体失败。
    fn rebind_hotkeys(&self) {
        let bindings = self.config.snapshot().hotkey_bindings.clone();
        let applied = self.hotkeys.rebind(&bindings);
        info!(
            thread = thread::UI,
            declared = bindings.len(),
            applied,
            "hotkey bindings re-registered after save"
        );
    }

    /// 设置窗口的用户提示（保存失败等）；窗口已关则无处可报，只留日志。
    fn report_settings(&mut self, notice: SettingsNotice) {
        if let Some(state) = &mut self.settings {
            state.report(notice);
        }
    }

    /// 关闭设置窗口：隐藏不销毁，丢弃编辑会话（未保存的草稿一并作废）。
    pub(super) fn close_settings(&mut self) {
        self.settings = None;
        self.settings_repaint = None;
        if let Some(windows) = &self.windows {
            windows.hide_settings();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gloss_core::model::{GlossError, Lang};
    use gloss_core::ports::HotkeyBinder;
    use gloss_core::task::{HotkeyBinding, InputSource, TaskKind};

    use crate::app::test_support::{
        driven_app, driven_app_using, driven_app_with, text_input, trigger_selection,
    };
    use crate::channel::PlatformEvent;
    use crate::stubs::ports::{MemoryConfigStore, RecordingHotkeyBinder};
    use crate::ui;
    use crate::ui::settings::{KeyUpdate, SettingsNotice};

    #[test]
    fn open_settings_request_starts_an_edit_session() {
        let (mut app, _config, _store, pe_tx, _ac_rx, _cmd_rx, _ev_tx) = driven_app();
        pe_tx.send(PlatformEvent::OpenSettingsRequested).unwrap();
        app.drain_platform_events();

        let state = app.settings.as_ref().expect("settings session expected");
        assert_eq!(
            state.draft(),
            &*app.config.snapshot(),
            "draft must start from the current snapshot"
        );
        assert_eq!(
            app.machine.generation(),
            0,
            "settings must not consume a gen"
        );
    }

    #[test]
    fn settings_save_writes_keychain_and_swaps_config() {
        let (mut app, config, store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
        pe_tx.send(PlatformEvent::OpenSettingsRequested).unwrap();
        app.drain_platform_events();

        let mut draft = (*app.config.snapshot()).clone();
        draft.target_lang = Lang::Ja;
        draft.set_model_for_kind(
            gloss_core::task::TaskKind::TranslateWord,
            "deepseek-reasoner",
        );
        app.save_settings(draft, KeyUpdate::Replace("sk-live-key".to_owned()));

        assert_eq!(
            store
                .secret("gloss/deepseek")
                .expect("store read")
                .as_deref(),
            Some("sk-live-key"),
            "trimmed key must land in the keychain under the provider entry"
        );
        assert_eq!(config.snapshot().target_lang, Lang::Ja, "snapshot advanced");
        assert!(app.settings.is_none(), "successful save closes the session");

        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("A")));
        let crate::channel::Command::RunTask { task, .. } = cmd_rx.try_recv().unwrap().payload;
        assert_eq!(
            task.options.model_override.as_deref(),
            Some("deepseek-reasoner")
        );
    }

    #[test]
    fn clearing_the_key_deletes_the_secret_on_save() {
        let (mut app, config, store, pe_tx, _ac_rx, _cmd_rx, _ev_tx) = driven_app();
        store
            .set_secret("gloss/deepseek", "sk-existing")
            .expect("stub store accepts secret");
        pe_tx.send(PlatformEvent::OpenSettingsRequested).unwrap();
        app.drain_platform_events();

        let draft = (*config.snapshot()).clone();
        app.save_settings(draft, KeyUpdate::Clear);
        assert_eq!(
            store.secret("gloss/deepseek").expect("store read"),
            None,
            "clear must remove the keychain entry"
        );
        assert!(app.settings.is_none(), "save closes the session");
    }

    #[test]
    fn failed_save_keeps_the_session_open_with_a_notice() {
        let failing = MemoryConfigStore::default()
            .with_save_failure(GlossError::Config("disk on fire".into()));
        let (mut app, config, _store, _pe_tx, _ac_rx, _cmd_rx, _ev_tx) =
            driven_app_with(Arc::new(failing));
        app.settings = Some(ui::settings::open(&config.snapshot()));

        let mut draft = (*config.snapshot()).clone();
        draft.target_lang = Lang::Ja;
        app.save_settings(draft, KeyUpdate::Keep);

        let state = app.settings.as_ref().expect("session must stay open");
        assert_eq!(
            state.notice(),
            Some(&SettingsNotice::SaveFailed(GlossError::Config(
                "disk on fire".into()
            ))),
            "the save error must be reported into the session as a typed notice"
        );
        assert_eq!(
            config.snapshot().target_lang,
            Lang::Zh,
            "failed save must not advance the runtime snapshot"
        );
    }

    #[test]
    fn saving_settings_rebinds_hotkeys_from_the_new_snapshot() {
        let binder = Arc::new(RecordingHotkeyBinder::default());
        let (mut app, config, _store, pe_tx, _ac_rx, _cmd_rx, _ev_tx) = driven_app_using(
            Arc::new(MemoryConfigStore::default()),
            Arc::clone(&binder) as Arc<dyn HotkeyBinder>,
        );
        assert_eq!(
            binder.call_count(),
            0,
            "the startup registration belongs to the assembly point, not the App"
        );

        pe_tx.send(PlatformEvent::OpenSettingsRequested).unwrap();
        app.drain_platform_events();

        let mut draft = (*config.snapshot()).clone();
        draft.hotkey_bindings = vec![
            HotkeyBinding {
                trigger: "Cmd+Alt+T".into(),
                kind: TaskKind::TranslateSentence,
                source: InputSource::Selection,
            },
            HotkeyBinding {
                trigger: "Cmd+Alt+C".into(),
                kind: TaskKind::ExplainCode,
                source: InputSource::Selection,
            },
        ];
        app.save_settings(draft, KeyUpdate::Keep);

        assert_eq!(binder.call_count(), 1, "one save means one rebind");
        let rebound = binder.last().expect("a successful save must rebind");
        let triggers: Vec<&str> = rebound.iter().map(|b| b.trigger.as_str()).collect();
        assert_eq!(triggers, ["Cmd+Alt+T", "Cmd+Alt+C"]);
    }

    #[test]
    fn every_save_rebinds_hotkeys_not_just_the_first() {
        let binder = Arc::new(RecordingHotkeyBinder::default());
        let (mut app, config, _store, _pe_tx, _ac_rx, _cmd_rx, _ev_tx) = driven_app_using(
            Arc::new(MemoryConfigStore::default()),
            Arc::clone(&binder) as Arc<dyn HotkeyBinder>,
        );

        for trigger in ["Cmd+Alt+T", "Cmd+Alt+R"] {
            app.settings = Some(ui::settings::open(&config.snapshot()));
            let mut draft = (*config.snapshot()).clone();
            draft.hotkey_bindings = vec![HotkeyBinding {
                trigger: trigger.into(),
                kind: TaskKind::TranslateSentence,
                source: InputSource::Selection,
            }];
            app.save_settings(draft, KeyUpdate::Keep);
        }

        assert_eq!(binder.call_count(), 2, "两次保存 = 两次重注册");
        let rebound = binder.last().expect("a successful save must rebind");
        assert_eq!(
            rebound
                .iter()
                .map(|b| b.trigger.as_str())
                .collect::<Vec<_>>(),
            ["Cmd+Alt+R"],
            "第二次生效的必须是第二次保存的那份，不是第一次的"
        );
    }

    #[test]
    fn failed_save_does_not_rebind_hotkeys() {
        let binder = Arc::new(RecordingHotkeyBinder::default());
        let failing = MemoryConfigStore::default()
            .with_save_failure(GlossError::Config("disk on fire".into()));
        let (mut app, config, _store, _pe_tx, _ac_rx, _cmd_rx, _ev_tx) = driven_app_using(
            Arc::new(failing),
            Arc::clone(&binder) as Arc<dyn HotkeyBinder>,
        );
        app.settings = Some(ui::settings::open(&config.snapshot()));

        let mut draft = (*config.snapshot()).clone();
        draft.hotkey_bindings = vec![HotkeyBinding {
            trigger: "Cmd+Alt+T".into(),
            kind: TaskKind::TranslateSentence,
            source: InputSource::Selection,
        }];
        app.save_settings(draft, KeyUpdate::Keep);

        assert_eq!(
            binder.call_count(),
            0,
            "a failed save keeps the old bindings live"
        );
    }
}
