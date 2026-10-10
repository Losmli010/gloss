//! 设置窗口的编辑会话生命周期：打开、保存（密钥 + 配置）、用户提示与关闭。

use std::sync::atomic::Ordering;

use gloss_core::config::Config;
use gloss_core::log::{info, thread, warn};

use crate::ui::i18n::Text;
use crate::ui::settings::{KeyUpdate, SettingsNotice};

use crate::app::GlossApp;

impl GlossApp {
    /// 打开设置窗口的统一入口（托盘的 `OpenSettingsRequested` 与浮层
    /// 失败卡的「打开设置」走同一条路）。窗口已可见时只聚焦；否则以当前
    /// 快照开一个新编辑会话——未保存的草稿随旧会话一并作废。
    pub(crate) fn open_settings(&mut self) {
        // 窗口标题按当前界面语言写入：窗内文案取自同一份快照语言，两者同语。
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

    /// 保存设置：密钥按 [`KeyUpdate`] 处理（失败即中止，不留下「密钥换了
    /// 配置没换」的半截状态），配置走热更新路径（先落盘再换快照）；成功即
    /// 关闭窗口——「下一次任务即生效」由快照语义保证。剪贴板图片哨兵的
    /// 共享开关位随保存置位：事件线程的哨兵源只读这一位，热切换不重启
    /// （落盘失败不置位，与快照同进退）。
    pub(crate) fn save_settings(&mut self, config: Config, key_update: KeyUpdate) {
        let keychain_id = config.resolved_provider().keychain_id.clone();
        let key_result = match &key_update {
            KeyUpdate::Keep => Ok(()),
            KeyUpdate::Replace(key) => self.env.store.set_secret(&keychain_id, key),
            KeyUpdate::Clear => self.env.store.delete_secret(&keychain_id),
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

    /// 设置窗口的用户提示（保存失败等）；窗口已关则无处可报，只留日志。
    fn report_settings(&mut self, notice: SettingsNotice) {
        if let Some(state) = &mut self.settings {
            state.report(notice);
        }
    }

    /// 关闭设置窗口：隐藏不销毁，丢弃编辑会话（未保存的草稿一并作废）。
    pub(crate) fn close_settings(&mut self) {
        self.settings = None;
        self.workspace.settings_repaint = None;
        if let Some(windows) = &self.workspace.windows {
            windows.hide_settings();
        }
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
        draft.target_lang = Lang::Ja;
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
        assert_eq!(config.snapshot().target_lang, Lang::Ja, "snapshot advanced");
        assert!(app.settings.is_none(), "successful save closes the session");

        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("A")));
        let crate::channel::Command::RunTask { options, .. } = cmd_rx.try_recv().unwrap().payload
        else {
            panic!("expected a RunTask command, got another variant")
        };
        assert_eq!(
            options.model, "deepseek-reasoner",
            "the saved model freezes into the next task"
        );
    }

    #[test]
    fn settings_save_hot_toggles_the_clipboard_watch_switch() {
        let (mut app, config, _store, pe_tx, _ac_rx, _cmd_rx, _ev_tx) = driven_app();
        assert!(
            !app.env.clipboard_watch_enabled.load(Ordering::Relaxed),
            "the factory default starts the sentinel off"
        );
        pe_tx.send(PlatformEvent::OpenSettingsRequested).unwrap();
        app.drain_platform_events();

        let mut draft = (*app.env.config.snapshot()).clone();
        draft.watch_clipboard_images = true;
        app.save_settings(draft, KeyUpdate::Keep);

        assert!(
            config.snapshot().watch_clipboard_images,
            "snapshot advanced"
        );
        assert!(
            app.env.clipboard_watch_enabled.load(Ordering::Relaxed),
            "the sentinel bit must flip on save, no restart"
        );

        pe_tx.send(PlatformEvent::OpenSettingsRequested).unwrap();
        app.drain_platform_events();
        let mut draft = (*app.env.config.snapshot()).clone();
        draft.watch_clipboard_images = false;
        app.save_settings(draft, KeyUpdate::Keep);

        assert!(
            !app.env.clipboard_watch_enabled.load(Ordering::Relaxed),
            "switching off must silence the sentinel immediately"
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
        draft.target_lang = Lang::Ja;
        draft.watch_clipboard_images = true;
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
            !app.env.clipboard_watch_enabled.load(Ordering::Relaxed),
            "failed save must not flip the sentinel bit"
        );
    }
}
