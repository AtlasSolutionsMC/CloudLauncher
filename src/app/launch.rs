use super::*;

impl LauncherApp {
    pub(in crate::app) fn start_selected_game(&mut self) {
        self.start_selected_game_on_server(None);
    }

    pub(in crate::app) fn start_selected_game_on_server(&mut self, server: Option<SavedServer>) {
        if self.launching {
            return;
        }
        if server
            .as_ref()
            .is_some_and(|server| parse_server_address(&server.address).is_none())
        {
            self.set_logged_status(
                LogLevel::Warn,
                LogTarget::Servers,
                tr("rust.status.servers.invalid-address"),
            );
            return;
        }
        let Some(selected_id) = self.settings.selected_account.clone() else {
            self.open_account_dialog();
            return;
        };
        let microsoft_account = self
            .settings
            .microsoft_accounts
            .iter()
            .find(|account| account.id == selected_id)
            .cloned();
        let ely_legacy_account = self
            .settings
            .ely_legacy_accounts
            .iter()
            .find(|account| account.id == selected_id)
            .cloned();
        let ely_account = self
            .settings
            .ely_accounts
            .iter()
            .find(|account| account.id == selected_id)
            .cloned();
        let offline_username = if microsoft_account.is_none()
            && ely_account.is_none()
            && ely_legacy_account.is_none()
            && self
                .settings
                .accounts
                .iter()
                .any(|name| name == &selected_id)
        {
            Some(selected_id)
        } else {
            None
        };
        let offline_uuid = offline_username
            .as_deref()
            .and_then(|username| offline_account_uuid_for(&self.settings, username));
        if microsoft_account.is_none()
            && ely_account.is_none()
            && ely_legacy_account.is_none()
            && offline_username.is_none()
        {
            self.settings.selected_account = None;
            self.open_account_dialog();
            return;
        }

        let library_replace_type = if microsoft_account.is_some() {
            Some("minecraft".to_owned())
        } else if ely_account.is_some() || ely_legacy_account.is_some() {
            Some("ely".to_owned())
        } else {
            offline_username.as_deref().map(|username| {
                if self.allow_ely_everywhere && offline_ely_skins_for(&self.settings, username) {
                    "ely".to_owned()
                } else {
                    "plain".to_owned()
                }
            })
        };
        let mut library_replace_types = Vec::new();
        if self.settings.delete_patchy {
            library_replace_types.push("patchy".to_owned());
        }
        if let Some(replacement_type) = library_replace_type {
            library_replace_types.push(replacement_type);
        }
        if self.settings.legacy_experiments.contains("updated_lwjgl") {
            library_replace_types.push("experiment-lwjgl-update".to_owned());
        }
        if self.settings.legacy_experiments.contains("updated_jna") {
            library_replace_types.push("experiment-jna-update".to_owned());
        }

        let version = self.settings.selected_version.clone();
        if !valid_version_id(&version) {
            self.set_logged_status(
                LogLevel::Warn,
                LogTarget::VersionManager,
                tr("rust.status.versions.invalid-id"),
            );
            return;
        }
        let force_update = self.settings.force_update;
        let skip_outdated_prompt =
            self.version_update_prompt_bypassed.take().as_deref() == Some(version.as_str());
        if !force_update
            && !skip_outdated_prompt
            && self.version_is_installed()
            && self.installed_version_is_outdated(&version)
        {
            self.outdated_version_confirmation =
                Some(PendingVersionUpdateConfirmation { version, server });
            return;
        }
        if force_update {
            let has_remote_version = self
                .version_manifest
                .as_ref()
                .is_some_and(|manifest| manifest.versions.iter().any(|entry| entry.id == version));
            if !has_remote_version {
                let title = tr("forceupdate.local.title").to_owned();
                let message = tr("forceupdate.local").to_owned();
                self.set_logged_status(LogLevel::Error, LogTarget::VersionManager, message.clone());
                self.launch_error_dialog = Some(LaunchErrorDialog {
                    title,
                    message,
                    details: None,
                });
                return;
            }

            let confirmation_accepted =
                self.force_update_confirmed_version.take().as_deref() == Some(version.as_str());
            if self.version_is_installed() && !confirmation_accepted {
                self.force_update_confirmation =
                    Some(PendingVersionUpdateConfirmation { version, server });
                return;
            }
        }
        let game_dir = self.game_root_for_version(&version);
        let promoted_servers = if self.settings.promoted_servers_in_game {
            self.promoted_servers_for_version(&version)
        } else {
            Vec::new()
        };
        let server_save_warning = match prepare_minecraft_server_list(
            &game_dir.join("servers.dat"),
            server.as_ref(),
            &promoted_servers,
            self.settings.promoted_servers_in_game,
        ) {
            Ok(()) => {
                if let Ok(servers) = read_minecraft_server_list(&game_dir.join("servers.dat")) {
                    self.settings.servers = servers.clone();
                    self.settings.servers_cache_root = game_dir.to_string_lossy().into_owned();
                    self.persist();
                }
                None
            }
            Err(error) => {
                Some(tr("rust.status.game.server-save-warning").replace("%0", &error.to_string()))
            }
        };
        let quick_play_supported = server
            .as_ref()
            .map(|_| selected_version_supports_quick_play(&version, &game_dir));
        let quick_play_server = server;
        let mods_directory = game_dir.join("mods");
        let should_scan_malware = self.settings.malware_scan_version < 2 && mods_directory.is_dir();
        if should_scan_malware {
            // Record the scan version first, matching Java's fail-open behavior.
            self.settings.malware_scan_version = 2;
            self.persist();
        }
        let crash_game_dir = self.game_data_directory_for_version(&version);
        let preferences = self.settings.clone();
        let show_full_game_command = preferences.show_full_game_command;
        let crash_detection_enabled = preferences.crash_detection_enabled;
        let crash_version = version.clone();
        let crash_loader = format!("{}-{}", preferences.loader_type, preferences.loader_build);
        let version_manifest = self.version_manifest.clone();
        let extra_version_repositories = self.extra_version_repositories.clone();
        let memory_release_time = self.version_release_time(&version);
        let game_log = self.game_log.clone();
        let launcher_log = self.launcher_log.clone();
        if let Ok(mut lines) = game_log.lock() {
            lines.clear();
        }
        let pending_crash_report = self.pending_crash_report.clone();
        if let Ok(mut report) = pending_crash_report.lock() {
            *report = None;
        }
        let launch_started = SystemTime::now();
        let (update_tx, update_rx) = mpsc::channel();
        self.launch_events = Some(update_rx);
        self.launching = true;
        if self.settings.logger_mode == "global" {
            self.show_log_window = true;
            self.show_live_launcher_log = true;
            self.show_live_game_log = false;
        }
        self.launch_progress = Some(0.0);
        self.cancel_requested.store(false, Ordering::Release);
        let cancel_requested = self.cancel_requested.clone();
        self.set_logged_status(
            LogLevel::Info,
            LogTarget::Minecraft,
            tr("rust.status.game.preparing").replace("%0", &version),
        );

        thread::spawn(move || {
            if let Some(warning) = server_save_warning {
                send_launch_message(&update_tx, LogLevel::Warn, LogTarget::Servers, warning);
            }
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
                let authenticator = if let Some(account) = microsoft_account {
                    tokio::select! {
                        result = authenticate_microsoft_account(&account) => match result {
                            Ok(authenticator) => authenticator,
                            Err(error) => {
                                send_launch_error_dialog(
                                    &update_tx,
                                    tr("account.manager.error.title"),
                                    error.clone(),
                                    None,
                                );
                                send_launch_message(
                                    &update_tx,
                                    LogLevel::Error,
                                    LogTarget::Authentication,
                                    error,
                                );
                                return;
                            }
                        },
                        _ = wait_for_cancel(cancel_requested.clone()) => {
                            send_launch_message(
                                &update_tx,
                                LogLevel::Warn,
                                LogTarget::Authentication,
                                tr("rust.status.account.signin-cancelled"),
                            );
                            return;
                        }
                    }
                } else if let Some(account) = ely_legacy_account {
                    tokio::select! {
                        result = authenticate_ely_legacy_account(&account) => match result {
                            Ok(authenticator) => authenticator,
                            Err(error) => {
                                send_launch_error_dialog(
                                    &update_tx,
                                    tr("account.manager.error.title"),
                                    error.clone(),
                                    None,
                                );
                                send_launch_message(
                                    &update_tx,
                                    LogLevel::Error,
                                    LogTarget::Authentication,
                                    error,
                                );
                                return;
                            }
                        },
                        _ = wait_for_cancel(cancel_requested.clone()) => {
                            send_launch_message(
                                &update_tx,
                                LogLevel::Warn,
                                LogTarget::Authentication,
                                tr("rust.status.account.ely.signin-cancelled"),
                            );
                            return;
                        }
                    }
                } else if let Some(account) = ely_account {
                    tokio::select! {
                        result = authenticate_ely_account(&account) => match result {
                            Ok(authenticator) => authenticator,
                            Err(error) => {
                                send_launch_error_dialog(
                                    &update_tx,
                                    tr("account.manager.error.title"),
                                    error.clone(),
                                    None,
                                );
                                send_launch_message(
                                    &update_tx,
                                    LogLevel::Error,
                                    LogTarget::Authentication,
                                    error,
                                );
                                return;
                            }
                        },
                        _ = wait_for_cancel(cancel_requested.clone()) => {
                            send_launch_message(
                                &update_tx,
                                LogLevel::Warn,
                                LogTarget::Authentication,
                                tr("rust.status.account.ely.signin-cancelled"),
                            );
                            return;
                        }
                    }
                } else {
                    offline_authenticator_with_uuid(
                        offline_username.unwrap_or_default(),
                        offline_uuid.as_deref(),
                    )
                };
                let mut preferences = preferences;
                let java_mode = preferences.java_mode.clone();
                match java_mode.as_str() {
                    "recommended" => preferences.java_path.clear(),
                    "current" => match tokio::task::spawn_blocking(detect_java).await {
                        Ok(Ok((path, version))) => {
                            preferences.java_path = path.to_string_lossy().into_owned();
                            send_launch_message(
                                &update_tx,
                                LogLevel::Info,
                                LogTarget::JavaRuntime,
                                tr("rust.status.java.current").replace("%0", &version),
                            );
                        }
                        Ok(Err(error)) => {
                            send_launch_error_dialog(
                                &update_tx,
                                tr("launcher.error.title"),
                                error.clone(),
                                None,
                            );
                            send_launch_message(
                                &update_tx,
                                LogLevel::Error,
                                LogTarget::JavaRuntime,
                                error,
                            );
                            return;
                        }
                        Err(error) => {
                            let message = tr("rust.status.java.detection-error")
                                .replace("%0", &error.to_string());
                            send_launch_error_dialog(
                                &update_tx,
                                tr("launcher.error.title"),
                                message.clone(),
                                None,
                            );
                            send_launch_message(
                                &update_tx,
                                LogLevel::Error,
                                LogTarget::JavaRuntime,
                                message,
                            );
                            return;
                        }
                    },
                    "custom" if preferences.java_path.trim().is_empty() => {
                        let message = tr("rust.status.java.custom-required").to_owned();
                        send_launch_message(
                            &update_tx,
                            LogLevel::Warn,
                            LogTarget::JavaRuntime,
                            message.clone(),
                        );
                        send_launch_error_dialog(
                            &update_tx,
                            tr("launcher.error.title"),
                            message,
                            None,
                        );
                        return;
                    }
                    _ => {}
                }
                if java_mode != "recommended" {
                    let java_path = PathBuf::from(preferences.java_path.trim());
                    let check_result = tokio::select! {
                        result = verify_java_compatibility(
                            &java_path,
                            &game_dir,
                            &version,
                            version_manifest.as_ref(),
                            &extra_version_repositories,
                        ) => {
                            Some(result)
                        }
                        _ = wait_for_cancel(cancel_requested.clone()) => None,
                    };
                    let Some(check_result) = check_result else {
                        send_launch_message(
                            &update_tx,
                            LogLevel::Warn,
                            LogTarget::JavaRuntime,
                            tr("rust.status.java.check-cancelled"),
                        );
                        return;
                    };
                    match check_result {
                        Ok(message) => {
                            send_launch_message(
                                &update_tx,
                                LogLevel::Info,
                                LogTarget::JavaRuntime,
                                message,
                            );
                        }
                        Err(error) => {
                            send_launch_error_dialog(
                                &update_tx,
                                tr("launcher.error.title"),
                                error.clone(),
                                None,
                            );
                            send_launch_message(
                                &update_tx,
                                LogLevel::Error,
                                LogTarget::JavaRuntime,
                                error,
                            );
                            return;
                        }
                    }
                }

                let experimental_java_major = if preferences
                    .legacy_experiments
                    .contains("zgc_generational")
                    && preferences.optimized_java_args == "zgc"
                {
                    selected_java_major_for_experiments(
                        &game_dir,
                        &version,
                        &preferences,
                        version_manifest.as_ref(),
                        &extra_version_repositories,
                    )
                    .await
                } else {
                    None
                };

                let gpu_preference = preferences.gpu_preference.clone();
                let gpu_preference_enabled = preferences.windows_gpu_performance;
                let remove_undesirable_mods = preferences.remove_undesirable_mods;
                let replacement_instance_version = version.clone();
                let legacy_experiments = preferences.legacy_experiments.clone();
                let mut options = make_launch_options_with_authenticator(
                    version,
                    authenticator,
                    game_dir,
                    force_update,
                    preferences,
                    memory_release_time.as_deref(),
                );
                if let Some(warning) = append_legacy_experiment_arguments(
                    &mut options,
                    &legacy_experiments,
                    experimental_java_major,
                ) {
                    send_launch_message(
                        &update_tx,
                        LogLevel::Warn,
                        LogTarget::JavaRuntime,
                        warning,
                    );
                }
                if !library_replace_types.is_empty() {
                    options.library_replace_types = library_replace_types.clone();
                    options.skip_bundle_check = false;
                    let replacement_profile = library_replace_types.join("-");
                    options.instance = Some(match options.instance.take() {
                        Some(instance) => {
                            format!("{instance}-replace-{replacement_profile}-{replacement_instance_version}")
                        }
                        None => format!("legacylauncher-replace-{replacement_profile}-{replacement_instance_version}"),
                    });
                }
                if let (Some(server), Some(supported)) = (quick_play_server, quick_play_supported) {
                    append_server_launch_arguments(&mut options, &server.address, supported);
                    send_launch_message(
                        &update_tx,
                        LogLevel::Info,
                        LogTarget::Minecraft,
                        tr("rust.status.game.connecting").replace("%0", &server.name),
                    );
                }

                if should_scan_malware {
                    match scan_mods_before_launch(
                        &mods_directory,
                        cancel_requested.clone(),
                        &update_tx,
                    )
                    .await
                    {
                        MalwareScanOutcome::Cancelled => {
                            send_launch_message(
                                &update_tx,
                                LogLevel::Warn,
                                LogTarget::Mods,
                                tr("rust.status.game.launch-cancelled"),
                            );
                            return;
                        }
                        MalwareScanOutcome::Infected(detection) => {
                            send_launch_message(
                                &update_tx,
                                LogLevel::Error,
                                LogTarget::Mods,
                                format!(
                                    "Fractureiser signature detected in {}: {}",
                                    detection.jar.display(),
                                    detection.entry
                                ),
                            );
                            let _ = update_tx.send(LaunchUiEvent::MalwareDetected(detection));
                            return;
                        }
                        MalwareScanOutcome::Clean | MalwareScanOutcome::Skipped => {}
                    }
                }

                let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(256);
                let progress_tx = update_tx.clone();
                let listener_game_log = game_log.clone();
                let listener_launcher_log = launcher_log.clone();
                let listener = tokio::spawn(async move {
                    while let Some(event) = event_rx.recv().await {
                        let message = launch_event_message_or_record(
                            &listener_game_log,
                            &listener_launcher_log,
                            event,
                            show_full_game_command,
                        );
                        if let Some(message) = message {
                            let _ = progress_tx.send(message);
                        }
                    }
                });

                let mut launcher = Launcher::new(options);
                let download_result = tokio::select! {
                    result = launcher.download_game(event_tx.clone()) => Some(result),
                    _ = wait_for_cancel(cancel_requested.clone()) => None,
                };
                match download_result {
                    None => {
                        send_launch_message(
                            &update_tx,
                            LogLevel::Warn,
                            LogTarget::Minecraft,
                            tr("rust.status.game.launch-cancelled"),
                        );
                        drop(event_tx);
                        let _ = listener.await;
                        return;
                    }
                    Some(Err(error)) => {
                        let message = tr("rust.status.game.launch-error")
                            .replace("%0", &error.to_string());
                        send_launch_message(
                            &update_tx,
                            LogLevel::Error,
                            LogTarget::Minecraft,
                            message.clone(),
                        );
                        send_launch_error_dialog(
                            &update_tx,
                            tr("launcher.error.title"),
                            message,
                            None,
                        );
                        drop(event_tx);
                        let _ = listener.await;
                        return;
                    }
                    Some(Ok(())) => {}
                }
                if gpu_preference_enabled {
                    if let Some(game_data) = launcher.game_data() {
                        if let Err(error) =
                            apply_gpu_preference(&game_data.minecraft_java.path, &gpu_preference)
                        {
                            send_launch_message(
                                &update_tx,
                                LogLevel::Warn,
                                LogTarget::JavaRuntime,
                                tr("rust.status.game.gpu-error").replace("%0", &error.to_string()),
                            );
                        }
                    }
                }
                if cancel_requested.load(Ordering::Acquire) {
                    send_launch_message(
                        &update_tx,
                        LogLevel::Warn,
                        LogTarget::Minecraft,
                        tr("rust.status.game.launch-cancelled"),
                    );
                    drop(event_tx);
                    let _ = listener.await;
                    return;
                }
                if remove_undesirable_mods {
                    match remove_undesirable_mod_files(&mods_directory) {
                        Ok(summary) => {
                            if !summary.removed.is_empty() {
                                send_launch_message(
                                    &update_tx,
                                    LogLevel::Info,
                                    LogTarget::Mods,
                                    tr("rust.status.game.removed-mods")
                                        .replace("%0", &summary.removed.join(", ")),
                                );
                            }
                            for (name, error) in summary.failed {
                                send_launch_message(
                                    &update_tx,
                                    LogLevel::Warn,
                                    LogTarget::Mods,
                                    tr("rust.status.game.remove-mod-error")
                                        .replace("%0", &name)
                                        .replace("%1", &error),
                                );
                            }
                        }
                        Err(error) => {
                            send_launch_message(
                                &update_tx,
                                LogLevel::Warn,
                                LogTarget::Mods,
                                tr("rust.status.game.scan-mods-error")
                                    .replace("%0", &error.to_string()),
                            );
                        }
                    }
                }
                match launcher.launch(event_tx.clone()).await {
                    Ok(mut child) => {
                        let _ = update_tx.send(LaunchUiEvent::GameStarted);
                        let wait_result = tokio::select! {
                            result = child.wait() => Some(result),
                            _ = wait_for_cancel(cancel_requested.clone()) => None,
                        };
                        match wait_result {
                            Some(Ok(status)) => {
                                let code = process_exit_code(&status);
                                if crash_detection_enabled && !status.success() {
                                    let crash_runtime = match launcher.game_data() {
                                        Some(game_data) => {
                                            let java_path = game_data.minecraft_java.path.clone();
                                            tokio::task::spawn_blocking(move || {
                                                crash::collect_runtime_info(&java_path)
                                            })
                                            .await
                                            .unwrap_or_default()
                                        }
                                        None => crash::CrashRuntimeInfo::default(),
                                    };
                                    if let Some(report) = collect_crash_report(
                                        &crash_game_dir,
                                        &game_log,
                                        launch_started,
                                        &crash_version,
                                        &crash_loader,
                                        code,
                                        &crash_runtime,
                                    ) {
                                        if let Ok(mut pending) = pending_crash_report.lock() {
                                            *pending = Some(report);
                                        }
                                    }
                                }
                                let _ = event_tx.send(LaunchEvent::Close(code)).await;
                            }
                            Some(Err(error)) => {
                                let message = tr("rust.status.game.monitor-error")
                                    .replace("%0", &error.to_string());
                                send_launch_message(
                                    &update_tx,
                                    LogLevel::Error,
                                    LogTarget::Minecraft,
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
                                let _ = child.kill().await;
                                let _ = child.wait().await;
                                send_launch_message(
                                    &update_tx,
                                    LogLevel::Warn,
                                    LogTarget::Minecraft,
                                    tr("rust.status.game.stopped"),
                                );
                            }
                        }
                    }
                    Err(error) => {
                        let message = tr("rust.status.game.launch-error")
                            .replace("%0", &error.to_string());
                        send_launch_message(
                            &update_tx,
                            LogLevel::Error,
                            LogTarget::Minecraft,
                            message.clone(),
                        );
                        send_launch_error_dialog(
                            &update_tx,
                            tr("launcher.error.title"),
                            message,
                            None,
                        );
                    }
                }
                drop(event_tx);
                let _ = listener.await;
            });

            let _ = done_tx.send(LaunchUiEvent::Finished);
        });
    }
}
