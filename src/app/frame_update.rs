use super::*;

impl LauncherApp {
    pub(in crate::app) fn process_frame_events(&mut self, ctx: &Context) {
        if !self.launcher_log_initialized {
            self.launcher_log_initialized = true;
            record_launcher_log_line(&self.launcher_log, tr("rust.log.launcher-started"));
        }
        let connectivity_report = self
            .connectivity_events
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok());
        if let Some(report) = connectivity_report {
            self.connectivity_events = None;
            for service in &report.services {
                record_launcher_log_event(
                    &self.launcher_log,
                    if service.reachable {
                        LogLevel::Info
                    } else {
                        LogLevel::Warn
                    },
                    LogTarget::Connectivity,
                    &tr("rust.log.connectivity.check")
                        .replace("%0", service.id)
                        .replace(
                            "%1",
                            &tr(if service.reachable {
                                "rust.log.connectivity.reachable"
                            } else {
                                "rust.log.connectivity.unreachable"
                            }),
                        )
                        .replace(
                            "%2",
                            &if service.hosts.is_empty() {
                                String::new()
                            } else {
                                format!(" ({})", service.hosts.join(", "))
                            },
                        ),
                );
                for detail in &service.failure_details {
                    record_launcher_log_event(
                        &self.launcher_log,
                        LogLevel::Warn,
                        LogTarget::Connectivity,
                        &tr("rust.log.connectivity.failure")
                            .replace("%0", service.id)
                            .replace("%1", detail),
                    );
                }
            }
            self.connectivity_report = Some(report);
        }
        if let Some(maximized) = ctx.input(|input| input.viewport().maximized) {
            if self.launcher_window_state_seen && maximized != self.settings.launcher_maximized {
                self.settings.launcher_maximized = maximized;
                self.persist();
            }
            self.launcher_window_state_seen = true;
        }
        if ctx.input(|input| input.viewport().close_requested())
            && !self.settings.feedback_acknowledged
            && self.bootstrap_feedback_url.is_some()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.settings.feedback_acknowledged = true;
            self.persist();
            self.show_feedback_dialog = true;
        }
        if self.auto_launch_pending
            && !self.show_first_run_notice
            && !self.show_first_run_folder_notice
            && !self.show_arm_compat_notice
        {
            self.auto_launch_pending = false;
            let timeout = self.settings.auto_login_timeout_secs.clamp(2, 10);
            self.auto_launch_deadline =
                Some(Instant::now() + Duration::from_secs(u64::from(timeout)));
        }
        if let Some(deadline) = self.auto_launch_deadline {
            if !self.settings.auto_login || self.settings.selected_account.is_none() {
                self.auto_launch_deadline = None;
            } else if Instant::now() >= deadline {
                self.auto_launch_deadline = None;
                self.start_selected_game();
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
        if self.legacy_theme_palette_path != self.settings.legacy_theme_path {
            self.legacy_theme_palette = load_legacy_theme_palette(&self.settings.legacy_theme_path);
            self.legacy_theme_palette_path = self.settings.legacy_theme_path.clone();
        }
        let theme_preference = match self.settings.look_and_feel.as_str() {
            "off" | "system" | "auto" => egui::ThemePreference::System,
            "light" => egui::ThemePreference::Light,
            _ => egui::ThemePreference::Dark,
        };
        ctx.set_theme(theme_preference);
        let theme = ctx.theme();
        let dark_theme_assets = self
            .legacy_theme_palette
            .as_ref()
            .and_then(|palette| palette.dark_theme_enabled)
            .unwrap_or_else(|| {
                theme == egui::Theme::Dark
                    || self
                        .legacy_theme_palette
                        .as_ref()
                        .and_then(|palette| palette.background)
                        .is_some_and(legacy_background_prefers_dark_assets)
            });
        if self.background_theme_dark != Some(dark_theme_assets) {
            self.background_theme_dark = Some(dark_theme_assets);
            if !self.background_custom_loaded {
                if let Some(assets) = &mut self.assets {
                    assets.background = if dark_theme_assets {
                        assets.background_dark.clone()
                    } else {
                        assets.background_light.clone()
                    };
                }
            }
        }
        let mut style = (*ctx.style_of(theme)).clone();
        style.visuals = theme.default_visuals();
        if theme == egui::Theme::Dark {
            style.visuals.window_fill = Color32::from_rgb(39, 56, 64);
            style.visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(55, 68, 73);
            style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(73, 80, 83);
            style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(92, 104, 108);
            style.visuals.widgets.active.bg_fill = Color32::from_rgb(62, 111, 139);
            style.visuals.selection.bg_fill = Color32::from_rgb(62, 118, 153);
        }
        if let Some(palette) = &self.legacy_theme_palette {
            apply_legacy_theme_palette(&mut style, palette);
        }
        style.spacing.item_spacing = Vec2::new(0.0, 5.0);
        style.spacing.interact_size.y = 22.0;
        ctx.set_style_of(theme, style);

        let mut updates = Vec::new();
        let mut finished = false;
        if let Some(receiver) = &self.launch_events {
            while let Ok(event) = receiver.try_recv() {
                match event {
                    LaunchUiEvent::Finished => finished = true,
                    _ => updates.push(event),
                }
            }
        }
        for event in updates {
            match event {
                LaunchUiEvent::Message(message) => {
                    record_launcher_log_event(
                        &self.launcher_log,
                        message.level,
                        message.target,
                        &message.message,
                    );
                    if let Some(progress) = parse_launch_progress(&message.message) {
                        self.launch_progress = Some(progress);
                    }
                    self.status = message.message;
                }
                LaunchUiEvent::ErrorDialog(dialog) => {
                    self.launch_error_dialog = Some(dialog);
                }
                LaunchUiEvent::LongMalwareScanConfirmation(reply) => {
                    self.malware_scan_confirmation = Some(reply);
                }
                LaunchUiEvent::MalwareDetected(detection) => {
                    self.settings.malware_scan_version = 0;
                    self.persist();
                    self.status = tr("jarscanner.detected").to_owned();
                    self.malware_detection = Some(detection);
                }
                LaunchUiEvent::GameStarted => {
                    let message = tr("rust.status.game.starting").to_owned();
                    record_launcher_log_event(
                        &self.launcher_log,
                        LogLevel::Info,
                        LogTarget::Minecraft,
                        &message,
                    );
                    self.game_running = true;
                    self.status = message;
                    self.launch_progress = None;
                    match self.settings.launch_action.as_str() {
                        "hide" => {
                            self.hidden_for_game = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                        }
                        "exit" => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                        _ => {}
                    }
                }
                LaunchUiEvent::Finished => {}
            }
        }
        if finished {
            self.launching = false;
            self.game_running = false;
            self.launch_progress = None;
            self.launch_events = None;
            if self.hidden_for_game {
                self.hidden_for_game = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            }
            let pending_report = self
                .pending_crash_report
                .lock()
                .ok()
                .and_then(|mut pending| pending.take());
            if let Some(report) = pending_report {
                self.set_logged_status(LogLevel::Error, LogTarget::Crash, report.summary.clone());
                self.crash_report = Some(report);
            }
        }

        let mut catalog_update = None;
        if let Some(receiver) = &self.version_catalog_events {
            if let Ok(result) = receiver.try_recv() {
                catalog_update = Some(result);
            }
        }
        if let Some(result) = catalog_update {
            self.version_catalog_events = None;
            self.version_catalog_cancel = None;
            self.version_catalog_loading = false;
            match result {
                Ok(manifest) => {
                    let version_count = manifest.versions.len();
                    let first_update = !self.version_alert_has_refreshed;
                    self.version_alert_has_refreshed = true;
                    if let Some(alert) =
                        refresh_version_alert_index(&manifest, &self.settings, first_update)
                    {
                        self.version_update_alert = Some(alert);
                    }
                    self.version_manifest = Some(manifest);
                    self.set_logged_status(
                        LogLevel::Info,
                        LogTarget::VersionCatalog,
                        tr("rust.status.catalog.loaded").replace("%0", &version_count.to_string()),
                    );
                }
                Err(error) => {
                    self.set_logged_status(LogLevel::Error, LogTarget::VersionCatalog, error);
                }
            }
        }
        let mut update_check_result = None;
        if let Some(receiver) = &self.update_check_events {
            if let Ok(result) = receiver.try_recv() {
                update_check_result = Some(result);
            }
        }
        if let Some(result) = update_check_result {
            self.update_check_events = None;
            self.update_checking = false;
            match result {
                Ok(github_update::CheckResult::NoPublishedRelease) => {
                    self.set_logged_status(
                        LogLevel::Info,
                        LogTarget::Update,
                        tr("update.check.no-release"),
                    );
                }
                Ok(github_update::CheckResult::UpToDate) => {
                    self.set_logged_status(
                        LogLevel::Info,
                        LogTarget::Update,
                        tr("update.check.current").replace("%0", env!("CARGO_PKG_VERSION")),
                    );
                }
                Ok(github_update::CheckResult::UpdateAvailable(release)) => {
                    self.set_logged_status(
                        LogLevel::Info,
                        LogTarget::Update,
                        format!("{} {}", tr("update.check.available"), release.tag_name),
                    );
                    self.available_release = Some(release);
                    self.update_prompt_pending = true;
                }
                Err(error) => {
                    self.set_logged_status(
                        LogLevel::Error,
                        LogTarget::Update,
                        tr("update.check.failed").replace("%0", &error.to_string()),
                    );
                }
            }
        }
        let mut update_install_result = None;
        if let Some(receiver) = &self.update_install_events {
            if let Ok(result) = receiver.try_recv() {
                update_install_result = Some(result);
            }
        }
        if let Some(result) = update_install_result {
            self.update_install_events = None;
            self.update_installing = false;
            match result {
                Ok(()) => {
                    self.set_logged_status(
                        LogLevel::Info,
                        LogTarget::Update,
                        tr("update.installing"),
                    );
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Err(error) => {
                    self.update_install_error = Some(error.clone());
                    self.set_logged_status(LogLevel::Error, LogTarget::Update, error);
                }
            }
        }
        if self.update_prompt_pending
            && !self.show_first_run_notice
            && !self.show_first_run_folder_notice
            && !self.show_arm_compat_notice
        {
            self.update_prompt_pending = false;
            self.show_update_dialog = true;
        }
        let mut bootstrap_update = None;
        if let Some(receiver) = &self.bootstrap_events {
            if let Ok(result) = receiver.try_recv() {
                bootstrap_update = Some(result);
            }
        }
        if let Some(result) = bootstrap_update {
            self.bootstrap_events = None;
            self.bootstrap_loading = false;
            match result {
                Ok(configuration) => {
                    let notices_were_enabled = self.settings.notices_enabled;
                    for notice in &configuration.notices {
                        if let Some(error) = &notice.image_load_error {
                            record_launcher_log_event(
                                &self.launcher_log,
                                LogLevel::Warn,
                                LogTarget::Bootstrap,
                                &tr("rust.log.bootstrap.image-failed")
                                    .replace("%0", &notice.id)
                                    .replace("%1", error),
                            );
                        }
                    }
                    if let Some(notification) = &configuration.notification
                        && let Some(error) = &notification.image_load_error
                    {
                        record_launcher_log_event(
                            &self.launcher_log,
                            LogLevel::Warn,
                            LogTarget::Bootstrap,
                            &tr("rust.log.bootstrap.image-failed")
                                .replace("%0", &notification.id)
                                .replace("%1", error),
                        );
                    }
                    minecraft_java_rs_core::net::client::set_repository_overrides(
                        configuration.repositories.clone(),
                    );
                    spawn_crash_signature_fetch(
                        configuration.extra_version_repositories.clone(),
                        Arc::clone(&self.launcher_log),
                    );
                    self.notices_disable_allowed =
                        configuration.allows_notice_disable(&self.settings.client_id);
                    self.allow_ely_everywhere = configuration.allow_ely_everywhere;
                    if !configuration.extra_version_repositories.is_empty()
                        && self.extra_version_repositories
                            != configuration.extra_version_repositories
                    {
                        self.extra_version_repositories =
                            configuration.extra_version_repositories.clone();
                        self.start_version_catalog_fetch();
                    }
                    if !self.notices_disable_allowed {
                        self.settings.notices_enabled = true;
                        if !notices_were_enabled {
                            self.persist();
                        }
                    }
                    self.bootstrap_notice_list = configuration.notices;
                    self.bootstrap_promoted_servers = configuration.promoted_servers;
                    self.bootstrap_feedback_url = configuration.feedback_url;
                    self.bootstrap_notification_texture = configuration
                        .notification
                        .as_ref()
                        .and_then(|notification| notification.image.as_ref())
                        .and_then(|bytes| {
                            load_optional_texture(ctx, "bootstrap_notification", bytes)
                        });
                    self.bootstrap_notification = configuration.notification;
                    self.bootstrap_error = None;
                    self.rebuild_notice_list(ctx);
                    if !notices_were_enabled && self.settings.notices_enabled {
                        self.refresh_notices();
                    }
                    record_launcher_log_event(
                        &self.launcher_log,
                        LogLevel::Info,
                        LogTarget::Bootstrap,
                        &tr("rust.log.bootstrap.loaded"),
                    );
                }
                Err(error) => {
                    let notices_were_enabled = self.settings.notices_enabled;
                    self.notices_disable_allowed = false;
                    self.allow_ely_everywhere = false;
                    self.settings.notices_enabled = true;
                    if !notices_were_enabled {
                        self.persist();
                        self.refresh_notices();
                    }
                    record_launcher_log_event(
                        &self.launcher_log,
                        LogLevel::Warn,
                        LogTarget::Bootstrap,
                        &error,
                    );
                    self.bootstrap_error = Some(error);
                }
            }
        }

        let mut notice_update = None;
        if let Some(receiver) = &self.notice_events {
            if let Ok(result) = receiver.try_recv() {
                notice_update = Some(result);
            }
        }
        if let Some(result) = notice_update {
            self.notice_events = None;
            self.notices_loading = false;
            match result {
                Ok(notices) => {
                    record_launcher_log_event(
                        &self.launcher_log,
                        LogLevel::Info,
                        LogTarget::Notices,
                        &tr("rust.log.notices.loaded").replace("%0", &notices.len().to_string()),
                    );
                    for notice in &notices {
                        if let Some(error) = &notice.image_load_error {
                            record_launcher_log_event(
                                &self.launcher_log,
                                LogLevel::Warn,
                                LogTarget::Notices,
                                &tr("rust.log.notices.image-failed")
                                    .replace("%0", &notice.id)
                                    .replace("%1", error),
                            );
                        }
                    }
                    self.personal_notice_list = notices;
                    if !self.settings.notices_enabled {
                        self.personal_notice_list.clear();
                    }
                    self.notice_error = None;
                }
                Err(error) => {
                    record_launcher_log_event(
                        &self.launcher_log,
                        LogLevel::Warn,
                        LogTarget::Notices,
                        &error,
                    );
                    self.notice_error = Some(error);
                    self.personal_notice_list.clear();
                }
            }
            self.rebuild_notice_list(ctx);
        }
        let mut diagnostic_update = None;
        if let Some(receiver) = &self.diagnostic_events {
            if let Ok(result) = receiver.try_recv() {
                diagnostic_update = Some(result);
            }
        }
        if let Some(result) = diagnostic_update {
            self.diagnostic_events = None;
            self.diagnostic_pending = false;
            match result {
                Ok(url) => {
                    record_launcher_log_event(
                        &self.launcher_log,
                        LogLevel::Info,
                        LogTarget::Diagnostics,
                        &tr("rust.log.diagnostics.uploaded"),
                    );
                    self.diagnostic_result_url = Some(url);
                    self.diagnostic_error = None;
                }
                Err(error) => {
                    record_launcher_log_event(
                        &self.launcher_log,
                        LogLevel::Error,
                        LogTarget::Diagnostics,
                        &error,
                    );
                    self.diagnostic_error = Some(error);
                }
            }
        }
        let mut microsoft_signin_update = None;
        if let Some(receiver) = &self.microsoft_signin_events {
            if let Ok(result) = receiver.try_recv() {
                microsoft_signin_update = Some(result);
            }
        }
        if let Some(result) = microsoft_signin_update {
            self.apply_microsoft_sign_in(result);
        }
        let mut ely_legacy_signin_update = None;
        if let Some(receiver) = &self.ely_legacy_signin_events {
            if let Ok(result) = receiver.try_recv() {
                ely_legacy_signin_update = Some(result);
            }
        }
        if let Some(result) = ely_legacy_signin_update {
            self.save_ely_legacy_account(result);
        }
        let mut ely_signin_update = None;
        if let Some(receiver) = &self.ely_signin_events {
            if let Ok(result) = receiver.try_recv() {
                ely_signin_update = Some(result);
            }
        }
        if let Some(result) = ely_signin_update {
            self.save_ely_account(result);
        }
        let mut profile_creation_update = None;
        if let Some(receiver) = &self.profile_creation_events {
            if let Ok(result) = receiver.try_recv() {
                profile_creation_update = Some(result);
            }
        }
        if let Some(result) = profile_creation_update {
            self.profile_creation_events = None;
            self.profile_creation_pending = false;
            match result {
                Ok(account) => self.save_microsoft_account(account),
                Err(error) => {
                    self.set_logged_status(LogLevel::Error, LogTarget::Authentication, error)
                }
            }
        }
        if self.launching {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
        if self.version_catalog_loading
            || self.update_checking
            || self.update_installing
            || self.notices_loading
            || self.microsoft_signin_pending
            || self.ely_signin_pending
            || self.ely_legacy_signin_pending
            || self.profile_creation_pending
        {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
    }
}
