use super::*;

impl LauncherApp {
    pub(in crate::app) fn persist(&self) {
        let path = settings_path();
        let mut primary_error = None;
        if let Some(parent) = path.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                primary_error = Some(
                    tr("rust.log.settings.create-directory-failed")
                        .replace("%0", &parent.display().to_string())
                        .replace("%1", &error.to_string()),
                );
            }
        }
        let mut saved_settings = self.settings.clone();
        self.command_line_overrides
            .restore_persisted_values(&mut saved_settings, &self.persisted_settings);
        http::set_certificate_validation(saved_settings.verify_tls_certificates);
        if primary_error.is_none() {
            primary_error = match serde_json::to_vec_pretty(&saved_settings) {
                Ok(bytes) => fs::write(&path, bytes).err().map(|error| {
                    tr("rust.log.settings.write-failed")
                        .replace("%0", &path.display().to_string())
                        .replace("%1", &error.to_string())
                }),
                Err(error) => {
                    Some(tr("rust.log.settings.serialize-failed").replace("%0", &error.to_string()))
                }
            };
        }
        if let Some(error) = primary_error {
            record_launcher_log_event(
                &self.launcher_log,
                LogLevel::Error,
                LogTarget::Settings,
                &error,
            );
        }
        if let Some(external_settings_file) = &self.external_settings_file
            && let Err(error) =
                write_external_settings_file(external_settings_file, &saved_settings)
        {
            record_launcher_log_event(
                &self.launcher_log,
                LogLevel::Error,
                LogTarget::Settings,
                &error,
            );
        }
    }

    pub(in crate::app) fn reset_preferences_to_defaults(&mut self, ctx: &egui::Context) {
        let current = self.settings.clone();
        let mut defaults = SavedSettings::default();

        // Keep launcher data and choices that are outside the source settings panel.
        defaults.accounts = current.accounts;
        defaults.migrated_mojang_accounts = current.migrated_mojang_accounts;
        defaults.offline_account_uuids = current.offline_account_uuids;
        defaults.offline_ely_skins = current.offline_ely_skins;
        defaults.microsoft_accounts = current.microsoft_accounts;
        defaults.ely_accounts = current.ely_accounts;
        defaults.ely_legacy_accounts = current.ely_legacy_accounts;
        defaults.selected_account = current.selected_account;
        defaults.selected_version = current.selected_version;
        defaults.auto_login = current.auto_login;
        defaults.auto_login_timeout_secs = current.auto_login_timeout_secs;
        defaults.settings_blocked = current.settings_blocked;
        defaults.feedback_acknowledged = current.feedback_acknowledged;
        defaults.force_update = current.force_update;
        defaults.client_id = current.client_id;
        defaults.malware_scan_version = current.malware_scan_version;
        defaults.legacy_font_size_revert = current.legacy_font_size_revert;
        defaults.log_window_width = current.log_window_width;
        defaults.log_window_height = current.log_window_height;
        defaults.log_window_x = current.log_window_x;
        defaults.log_window_y = current.log_window_y;
        defaults.hidden_notices_until = current.hidden_notices_until;
        defaults.servers = current.servers;
        defaults.servers_cache_root = current.servers_cache_root;
        defaults.legacy_profiles_path = current.legacy_profiles_path;
        defaults.legacy_config_path = current.legacy_config_path;
        defaults.login_form_direction = "center".to_owned();

        let beta_channel_changed = self.settings.beta_updates != defaults.beta_updates;
        let notices_were_disabled = !self.settings.notices_enabled;
        self.settings = defaults;
        self.command_line_overrides.apply(&mut self.settings);
        http::set_certificate_validation(self.settings.verify_tls_certificates);

        ctx.set_zoom_factor(self.settings.launcher_font_size / 12.0);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(
            self.settings.launcher_window_width as f32,
            self.settings.launcher_window_height as f32,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(
            self.settings.launcher_maximized,
        ));

        self.background_custom_loaded = false;
        if let Some(assets) = &mut self.assets {
            assets.background = if self.background_theme_dark.unwrap_or(false) {
                assets.background_dark.clone()
            } else {
                assets.background_light.clone()
            };
        }
        self.show_log_window =
            self.settings.logger_mode == "global" || self.command_line_overrides.debug_mode;
        self.show_live_launcher_log = self.show_log_window;
        self.show_live_game_log = false;

        if notices_were_disabled {
            self.refresh_notices();
        }
        if beta_channel_changed && !self.update_checking {
            self.available_release = None;
            self.update_prompt_pending = false;
            self.show_update_dialog = false;
            self.start_update_check();
        }

        self.persist();
        self.set_logged_status(
            LogLevel::Info,
            LogTarget::Settings,
            tr("rust.status.settings.reset"),
        );
    }

    pub(in crate::app) fn apply_imported_settings_effects(
        &mut self,
        previous: &SavedSettings,
        ctx: &egui::Context,
    ) -> Option<String> {
        let mut background_load_error = None;
        ctx.set_zoom_factor(self.settings.launcher_font_size / 12.0);
        http::set_certificate_validation(self.settings.verify_tls_certificates);

        if previous.launcher_window_width != self.settings.launcher_window_width
            || previous.launcher_window_height != self.settings.launcher_window_height
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(
                self.settings.launcher_window_width as f32,
                self.settings.launcher_window_height as f32,
            )));
        }
        if previous.launcher_maximized != self.settings.launcher_maximized {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(
                self.settings.launcher_maximized,
            ));
        }

        if previous.background_path != self.settings.background_path {
            if self.settings.background_path.trim().is_empty() {
                self.background_custom_loaded = false;
                if let Some(assets) = &mut self.assets {
                    assets.background = if self.background_theme_dark.unwrap_or(false) {
                        assets.background_dark.clone()
                    } else {
                        assets.background_light.clone()
                    };
                }
            } else {
                self.background_custom_loaded = false;
                if let Some(assets) = &mut self.assets {
                    assets.background = if self.background_theme_dark.unwrap_or(false) {
                        assets.background_dark.clone()
                    } else {
                        assets.background_light.clone()
                    };
                }
                let path = PathBuf::from(self.settings.background_path.trim());
                match load_texture_from_path(ctx, "background", &path) {
                    Ok(background) => {
                        self.background_custom_loaded = true;
                        if let Some(assets) = &mut self.assets {
                            assets.background = background;
                        }
                    }
                    Err(error) => background_load_error = Some(error),
                }
            }
        }

        if previous.logger_mode != self.settings.logger_mode {
            self.show_log_window =
                self.settings.logger_mode == "global" || self.command_line_overrides.debug_mode;
            self.show_live_launcher_log = self.show_log_window;
            self.show_live_game_log = false;
        }
        if previous.notices_enabled != self.settings.notices_enabled {
            self.notice_events = None;
            self.notices_loading = false;
            self.personal_notice_list.clear();
            self.refresh_notices();
            self.rebuild_notice_list(ctx);
        }
        if previous.beta_updates != self.settings.beta_updates {
            self.update_check_events = None;
            self.update_checking = false;
            self.available_release = None;
            self.update_prompt_pending = false;
            self.show_update_dialog = false;
            self.start_update_check();
        }

        background_load_error
    }

    pub(in crate::app) fn set_logged_status(
        &mut self,
        level: LogLevel,
        target: LogTarget,
        message: impl Into<String>,
    ) {
        let message = message.into();
        record_launcher_log_event(&self.launcher_log, level, target, &message);
        self.status = message;
    }

    pub(in crate::app) fn open_path_with_status(&mut self, path: &Path, target: LogTarget) {
        if let Err(error) = open_path(path) {
            self.set_logged_status(LogLevel::Error, target, error);
        }
    }

    pub(in crate::app) fn create_and_open_folder_with_status(
        &mut self,
        path: &Path,
        target: LogTarget,
    ) {
        if let Err(error) = fs::create_dir_all(path) {
            self.set_logged_status(
                LogLevel::Error,
                target,
                tr("rust.status.folder.create-error")
                    .replace("%0", &path.display().to_string())
                    .replace("%1", &error.to_string()),
            );
            return;
        }
        self.open_path_with_status(path, target);
    }

    pub(in crate::app) fn open_url_with_status(&mut self, url: &str, target: LogTarget) -> bool {
        match open_url(url) {
            Ok(()) => true,
            Err(error) => {
                self.set_logged_status(LogLevel::Error, target, error);
                false
            }
        }
    }
}
