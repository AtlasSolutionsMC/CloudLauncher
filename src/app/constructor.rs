use super::*;

impl LauncherApp {
    pub(in crate::app) fn new(
        cc: &eframe::CreationContext<'_>,
        require_minecraft_account: bool,
        command_line_overrides: CommandLineOverrides,
    ) -> Self {
        let background_light = load_texture(
            &cc.egui_ctx,
            "background-light",
            include_bytes!("../../assets/plains.jpg"),
            false,
        );
        let background_dark = load_texture(
            &cc.egui_ctx,
            "background-dark",
            include_bytes!("../../assets/dark/plains.jpg"),
            false,
        );
        let mut assets = Assets {
            background: background_light.clone(),
            background_light,
            background_dark,
            support: load_texture(
                &cc.egui_ctx,
                "support",
                include_bytes!("../../assets/icons/comments-o.png"),
                true,
            ),
            folder: load_texture(
                &cc.egui_ctx,
                "folder",
                include_bytes!("../../assets/icons/folder-open.png"),
                true,
            ),
            refresh: load_texture(
                &cc.egui_ctx,
                "refresh",
                include_bytes!("../../assets/icons/refresh.png"),
                true,
            ),
            settings: load_texture(
                &cc.egui_ctx,
                "settings",
                include_bytes!("../../assets/icons/bars.png"),
                true,
            ),
            crash_bug: load_texture(
                &cc.egui_ctx,
                "crash-bug",
                include_bytes!("../../assets/icons/bug.png"),
                true,
            ),
            crash_duke: load_texture(
                &cc.egui_ctx,
                "crash-duke",
                include_bytes!("../../assets/icons/logo-duke.png"),
                false,
            ),
        };
        let mut settings = load_settings();
        install_system_ui_font(&cc.egui_ctx);
        let default_settings_exists = settings_path().is_file();
        let custom_settings_path = command_line_overrides
            .settings_path
            .as_deref()
            .map(PathBuf::from);
        let custom_settings_exists = custom_settings_path
            .as_ref()
            .is_some_and(|path| path.is_file());
        let first_run_pending = !default_settings_exists && !custom_settings_exists;
        let mut external_settings_file = None;
        let mut external_settings_error = None;
        if let Some(path) = custom_settings_path {
            match load_external_settings_file(&path, &mut settings) {
                Ok(file) => {
                    if !path.is_file()
                        && let Err(error) = write_external_settings_file(&file, &settings)
                    {
                        external_settings_error = Some(error);
                    }
                    external_settings_file = Some(file);
                }
                Err(error) => external_settings_error = Some(error),
            }
        }
        let persisted_settings = settings.clone();
        command_line_overrides.apply(&mut settings);
        http::set_certificate_validation(settings.verify_tls_certificates);
        let background_custom_loaded = !settings.background_path.trim().is_empty();
        let show_global_logger =
            settings.logger_mode == "global" || command_line_overrides.debug_mode;
        if command_line_overrides.debug_mode {
            cc.egui_ctx.send_viewport_cmd(egui::ViewportCommand::Title(
                "Legacy Launcher — Rust [DEBUG]".to_owned(),
            ));
        }
        let background_load_error = if settings.background_path.trim().is_empty() {
            None
        } else {
            match load_texture_from_path(
                &cc.egui_ctx,
                "background",
                Path::new(&settings.background_path),
            ) {
                Ok(background) => {
                    assets.background = background;
                    None
                }
                Err(error) => Some(error),
            }
        };
        cc.egui_ctx
            .set_zoom_factor(settings.launcher_font_size / 12.0);
        let first_run_folder_current_directory = PathBuf::from(settings.game_directory.trim());
        let first_run_folder_suggestion = first_run_pending
            .then(|| first_run_game_directory_suggestion(&first_run_folder_current_directory))
            .flatten();
        let show_first_run_folder_notice = first_run_folder_suggestion.is_some();
        let first_run_folder_path = first_run_folder_suggestion
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        let show_arm_compat_notice = first_run_pending && is_windows_arm_host();
        let auto_launch_pending = settings.auto_login && settings.selected_account.is_some();
        let update_check_events = Some(spawn_update_check(settings.beta_updates));
        let notice_events = settings
            .notices_enabled
            .then(|| spawn_notice_fetch(settings.client_id.clone()));
        let notices_loading = settings.notices_enabled;
        let version_catalog_cancel = Arc::new(AtomicBool::new(false));
        let mut app = Self {
            settings,
            assets: Some(assets),
            background_theme_dark: None,
            background_custom_loaded: background_custom_loaded && background_load_error.is_none(),
            command_line_overrides,
            persisted_settings,
            external_settings_file,
            extra_version_repositories: Vec::new(),
            require_minecraft_account,
            show_first_run_notice: first_run_pending,
            show_first_run_folder_notice,
            first_run_folder_current_directory,
            first_run_folder_path,
            show_arm_compat_notice,
            version_catalog_events: Some(spawn_catalog_fetch(
                Vec::new(),
                Arc::clone(&version_catalog_cancel),
            )),
            version_catalog_cancel: Some(version_catalog_cancel),
            version_catalog_loading: true,
            show_log_window: show_global_logger,
            show_live_launcher_log: show_global_logger,
            show_live_game_log: false,
            bootstrap_events: Some(spawn_bootstrap_fetch()),
            bootstrap_loading: true,
            notices_disable_allowed: false,
            allow_ely_everywhere: false,
            notice_events,
            notices_loading,
            connectivity_events: Some(spawn_connectivity_check()),
            update_check_events,
            update_checking: true,
            version_kind_filter: "all".to_owned(),
            auto_launch_pending,
            status: external_settings_error
                .or_else(|| {
                    background_load_error
                        .map(|error| tr("rust.status.background.load-error").replace("%0", &error))
                })
                .unwrap_or_default(),
            ..Default::default()
        };
        record_launcher_log_event(
            &app.launcher_log,
            LogLevel::Info,
            LogTarget::Launcher,
            &format!(
                "Starting Legacy Launcher Rust {} ({}/{})",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
        );
        if let Ok(directory) = std::env::current_dir() {
            record_launcher_log_event(
                &app.launcher_log,
                LogLevel::Info,
                LogTarget::Launcher,
                &format!("Working directory: {}", directory.display()),
            );
        }
        record_launcher_log_event(
            &app.launcher_log,
            LogLevel::Debug,
            LogTarget::Settings,
            &format!("Settings file: {}", settings_path().display()),
        );
        if !app.settings.verify_tls_certificates {
            record_launcher_log_event(
                &app.launcher_log,
                LogLevel::Warn,
                LogTarget::Settings,
                &tr("rust.log.connection.insecure-tls"),
            );
        }
        if let Some(file) = &app.external_settings_file {
            let path = match file {
                ExternalSettingsFile::LegacyProperties(path)
                | ExternalSettingsFile::RustJson(path) => path,
            };
            record_launcher_log_event(
                &app.launcher_log,
                LogLevel::Debug,
                LogTarget::Settings,
                &format!("External settings file: {}", path.display()),
            );
        }
        if !app.status.is_empty() {
            let startup_error = app.status.clone();
            app.set_logged_status(LogLevel::Error, LogTarget::Launcher, startup_error);
        }
        app
    }
}
