use super::*;

impl LauncherApp {
    pub(in crate::app) fn start_version_install(&mut self, version: String, force_update: bool) {
        if self.launching {
            return;
        }
        self.settings.selected_version = version.clone();
        self.persist();

        if !valid_version_id(&version) {
            self.set_logged_status(
                LogLevel::Warn,
                LogTarget::VersionManager,
                tr("rust.status.versions.invalid-id"),
            );
            return;
        }
        let game_dir = self.game_root_for_version(&version);
        let preferences = self.settings.clone();
        let (update_tx, update_rx) = mpsc::channel();
        self.launch_events = Some(update_rx);
        self.launching = true;
        self.launch_progress = Some(0.0);
        self.cancel_requested.store(false, Ordering::Release);
        let cancel_requested = self.cancel_requested.clone();
        self.set_logged_status(
            LogLevel::Info,
            LogTarget::VersionManager,
            tr(if force_update {
                "rust.status.versions.force-updating"
            } else {
                "rust.status.game.installing"
            })
            .replace("%0", &version),
        );

        thread::spawn(move || {
            let done_tx = update_tx.clone();
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let details = error.to_string();
                    send_launch_message(
                        &update_tx,
                        LogLevel::Error,
                        LogTarget::Launcher,
                        tr("rust.status.runtime.start-error").replace("%0", &details),
                    );
                    send_launch_error_dialog(
                        &update_tx,
                        tr("launcher.error.title"),
                        tr("launcher.error.unknown"),
                        Some(details),
                    );
                    let _ = done_tx.send(LaunchUiEvent::Finished);
                    return;
                }
            };

            runtime.block_on(async move {
                let options = make_launch_options(
                    version.clone(),
                    "VersionManager".to_owned(),
                    game_dir,
                    force_update,
                    preferences,
                );
                let mut launcher = Launcher::new(options);
                let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(256);
                let progress_tx = update_tx.clone();
                let listener = tokio::spawn(async move {
                    while let Some(event) = event_rx.recv().await {
                        if let Some(message) = launch_event_ui_message(event) {
                            let _ = progress_tx.send(LaunchUiEvent::Message(message));
                        }
                    }
                });

                let install_result = tokio::select! {
                    result = launcher.download_game(event_tx.clone()) => Some(result),
                    _ = wait_for_cancel(cancel_requested) => None,
                };
                match install_result {
                    Some(Ok(())) => {
                        send_launch_message(
                            &update_tx,
                            LogLevel::Info,
                            LogTarget::VersionManager,
                            tr("rust.status.versions.installed").replace("%0", &version),
                        );
                    }
                    Some(Err(error)) => {
                        let message = tr("rust.status.versions.install-failed")
                            .replace("%0", &error.to_string());
                        send_launch_message(
                            &update_tx,
                            LogLevel::Error,
                            LogTarget::VersionManager,
                            message.clone(),
                        );
                        send_launch_error_dialog(
                            &update_tx,
                            tr("launcher.error.title"),
                            message,
                            None,
                        );
                    }
                    None => {
                        send_launch_message(
                            &update_tx,
                            LogLevel::Warn,
                            LogTarget::VersionManager,
                            tr("rust.status.versions.install-cancelled"),
                        );
                    }
                }
                drop(event_tx);
                let _ = listener.await;
            });

            let _ = done_tx.send(LaunchUiEvent::Finished);
        });
    }

    pub(in crate::app) fn show_version_manager(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(tr("rust.ui.versions.heading"));
            if self.version_catalog_loading {
                if ui.button(tr("rust.ui.versions.cancel-refresh")).clicked() {
                    self.cancel_version_catalog_fetch();
                }
            } else {
                let mut refresh_action = None;
                ui.menu_button(tr("rust.ui.versions.refresh-catalog"), |ui| {
                    if ui
                        .button(tr("version.manager.refresher.menu.local"))
                        .clicked()
                    {
                        refresh_action = Some(false);
                        ui.close();
                    }
                    if ui
                        .button(tr("version.manager.refresher.menu.remote"))
                        .clicked()
                    {
                        refresh_action = Some(true);
                        ui.close();
                    }
                });
                match refresh_action {
                    Some(true) => self.refresh_version_catalog(),
                    Some(false) => {
                        let count = self.installed_versions().len();
                        self.set_logged_status(
                            LogLevel::Info,
                            LogTarget::VersionCatalog,
                            tr("rust.status.versions.refreshed-local")
                                .replace("%0", &count.to_string()),
                        );
                    }
                    None => {}
                }
            }
            if self.version_catalog_loading {
                ui.spinner();
            }
        });

        ui.horizontal(|ui| {
            ui.label(tr("rust.ui.search"));
            ui.text_edit_singleline(&mut self.version_search);
        });

        let mut filter_settings_changed = false;
        ui.horizontal_wrapped(|ui| {
            for (kind, label_key) in [
                ("snapshot", "settings.versions.snapshot"),
                ("modified", "settings.versions.modified"),
                ("old-beta", "settings.versions.old_beta"),
                ("old-alpha", "settings.versions.old_alpha"),
                ("launcher", "settings.versions.launcher"),
                ("pending", "settings.versions.pending"),
            ] {
                let enabled = self
                    .settings
                    .version_catalog_types
                    .entry(kind.to_owned())
                    .or_insert_with(|| default_version_type_enabled(kind));
                filter_settings_changed |= ui.checkbox(enabled, tr(label_key)).changed();
            }
        });
        ui.horizontal(|ui| {
            filter_settings_changed |= ui
                .checkbox(
                    &mut self.settings.version_only_installed,
                    tr("settings.versions.only-installed"),
                )
                .changed();
            filter_settings_changed |= ui
                .checkbox(
                    &mut self.settings.version_include_remote,
                    tr("rust.ui.versions.filter.remote"),
                )
                .changed();
            filter_settings_changed |= ui
                .checkbox(
                    &mut self.settings.version_show_old_releases,
                    tr("settings.versions.sub.old_release"),
                )
                .changed();
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("settings.alert-on.label"));
            for (kind, label_key) in [
                ("release", "settings.alert-on.release"),
                ("snapshot", "settings.alert-on.snapshot"),
                ("modified", "settings.alert-on.modified"),
            ] {
                let enabled = self
                    .settings
                    .version_alert_types
                    .entry(kind.to_owned())
                    .or_insert(false);
                filter_settings_changed |= ui.checkbox(enabled, tr(label_key)).changed();
            }
        });
        if filter_settings_changed {
            self.persist();
        }

        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.version_kind_filter,
                "all".to_owned(),
                tr("rust.ui.versions.filter.all"),
            );
            ui.selectable_value(
                &mut self.version_kind_filter,
                "release".to_owned(),
                tr("version.release"),
            );
            ui.selectable_value(
                &mut self.version_kind_filter,
                "snapshot".to_owned(),
                tr("settings.versions.snapshot"),
            );
            ui.selectable_value(
                &mut self.version_kind_filter,
                "modified".to_owned(),
                tr("settings.versions.modified"),
            );
            ui.selectable_value(
                &mut self.version_kind_filter,
                "old".to_owned(),
                tr("rust.ui.versions.filter.old"),
            );
            ui.selectable_value(
                &mut self.version_kind_filter,
                "launcher".to_owned(),
                tr("settings.versions.launcher"),
            );
            ui.selectable_value(
                &mut self.version_kind_filter,
                "pending".to_owned(),
                tr("settings.versions.pending"),
            );
        });

        if self.version_manifest.is_none() && !self.version_catalog_loading {
            ui.colored_label(
                Color32::from_rgb(238, 183, 85),
                tr("rust.ui.versions.catalog-fallback"),
            );
        }

        let entries = self.visible_catalog_entries();
        let installed: HashSet<String> = self.installed_versions().into_iter().collect();
        let busy = self.launching;
        let mut install_action = None;
        egui::ScrollArea::vertical()
            .max_height(330.0)
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                if entries.is_empty() {
                    ui.label(tr("rust.ui.versions.empty"));
                }
                for (version, kind) in entries {
                    let is_installed = installed.contains(&version);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&version).strong());
                        ui.label(RichText::new(&kind).weak());
                        if is_installed {
                            ui.label(
                                RichText::new(tr("rust.ui.versions.installed"))
                                    .color(Color32::LIGHT_GREEN),
                            );
                        }
                        if ui.small_button(tr("rust.ui.select")).clicked() {
                            self.settings.selected_version = version.clone();
                            self.persist();
                        }
                        let install_label = tr(if is_installed {
                            "rust.ui.versions.repair"
                        } else {
                            "rust.ui.versions.install"
                        });
                        ui.add_enabled_ui(!busy, |ui| {
                            ui.menu_button(install_label, |ui| {
                                if ui
                                    .button(tr("version.manager.downloader.menu.ordinary"))
                                    .clicked()
                                {
                                    install_action = Some((version.clone(), false));
                                    ui.close();
                                }
                                if ui
                                    .button(tr("version.manager.downloader.menu.force"))
                                    .clicked()
                                {
                                    install_action = Some((version.clone(), true));
                                    ui.close();
                                }
                            });
                        });
                        if is_installed
                            && ui
                                .add_enabled(!busy, egui::Button::new(tr("rust.ui.remove")).small())
                                .clicked()
                        {
                            self.confirm_remove_version = Some(version.clone());
                        }
                    });
                }
            });
        if let Some((version, force_update)) = install_action {
            self.request_version_install(version, force_update);
        }
    }
}
