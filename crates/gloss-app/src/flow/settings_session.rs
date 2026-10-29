//! 设置窗口的编辑会话生命周期：打开、保存（密钥与配置）、用户提示与关闭。

use std::sync::atomic::Ordering;

use gloss_core::config::Config;
use gloss_core::log::{info, thread, warn};

use crate::ui::i18n::Text;
use crate::ui::settings::{KeyUpdate, SettingsNotice};

use crate::app::GlossApp;

impl GlossApp {
    pub(crate) fn open_settings(&mut self) {
        let title = Text::get(self.env.locale())
            .gloss_app_settings_title
            .as_str();
        if self.settings.is_some() {
            if let Some(windows) = &self.workspace.windows {
                windows.show_settings(title);
            }
            return;
        }
        self.settings = Some(crate::ui::settings::open(&self.env.config.snapshot()));
        if let Some(windows) = &self.workspace.windows {
            windows.show_settings(title);
            windows.request_redraw_settings();
        }
        info!(thread = thread::UI, "settings window opened");
    }

    pub(crate) fn save_settings(&mut self, config: Config, key_update: KeyUpdate) {
        let keychain_id = config.resolved_provider().keychain_id.as_str();
        let key_result = match &key_update {
            KeyUpdate::Keep => Ok(()),
            KeyUpdate::Replace(key) => self.env.store.set_secret(keychain_id, key),
            KeyUpdate::Clear => self.env.store.delete_secret(keychain_id),
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
        let watch_clipboard_images = config.watch_clipboard_images;
        if let Err(err) = self.env.config.save(config) {
            warn!(thread = thread::UI, error = %err, "failed to save settings");
            let notice = if key_update == KeyUpdate::Keep {
                SettingsNotice::SaveFailed(err)
            } else {
                SettingsNotice::KeyUpdatedSaveFailed(err)
            };
            self.report_settings(notice);
            return;
        }
        self.env
            .clipboard_watch_enabled
            .store(watch_clipboard_images, Ordering::Relaxed);
        info!(
            thread = thread::UI,
            language = ?language,
            watch_clipboard_images,
            "settings saved"
        );
        self.close_settings();
    }

    fn report_settings(&mut self, notice: SettingsNotice) {
        if let Some(state) = &mut self.settings {
            state.report(notice);
        }
    }

    pub(crate) fn close_settings(&mut self) {
        self.settings = None;
        self.workspace.settings_repaint = None;
        if let Some(windows) = &self.workspace.windows {
            windows.hide_settings();
        }
        self.workspace.drop_settings_frame();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    use gloss_core::model::{GlossError, Lang};

    use crate::app::test_support::{driven_app, driven_app_with, text_input, trigger_selection};
    use crate::channel::PlatformEvent;
    use crate::stubs::ports::MemoryConfigStore;
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
            &*app.env.config.snapshot(),
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

        let mut draft = (*app.env.config.snapshot()).clone();
        draft.target_lang = Lang::En;
        draft.model = "deepseek-reasoner".into();
        app.save_settings(draft, KeyUpdate::Replace("sk-live-key".to_owned()));

        assert_eq!(
            store
                .secret("gloss/deepseek")
                .expect("store read")
                .as_deref(),
            Some("sk-live-key"),
            "trimmed key must land in the keychain under the provider entry"
        );
        assert_eq!(config.snapshot().target_lang, Lang::En, "snapshot advanced");
        assert!(app.settings.is_none(), "successful save closes the session");

        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("A")));
        app.handle_overlay_action(crate::ui::popup::OverlayAction::Confirm);
        let crate::channel::Command::RunTask { options, .. } = cmd_rx.try_recv().unwrap().payload
        else {
            panic!("expected a RunTask command, got another variant")
        };
        assert_eq!(
            options.model.as_ref(),
            "deepseek-reasoner",
            "the saved model freezes into the next task"
        );
    }

    #[test]
    fn settings_save_hot_toggles_the_clipboard_watch_switch() {
        let (mut app, config, _store, pe_tx, _ac_rx, _cmd_rx, _ev_tx) = driven_app();
        assert!(
            app.env.clipboard_watch_enabled.load(Ordering::Relaxed),
            "the factory default starts the sentinel on"
        );
        pe_tx.send(PlatformEvent::OpenSettingsRequested).unwrap();
        app.drain_platform_events();

        let mut draft = (*app.env.config.snapshot()).clone();
        draft.watch_clipboard_images = false;
        app.save_settings(draft, KeyUpdate::Keep);

        assert!(
            !config.snapshot().watch_clipboard_images,
            "snapshot advanced"
        );
        assert!(
            !app.env.clipboard_watch_enabled.load(Ordering::Relaxed),
            "switching off must silence the sentinel immediately, no restart"
        );

        pe_tx.send(PlatformEvent::OpenSettingsRequested).unwrap();
        app.drain_platform_events();
        let mut draft = (*app.env.config.snapshot()).clone();
        draft.watch_clipboard_images = true;
        app.save_settings(draft, KeyUpdate::Keep);

        assert!(
            app.env.clipboard_watch_enabled.load(Ordering::Relaxed),
            "switching back on must revive the sentinel immediately"
        );
        assert!(app.settings.is_none(), "save closes the session");
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
        draft.target_lang = Lang::En;
        draft.watch_clipboard_images = false;
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
        assert!(
            app.env.clipboard_watch_enabled.load(Ordering::Relaxed),
            "failed save must not flip the sentinel bit"
        );
    }
}
