use super::*;

impl LauncherApp {
    pub(in crate::app) fn refresh_notices(&mut self) {
        if self.notices_loading {
            return;
        }
        if !self.settings.notices_enabled {
            self.personal_notice_list.clear();
            self.notice_list.clone_from(&self.bootstrap_notice_list);
            self.notice_error = None;
            self.set_logged_status(
                LogLevel::Info,
                LogTarget::Notices,
                tr("rust.status.notices.disabled"),
            );
            return;
        }
        self.notice_events = Some(spawn_notice_fetch(self.settings.client_id.clone()));
        self.notices_loading = true;
        self.notice_error = None;
        record_launcher_log_event(
            &self.launcher_log,
            LogLevel::Info,
            LogTarget::Notices,
            &tr("rust.log.notices.refresh"),
        );
    }

    pub(in crate::app) fn rebuild_notice_list(&mut self, ctx: &Context) {
        self.notice_list.clone_from(&self.bootstrap_notice_list);
        self.notice_list
            .extend(self.personal_notice_list.iter().cloned());
        self.notice_list
            .sort_by_key(|notice| (notice.order, notice.id.clone()));
        self.notice_images.clear();
        for notice in &self.notice_list {
            let Some(bytes) = &notice.image else {
                continue;
            };
            if let Some(texture) =
                load_optional_texture(ctx, &format!("notice_image_{}", notice.id), bytes)
            {
                self.notice_images.insert(notice.id.clone(), texture);
            }
        }
    }

    pub(in crate::app) fn diagnostic_report(&self) -> String {
        let mut report = format!(
            "Legacy Launcher Rust diagnostic report\nLauncher version: {}\nPlatform: {} {}\nMinecraft version: {}\nMod loader: {} {}\n\n{}\n\nRecent game output:\n",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            self.settings.selected_version,
            self.settings.loader_type,
            self.settings.loader_build,
            diagnostics::system_info(),
        );
        let lines = self
            .game_log
            .lock()
            .map(|lines| lines.iter().rev().take(1200).cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        for line in lines.into_iter().rev() {
            let lower = line.to_ascii_lowercase();
            if [
                "token",
                "secret",
                "authorization",
                "bearer ",
                "xsts",
                "xuid",
            ]
            .iter()
            .any(|sensitive| lower.contains(sensitive))
            {
                report.push_str("[sensitive log line omitted]\n");
            } else {
                report.push_str(&line);
                report.push('\n');
            }
            if report.len() >= 700_000 {
                break;
            }
        }
        report.push_str("\nRecent launcher output:\n");
        let launcher_lines = self
            .launcher_log
            .lock()
            .map(|lines| {
                lines
                    .iter()
                    .rev()
                    .take(1200)
                    .map(LogRecord::render)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for line in launcher_lines.into_iter().rev() {
            let lower = line.to_ascii_lowercase();
            if [
                "token",
                "secret",
                "authorization",
                "bearer ",
                "xsts",
                "xuid",
            ]
            .iter()
            .any(|sensitive| lower.contains(sensitive))
            {
                report.push_str("[sensitive log line omitted]\n");
            } else {
                report.push_str(&redact_sensitive_log_values(&line));
                report.push('\n');
            }
            if report.len() >= 700_000 {
                break;
            }
        }
        report.chars().take(700_000).collect()
    }

    pub(in crate::app) fn start_diagnostic_upload(&mut self) {
        let report = self.diagnostic_report();
        self.diagnostic_payload = Some(report.clone());
        self.diagnostic_events = Some(spawn_diagnostic_upload(report));
        self.diagnostic_pending = true;
        self.diagnostic_result_url = None;
        self.diagnostic_error = None;
        record_launcher_log_event(
            &self.launcher_log,
            LogLevel::Info,
            LogTarget::Diagnostics,
            &tr("rust.log.diagnostics.uploading"),
        );
    }

    pub(in crate::app) fn open_support_flow(&mut self) {
        self.show_diagnostic_consent = true;
        self.diagnostic_send_enabled = true;
        self.diagnostic_error = None;
    }

    pub(in crate::app) fn save_diagnostic_report(&mut self) {
        let Some(report) = self.diagnostic_payload.as_ref() else {
            return;
        };
        let path = settings_path()
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("diagnostic.log");
        let result = path
            .parent()
            .map(fs::create_dir_all)
            .transpose()
            .and_then(|_| fs::write(&path, report));
        match result {
            Ok(()) => {
                self.set_logged_status(
                    LogLevel::Info,
                    LogTarget::Diagnostics,
                    tr("rust.status.diagnostics.saved").replace("%0", &path.display().to_string()),
                );
                self.show_diagnostic_consent = false;
                self.diagnostic_error = None;
            }
            Err(error) => {
                self.diagnostic_error = Some(
                    tr("rust.status.diagnostics.save-error").replace("%0", &error.to_string()),
                );
            }
        }
    }

    pub(in crate::app) fn visible_notice_indices(&self) -> Vec<usize> {
        let now = unix_time_millis();
        self.notice_list
            .iter()
            .enumerate()
            .filter(|(_, notice)| {
                (!notice.promoted || self.settings.promoted_notices_enabled)
                    && self
                        .settings
                        .hidden_notices_until
                        .get(&notice.id)
                        .is_none_or(|expiry| *expiry <= now)
            })
            .map(|(index, _)| index)
            .collect()
    }

    pub(in crate::app) fn hide_notice_for_week(&mut self, id: &str) {
        const WEEK_MS: u64 = 7 * 24 * 60 * 60 * 1000;
        self.settings
            .hidden_notices_until
            .insert(id.to_owned(), unix_time_millis().saturating_add(WEEK_MS));
        self.notice_index = self.notice_index.saturating_add(1);
        self.persist();
    }

    pub(in crate::app) fn toggle_notice_visibility_for_week(&mut self, id: &str) {
        const WEEK_MS: u64 = 7 * 24 * 60 * 60 * 1000;
        let now = unix_time_millis();
        let is_hidden = self
            .settings
            .hidden_notices_until
            .get(id)
            .is_some_and(|expiry| *expiry > now);
        if is_hidden {
            self.settings.hidden_notices_until.remove(id);
        } else {
            self.settings
                .hidden_notices_until
                .insert(id.to_owned(), now.saturating_add(WEEK_MS));
        }
        self.persist();
    }

    pub(in crate::app) fn perform_notice_action(
        &mut self,
        action: notices::NoticeAction,
        _ctx: &Context,
    ) {
        match action {
            notices::NoticeAction::Url { url, .. } => {
                self.open_url_with_status(&url, LogTarget::Notices);
            }
            notices::NoticeAction::Server {
                label,
                address,
                accounts,
                families,
            } => {
                self.show_notice_scene = false;
                let name = label
                    .strip_prefix("Join ")
                    .unwrap_or(tr("rust.ui.servers.promoted"))
                    .to_owned();
                if let Some(allowed_accounts) = accounts {
                    let selected_type = self.settings.selected_account.as_ref().map(|selected| {
                        if self.settings.accounts.iter().any(|name| name == selected) {
                            "plain"
                        } else if self
                            .settings
                            .microsoft_accounts
                            .iter()
                            .any(|account| &account.id == selected)
                        {
                            "minecraft"
                        } else if self
                            .settings
                            .ely_accounts
                            .iter()
                            .any(|account| &account.id == selected)
                        {
                            "ely"
                        } else if self
                            .settings
                            .ely_legacy_accounts
                            .iter()
                            .any(|account| &account.id == selected)
                        {
                            "ely_legacy"
                        } else {
                            "unknown"
                        }
                    });
                    if selected_type
                        .is_some_and(|kind| !allowed_accounts.iter().any(|allowed| allowed == kind))
                    {
                        self.set_logged_status(
                            LogLevel::Warn,
                            LogTarget::Servers,
                            tr("rust.status.servers.account-required")
                                .replace("%0", &allowed_accounts.join(", ")),
                        );
                        self.account_manager_view = AccountManagerView::Overview;
                        self.settings_page = Some("Manage accounts");
                        return;
                    }
                }
                let server = SavedServer {
                    name,
                    address,
                    ..Default::default()
                };
                let current_version = self.settings.selected_version.clone();
                let family_versions = self
                    .promoted_family_versions(&families)
                    .into_iter()
                    .filter(|version| version != &current_version)
                    .collect();
                self.pending_promoted_server = Some(PendingPromotedServerLaunch {
                    server,
                    current_version,
                    family_versions,
                });
            }
            notices::NoticeAction::Scene { scene, .. } => match scene.as_str() {
                "settings" | "launcher-settings" => {
                    self.show_notice_scene = false;
                    self.settings_page = Some("Launcher")
                }
                "versions" | "version-manager" => {
                    self.show_notice_scene = false;
                    self.settings_page = Some("Manage versions");
                }
                "accounts" | "account-manager" => {
                    self.show_notice_scene = false;
                    self.account_manager_view = AccountManagerView::Overview;
                    self.settings_page = Some("Manage accounts");
                }
                "notices" => {
                    self.show_notices_window = false;
                    self.notice_scene_index = 0;
                    self.show_notice_scene = true;
                }
                "main" | "login" => self.show_notice_scene = false,
                _ => self.set_logged_status(
                    LogLevel::Warn,
                    LogTarget::Launcher,
                    tr("rust.status.scene.unavailable").replace("%0", &scene),
                ),
            },
            notices::NoticeAction::Launcher {
                launcher,
                download_url,
                ..
            } => {
                if let Some(url) = download_url {
                    self.open_url_with_status(&url, LogTarget::Update);
                } else {
                    self.set_logged_status(
                        LogLevel::Warn,
                        LogTarget::Update,
                        tr("rust.status.launcher.version-missing").replace("%0", &launcher),
                    );
                }
            }
        }
    }
}
