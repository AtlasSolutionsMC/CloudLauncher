use super::*;

impl LauncherApp {
    pub(in crate::app) fn draw_settings_page(&mut self, ctx: &Context) {
        if let Some(title) = self.settings_page {
            let mut open = true;
            let mut browse_game_directory = false;
            let mut browse_background_file = false;
            let mut browse_theme_file = false;
            let mut logger_mode_changed = false;
            let mut about_action = None;
            let scene_fill = self
                .legacy_theme_palette
                .as_ref()
                .and_then(|palette| palette.panel_background)
                .unwrap_or(Color32::from_rgba_unmultiplied(43, 62, 72, 208));
            let scene_border = self
                .legacy_theme_palette
                .as_ref()
                .and_then(|palette| palette.border_main.or(palette.border))
                .unwrap_or(Color32::from_white_alpha(18));
            let scene_border_size = self
                .legacy_theme_palette
                .as_ref()
                .and_then(|palette| palette.border_size)
                .unwrap_or(1.0);
            let paired_scene = matches!(title, "Minecraft" | "Launcher");
            let ui_scale = self.settings.launcher_font_size / 12.0;
            let window_size = match title {
                "Minecraft" | "Launcher" => {
                    [JAVA_SETTINGS_PANEL_SIZE.x, JAVA_SETTINGS_PANEL_SIZE.y]
                }
                "Manage accounts" => match self.account_manager_view {
                    AccountManagerView::Overview => [800.0, 440.0],
                    AccountManagerView::Add | AccountManagerView::Edit => [440.0, 440.0],
                },
                "Manage versions" => [470.0, 550.0],
                _ => [600.0, 550.0],
            }
            .map(|dimension| dimension * ui_scale);
            let screen = ctx.content_rect();
            let window_size_vec = Vec2::new(window_size[0], window_size[1]);
            let mut navigate_to = None;
            let mut return_home = false;
            let window_position = if paired_scene {
                java_scene_layout(
                    screen,
                    &self.settings.login_form_direction,
                    JAVA_LOGIN_FORM_SIZE * ui_scale,
                    window_size_vec,
                )
                .panel_top_left
            } else {
                Pos2::new(
                    screen.center().x - window_size_vec.x / 2.0,
                    screen.center().y - window_size_vec.y / 2.0,
                )
            };
            let window_id = if title == "Manage accounts" {
                match self.account_manager_view {
                    AccountManagerView::Overview => Id::unique("account_manager_overview"),
                    AccountManagerView::Add => Id::unique("account_manager_add"),
                    AccountManagerView::Edit => Id::unique("account_manager_edit"),
                }
            } else {
                Id::unique(("settings_page", title))
            };
            egui::Window::new(settings_page_title(title))
                .id(window_id)
                .order(OVERLAY_LAYER)
                .open(&mut open)
                .title_bar(false)
                .collapsible(false)
                .resizable(title != "Manage accounts")
                .fixed_pos(window_position)
                .default_size(window_size)
                .frame(
                    egui::Frame::new()
                        .fill(scene_fill)
                        .stroke(Stroke::new(scene_border_size, scene_border))
                        .inner_margin(Margin::same(12)),
                )
                .show(&ctx, |ui| {
                    if paired_scene {
                        ui.horizontal_wrapped(|ui| {
                            for (page, key) in [
                                ("Minecraft", "settings.tab.minecraft"),
                                ("Launcher", "settings.tab.tlauncher"),
                            ] {
                                if ui
                                    .selectable_label(title == page, tr(key))
                                    .clicked()
                                {
                                    navigate_to = Some(page);
                                }
                            }
                        });
                        ui.separator();
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("settings_page_content")
                        .auto_shrink([false, false])
                        .show(ui, |ui| match title {
                    "About us" => {
                        ui.horizontal(|ui| {
                            ui.heading("Legacy Launcher");
                            ui.label(format!("v{}", env!("CARGO_PKG_VERSION")));
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.label("(by");
                            if ui.link("turikhay").clicked() {
                                self.open_url_with_status("https://turikhay.com", LogTarget::Launcher);
                            }
                            ui.label(")");
                        });
                        render_notice_html(
                            ui,
                            &tr("about.text"),
                            true,
                            &mut about_action,
                        );
                        ui.add_space(10.0);
                        ui.separator();
                        ui.horizontal_wrapped(|ui| {
                            if ui.link("Ely.by").clicked() {
                                self.open_url_with_status("https://ely.by", LogTarget::Launcher);
                            }
                            ui.label("—");
                        });
                        render_notice_html(
                            ui,
                            &tr("about.partners.ely"),
                            true,
                            &mut about_action,
                        );
                        ui.separator();
                        render_notice_html(
                            ui,
                            &tr("about.license"),
                            true,
                            &mut about_action,
                        );
                    }
                    "Launcher legacy controls" => {
                        if title == "Launcher" {
                        ui.label(tr("rust.settings.locale.english-us"));
                        ui.horizontal_wrapped(|ui| {
                            let update_label = if self.update_checking {
                                tr("update.checking")
                            } else {
                                tr("update.check.button")
                            };
                            if ui
                                .add_enabled(
                                    !self.update_checking,
                                    egui::Button::new(update_label).small(),
                                )
                                .clicked()
                            {
                                self.start_update_check();
                            }
                            if ui.button(tr("settings.default")).clicked() {
                                self.show_settings_reset_confirmation = true;
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.switch-to-beta.label"));
                            if ui
                                .checkbox(
                                    &mut self.settings.beta_updates,
                                    tr("settings.switch-to-beta"),
                                )
                                .changed()
                            {
                                self.persist();
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.slide.list.label"));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.settings.background_path)
                                    .desired_width(280.0)
                                    .interactive(false),
                            );
                            if ui.button(tr("newfolder.select.browse")).clicked() {
                                browse_background_file = true;
                            }
                            if ui.button(tr("rust.settings.background.default")).clicked() {
                                self.settings.background_path.clear();
                                self.background_custom_loaded = false;
                                self.persist();
                                if let Some(assets) = &mut self.assets {
                                    assets.background = if self.background_theme_dark.unwrap_or(false) {
                                        assets.background_dark.clone()
                                    } else {
                                        assets.background_light.clone()
                                    };
                                }
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("rust.settings.theme.file"));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.settings.legacy_theme_path)
                                    .desired_width(280.0)
                                    .interactive(false),
                            );
                            if ui.button(tr("newfolder.select.browse")).clicked() {
                                browse_theme_file = true;
                            }
                            if ui.button(tr("rust.settings.theme.default")).clicked() {
                                self.settings.legacy_theme_path.clear();
                                self.persist();
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.fontsize.label"));
                            if ui
                                .add(
                                    egui::Slider::new(
                                        &mut self.settings.launcher_font_size,
                                        12.0..=18.0,
                                    )
                                    .integer()
                                    .suffix(format!(" {}", tr("settings.fontsize.pt"))),
                                )
                                .changed()
                            {
                                ui.ctx().set_zoom_factor(self.settings.launcher_font_size / 12.0);
                                self.persist();
                            }
                        });
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.laf.label"));
                            let state_label = tr(look_and_feel_label_key(
                                &self.settings.look_and_feel,
                            ));
                            ComboBox::from_id_salt("look_and_feel")
                                .selected_text(state_label)
                                .show_ui(ui, |ui| {
                                    for (state, key) in [
                                        ("off", "settings.laf.state.off"),
                                        ("system", "settings.laf.state.system"),
                                        ("auto", "settings.laf.state.auto"),
                                        ("dark", "settings.laf.state.dark"),
                                        ("light", "settings.laf.state.light"),
                                    ] {
                                        if ui
                                            .selectable_value(
                                                &mut self.settings.look_and_feel,
                                                state.to_owned(),
                                                tr(key),
                                            )
                                            .changed()
                                        {
                                            self.persist();
                                        }
                                    }
                                });
                        });
                        ui.separator();
                        }
                        if title == "Minecraft" {
                        ui.separator();
                        ui.label(tr("settings.client.gamedir.label"));
                        ui.horizontal(|ui| {
                            let path = ui.text_edit_singleline(&mut self.settings.game_directory);
                            if path.changed()
                                && PathBuf::from(self.settings.game_directory.trim()).is_absolute()
                            {
                                self.persist();
                            }
                            if ui
                                .small_button(tr("rust.settings.gamedir.browse"))
                                .clicked()
                            {
                                browse_game_directory = true;
                            }
                            if ui.small_button(tr("rust.settings.gamedir.default")).clicked() {
                                self.settings.game_directory =
                                    default_game_directory().to_string_lossy().into_owned();
                                self.persist();
                            }
                            if ui.small_button(tr("rust.settings.gamedir.open")).clicked() {
                                let path = self.game_dir_path();
                                self.open_path_with_status(&path, LogTarget::Settings);
                            }
                        });
                        }
                        if title == "Launcher" {
                        ui.horizontal(|ui| {
                            ui.label(tr("launcher.window.size"));
                            let mut changed = false;
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut self.settings.launcher_window_width)
                                        .range(700..=7680)
                                        .suffix(" px"),
                                )
                                .changed();
                            ui.label("×");
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut self.settings.launcher_window_height)
                                        .range(600..=4320)
                                        .suffix(" px"),
                                )
                                .changed();
                            if changed {
                                ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(
                                    Vec2::new(
                                        self.settings.launcher_window_width as f32,
                                        self.settings.launcher_window_height as f32,
                                    ),
                                ));
                                self.persist();
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.direction.label"));
                            let label_key = login_form_direction_label_key(
                                &self.settings.login_form_direction,
                            );
                            ComboBox::from_id_salt("login_form_direction")
                                .selected_text(tr(label_key))
                                .show_ui(ui, |ui| {
                                    for (direction, key) in [
                                        ("top_left", "settings.direction.top_left"),
                                        ("top", "settings.direction.top"),
                                        ("top_right", "settings.direction.top_right"),
                                        ("center_left", "settings.direction.center_left"),
                                        ("center", "settings.direction.center"),
                                        ("center_right", "settings.direction.center_right"),
                                        ("bottom_left", "settings.direction.bottom_left"),
                                        ("bottom", "settings.direction.bottom"),
                                        ("bottom_right", "settings.direction.bottom_right"),
                                    ] {
                                        if ui
                                            .selectable_value(
                                                &mut self.settings.login_form_direction,
                                                direction.to_owned(),
                                                tr(key),
                                            )
                                            .changed()
                                        {
                                            self.persist();
                                        }
                                    }
                                });
                        });
                        }
                        if title == "Minecraft" {
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.client.gamedir.separate.label"));
                            ComboBox::from_id_salt("separate_game_directories")
                                .selected_text(tr(match self.settings.separate_game_directories {
                                    SeparateDirs::Family => {
                                        "settings.client.gamedir.separatedirs.family"
                                    }
                                    SeparateDirs::Version => {
                                        "settings.client.gamedir.separatedirs.version"
                                    }
                                    SeparateDirs::None => {
                                        "settings.client.gamedir.separatedirs.none"
                                    }
                                }))
                                .show_ui(ui, |ui| {
                                    for (mode, key) in [
                                        (
                                            SeparateDirs::Family,
                                            "settings.client.gamedir.separatedirs.family",
                                        ),
                                        (
                                            SeparateDirs::Version,
                                            "settings.client.gamedir.separatedirs.version",
                                        ),
                                        (
                                            SeparateDirs::None,
                                            "settings.client.gamedir.separatedirs.none",
                                        ),
                                    ] {
                                        if ui
                                            .selectable_value(
                                                &mut self.settings.separate_game_directories,
                                                mode,
                                                tr(key),
                                            )
                                            .changed()
                                        {
                                            self.persist();
                                        }
                                    }
                                });
                        });
                        }
                        if title == "Launcher" {
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.launch-action.label"));
                            ComboBox::from_id_salt("launch_action")
                                .selected_text(tr(launch_action_label_key(
                                    &self.settings.launch_action,
                                )))
                                .show_ui(ui, |ui| {
                                    for (value, key) in [
                                        ("hide", "settings.launch-action.hide"),
                                        ("exit", "settings.launch-action.exit"),
                                        ("nothing", "settings.launch-action.nothing"),
                                    ] {
                                        if ui
                                            .selectable_value(
                                                &mut self.settings.launch_action,
                                                value.to_owned(),
                                                tr(key),
                                            )
                                            .changed()
                                        {
                                            self.persist();
                                        }
                                    }
                                });
                        });
                        }

                        if title == "Minecraft" {
                        ui.separator();
                        if ui
                            .checkbox(
                                &mut self.settings.promoted_servers_in_game,
                                tr("settings.promotion.ingame"),
                            )
                            .changed()
                        {
                            self.persist();
                        }
                        ui.label(tr("settings.promotion.ingame.hint"));
                        if ui
                            .checkbox(
                                &mut self.settings.remove_undesirable_mods,
                                tr("settings.mods.remove-undesirable"),
                            )
                            .changed()
                        {
                            self.persist();
                        }
                        ui.label(tr("settings.mods.remove-undesirable.hint"));
                        }
                        if title == "Launcher" {
                        if ui
                            .checkbox(
                                &mut self.settings.crash_detection_enabled,
                                tr("settings.crash.enable"),
                            )
                            .changed()
                        {
                            self.persist();
                        }
                        if ui
                            .checkbox(
                                &mut self.settings.show_full_game_command,
                                tr("settings.logger.fullcommand"),
                            )
                            .changed()
                        {
                            self.persist();
                        }
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.logger.label"));
                            ComboBox::from_id_salt("logger_mode")
                                .selected_text(tr(if self.settings.logger_mode == "global" {
                                    "settings.logger.global"
                                } else {
                                    "settings.logger.none"
                                }))
                                .show_ui(ui, |ui| {
                                    for (mode, label) in [
                                        ("global", "settings.logger.global"),
                                        ("none", "settings.logger.none"),
                                    ] {
                                        logger_mode_changed |= ui
                                            .selectable_value(
                                                &mut self.settings.logger_mode,
                                                mode.to_owned(),
                                                tr(label),
                                            )
                                            .changed();
                                    }
                                });
                        });
                        }
                        if title == "Minecraft" {
                        if ui
                            .checkbox(
                                &mut self.settings.delete_patchy,
                                tr("settings.patchy.fix"),
                            )
                            .changed()
                        {
                            self.persist();
                        }
                        ui.label(tr("settings.patchy.fix.hint"));
                        ui.separator();
                        ui.label(tr("rust.settings.memory.heading"));
                        let mut memory_changed = false;
                        if ui
                            .checkbox(
                                &mut self.settings.automatic_memory,
                                tr("settings.java.memory.auto"),
                            )
                            .changed()
                        {
                            self.persist();
                        }
                        ui.label(tr("rust.settings.memory.auto-description"));
                        ui.horizontal(|ui| {
                            ui.label(tr("rust.settings.memory.minimum"));
                            memory_changed |= ui
                                .text_edit_singleline(&mut self.settings.min_memory)
                                .changed();
                            ui.label(tr("rust.settings.memory.maximum"));
                            memory_changed |= ui
                                .add_enabled(
                                    !self.settings.automatic_memory,
                                    egui::TextEdit::singleline(&mut self.settings.max_memory),
                                )
                                .changed();
                        });
                        let min_memory = parse_memory_size(&self.settings.min_memory);
                        let max_memory = parse_memory_size(&self.settings.max_memory);
                        if self.settings.automatic_memory {
                            if min_memory.is_some() {
                                if memory_changed {
                                    self.persist();
                                }
                            } else {
                                ui.colored_label(
                                    Color32::LIGHT_RED,
                                    tr("rust.settings.memory.invalid-value"),
                                );
                            }
                        } else if let (Some(min), Some(max)) = (min_memory, max_memory) {
                            if min <= max {
                                if memory_changed {
                                    self.persist();
                                }
                            } else {
                                ui.colored_label(
                                    Color32::LIGHT_RED,
                                    tr("rust.settings.memory.invalid-range"),
                                );
                            }
                        } else {
                            ui.colored_label(
                                Color32::LIGHT_RED,
                                tr("rust.settings.memory.invalid-value"),
                            );
                        }

                        ui.separator();
                        if ui
                            .checkbox(
                                &mut self.settings.fullscreen,
                                tr("settings.client.resolution.fullscreen"),
                            )
                            .changed()
                        {
                            self.persist();
                        }
                        let mut display_changed = false;
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.client.resolution.label"));
                            display_changed |= ui
                                .add(
                                    egui::DragValue::new(&mut self.settings.screen_width)
                                        .range(320..=7680)
                                        .suffix(" px"),
                                )
                                .changed();
                            ui.label("×");
                            display_changed |= ui
                                .add(
                                    egui::DragValue::new(&mut self.settings.screen_height)
                                        .range(240..=4320)
                                        .suffix(" px"),
                                )
                                .changed();
                        });
                        if display_changed {
                            self.persist();
                        }
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.gpu.label"));
                            ComboBox::from_id_salt("gpu_preference")
                                .selected_text(tr(match self.settings.gpu_preference.as_str() {
                                    "system" => "settings.gpu.default.label",
                                    "integrated" => "settings.gpu.integrated.label",
                                    _ => "settings.gpu.discrete.label",
                                }))
                                .show_ui(ui, |ui| {
                                    for (preference, key) in [
                                        ("system", "settings.gpu.default.label"),
                                        ("integrated", "settings.gpu.integrated.label"),
                                        ("discrete", "settings.gpu.discrete.label"),
                                    ] {
                                        if ui
                                            .selectable_value(
                                                &mut self.settings.gpu_preference,
                                                preference.to_owned(),
                                                tr(key),
                                            )
                                            .changed()
                                        {
                                            self.persist();
                                        }
                                    }
                                });
                        });
                        if ui
                            .checkbox(
                                &mut self.settings.windows_gpu_performance,
                                tr("rust.settings.gpu.windows-preference"),
                            )
                            .changed()
                        {
                            self.persist();
                        }
                        ui.label(tr("settings.jre.window.configure.label"));
                        ComboBox::from_id_salt("java_mode")
                            .selected_text(tr(match self.settings.java_mode.as_str() {
                                "current" => "settings.jre.window.select.current",
                                "custom" => "settings.jre.window.select.custom",
                                _ => "settings.jre.window.select.recommended",
                            }))
                            .show_ui(ui, |ui| {
                                for (mode, key) in [
                                    ("recommended", "settings.jre.window.select.recommended"),
                                    ("current", "settings.jre.window.select.current"),
                                    ("custom", "settings.jre.window.select.custom"),
                                ] {
                                    if ui
                                        .selectable_value(
                                            &mut self.settings.java_mode,
                                            mode.to_owned(),
                                            tr(key),
                                        )
                                        .changed()
                                    {
                                        if mode == "recommended" {
                                            self.settings.java_path.clear();
                                        }
                                        self.persist();
                                    }
                                }
                            });
                        match self.settings.java_mode.as_str() {
                            "current" => {
                                ui.label(tr("settings.jre.window.select.current.hint"));
                                if ui
                                    .button(tr("rust.settings.java.check-current"))
                                    .clicked()
                                {
                                    match detect_java() {
                                        Ok((path, version)) => {
                                            self.set_logged_status(
                                                LogLevel::Info,
                                                LogTarget::JavaRuntime,
                                                tr("rust.status.java.found-current")
                                                    .replace("%0", &version)
                                                    .replace("%1", &path.display().to_string()),
                                            );
                                        }
                                        Err(error) => self.set_logged_status(
                                            LogLevel::Error,
                                            LogTarget::JavaRuntime,
                                            error,
                                        ),
                                    }
                                }
                            }
                            "custom" => {
                                ui.label(tr(
                                    "settings.jre.window.configure.path.custom.placeholder.windows",
                                ));
                                ui.horizontal(|ui| {
                                    if ui
                                        .text_edit_singleline(&mut self.settings.java_path)
                                        .changed()
                                    {
                                        self.persist();
                                    }
                                    if ui
                                        .small_button(tr("rust.settings.java.detect"))
                                        .clicked()
                                    {
                                        match detect_java() {
                                            Ok((path, version)) => {
                                                self.settings.java_path =
                                                    path.to_string_lossy().into_owned();
                                                self.set_logged_status(
                                                    LogLevel::Info,
                                                    LogTarget::JavaRuntime,
                                                    tr("rust.status.java.found")
                                                        .replace("%0", &version)
                                                        .replace("%1", &path.display().to_string()),
                                                );
                                                self.persist();
                                            }
                                            Err(error) => self.set_logged_status(
                                                LogLevel::Error,
                                                LogTarget::JavaRuntime,
                                                error,
                                            ),
                                        }
                                    }
                                });
                            }
                            _ => {
                                ui.label(tr("settings.jre.window.select.recommended.hint"));
                            }
                        }
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.jre.window.configure.jvm-args.improved"));
                            ComboBox::from_id_salt("optimized_java_args")
                                .selected_text(tr(match self.settings.optimized_java_args.as_str() {
                                    "none" => "settings.jre.window.configure.jvm-args.type.none",
                                    "g1gc" => "settings.jre.window.configure.jvm-args.type.g1gc",
                                    "shenandoah" => "settings.jre.window.configure.jvm-args.type.shenandoah",
                                    "zgc" => "settings.jre.window.configure.jvm-args.type.zgc",
                                    _ => "settings.jre.window.configure.jvm-args.type.default",
                                }))
                                .show_ui(ui, |ui| {
                                    for (value, label) in [
                                        ("none", "settings.jre.window.configure.jvm-args.type.none"),
                                        ("default", "settings.jre.window.configure.jvm-args.type.default"),
                                        ("g1gc", "settings.jre.window.configure.jvm-args.type.g1gc"),
                                        ("shenandoah", "settings.jre.window.configure.jvm-args.type.shenandoah"),
                                        ("zgc", "settings.jre.window.configure.jvm-args.type.zgc"),
                                    ] {
                                        if ui
                                            .selectable_value(
                                                &mut self.settings.optimized_java_args,
                                                value.to_owned(),
                                                tr(label),
                                            )
                                            .changed()
                                        {
                                            self.persist();
                                        }
                                    }
                                });
                        });
                        if ui
                            .checkbox(
                                &mut self.settings.use_current_trust_store,
                                tr("settings.jre.window.configure.jvm-args.use-current-trust-store"),
                            )
                            .changed()
                        {
                            self.persist();
                        }
                        if ui
                            .checkbox(
                                &mut self.settings.verify_tls_certificates,
                                tr("rust.settings.network.verify-tls"),
                            )
                            .changed()
                        {
                            if !self.settings.verify_tls_certificates {
                                record_launcher_log_event(
                                    &self.launcher_log,
                                    LogLevel::Warn,
                                    LogTarget::Settings,
                                    &tr("rust.log.connection.insecure-tls"),
                                );
                            }
                            self.persist();
                        }
                        if !self.settings.verify_tls_certificates {
                            ui.colored_label(
                                Color32::YELLOW,
                                tr("rust.settings.network.insecure-tls.warning"),
                            );
                        }
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.jre.window.configure.wrapper-command"));
                            if ui
                                .text_edit_singleline(&mut self.settings.wrapper_command)
                                .changed()
                            {
                                self.persist();
                            }
                        });
                        ui.label(tr("settings.jre.window.configure.wrapper-command.hint"));
                        ui.separator();
                        ui.heading(tr("rust.settings.java.managed.heading"));
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.jre.window.configure.runtime-directory"));
                            if ui
                                .text_edit_singleline(&mut self.settings.java_runtime_directory)
                                .changed()
                            {
                                self.persist();
                            }
                        });
                        let game_root = self.game_root_for_version(&self.settings.selected_version);
                        let runtime_root = if self.settings.java_runtime_directory.trim().is_empty() {
                            game_root.join("runtime")
                        } else {
                            PathBuf::from(self.settings.java_runtime_directory.trim())
                        };
                        ui.horizontal(|ui| {
                            ui.label(runtime_root.to_string_lossy());
                            if ui
                                .small_button(tr("settings.jre.window.configure.path.recommended.actual.open"))
                                .clicked()
                            {
                                if let Err(error) = fs::create_dir_all(&runtime_root) {
                                    self.set_logged_status(
                                        LogLevel::Error,
                                        LogTarget::JavaRuntime,
                                        tr("rust.status.java.runtime-folder-error")
                                            .replace("%0", &error.to_string()),
                                    );
                                } else {
                                    self.open_path_with_status(&runtime_root, LogTarget::JavaRuntime);
                                }
                            }
                        });
                        let managed_runtimes = list_managed_java_runtimes(&runtime_root);
                        if managed_runtimes.is_empty() {
                            ui.label(tr("rust.settings.java.managed.empty"));
                        } else {
                            ui.label(
                                tr("rust.settings.java.managed.count")
                                    .replace("%0", &managed_runtimes.len().to_string()),
                            );
                            egui::ScrollArea::vertical().max_height(110.0).show(ui, |ui| {
                                for java_path in managed_runtimes.iter().take(12) {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(java_path.to_string_lossy()).small());
                                        if ui
                                            .small_button(tr("rust.settings.java.use-custom"))
                                            .clicked()
                                        {
                                            self.settings.java_path = java_path.to_string_lossy().into_owned();
                                            self.settings.java_mode = "custom".to_owned();
                                            self.persist();
                                        }
                                    });
                                }
                            });
                        }
                        let installed_game_data = self
                            .game_data_directory_for_version(&self.settings.selected_version)
                            .join("gameData.json");
                        if let Ok(bytes) = fs::read(installed_game_data)
                            && let Ok(game_data) = serde_json::from_slice::<serde_json::Value>(&bytes)
                            && let Some(java_path) = game_data
                                .get("minecraftJava")
                                .and_then(|java| java.get("path"))
                                .and_then(serde_json::Value::as_str)
                        {
                            let major = game_data
                                .get("minecraftJava")
                                .and_then(|java| java.get("majorVersion"))
                                .and_then(serde_json::Value::as_u64)
                                .map(|major| major.to_string())
                                .unwrap_or_else(|| "unknown".to_owned());
                            ui.label(
                                tr("settings.jre.window.configure.path.recommended.actual.hint.0")
                                    .replace("%0", &self.settings.selected_version)
                                    .replace("%1", &major),
                            );
                            ui.label(RichText::new(java_path).small());
                            ui.label(
                                tr("settings.jre.window.configure.path.recommended.actual.hint.1"),
                            );
                        }
                        ui.separator();
                        ui.label(tr("rust.settings.loader.type"));
                        let loader_labels = [
                            ("none", "rust.settings.loader.vanilla"),
                            ("forge", "rust.settings.loader.forge"),
                            ("neoforge", "rust.settings.loader.neoforge"),
                            ("fabric", "rust.settings.loader.fabric"),
                            ("legacyfabric", "rust.settings.loader.legacyfabric"),
                            ("quilt", "rust.settings.loader.quilt"),
                        ];
                        ComboBox::from_id_salt("loader_type")
                            .selected_text(
                                loader_labels
                                    .iter()
                                    .find(|(value, _)| *value == self.settings.loader_type)
                                    .map(|(_, key)| tr(key))
                                    .unwrap_or_else(|| tr("rust.settings.loader.vanilla")),
                            )
                            .show_ui(ui, |ui| {
                                for (value, label) in loader_labels {
                                    if ui
                                        .selectable_value(
                                            &mut self.settings.loader_type,
                                            value.to_owned(),
                                            tr(label),
                                        )
                                        .changed()
                                    {
                                        self.persist();
                                    }
                                }
                            });
                        if self.settings.loader_type != "none" {
                            ui.horizontal(|ui| {
                                ui.label(tr("rust.settings.loader.build"));
                                if ui
                                    .text_edit_singleline(&mut self.settings.loader_build)
                                    .changed()
                                {
                                    self.persist();
                                }
                            });
                            ui.label(tr("rust.settings.loader.build.hint"));
                        }
                        ui.separator();
                        ui.label(tr("settings.jre.window.configure.jvm-args"));
                        if ui
                            .text_edit_singleline(&mut self.settings.jvm_args)
                            .changed()
                        {
                            self.persist();
                        }
                        ui.label(tr("settings.jre.window.configure.mc-args"));
                        if ui
                            .text_edit_singleline(&mut self.settings.game_args)
                            .changed()
                        {
                            self.persist();
                        }
                        ui.separator();
                        }
                        if title == "Launcher" {
                        ui.heading(tr("rust.settings.import.settings.heading"));
                        ui.horizontal(|ui| {
                            if ui
                                .text_edit_singleline(&mut self.settings.legacy_config_path)
                                .changed()
                            {
                                self.persist();
                            }
                            if ui
                                .small_button(tr("rust.settings.import.settings.button"))
                                .clicked()
                            {
                                let path = if self.settings.legacy_config_path.trim().is_empty() {
                                    default_legacy_config_path()
                                } else {
                                    PathBuf::from(self.settings.legacy_config_path.trim())
                                };
                                let previous_settings = self.settings.clone();
                                match import_legacy_settings(&path, &mut self.settings) {
                                    Ok(summary) => {
                                        self.persisted_settings = self.settings.clone();
                                        self.command_line_overrides.apply(&mut self.settings);
                                        let background_error = self.apply_imported_settings_effects(
                                            &previous_settings,
                                            &ctx,
                                        );
                                        if let Some(error) = background_error {
                                            self.set_logged_status(
                                                LogLevel::Error,
                                                LogTarget::Settings,
                                                error,
                                            );
                                        } else {
                                            self.set_logged_status(
                                                LogLevel::Info,
                                                LogTarget::Settings,
                                                tr("rust.status.import.settings.success")
                                                    .replace(
                                                        "%0",
                                                        &summary.applied.to_string(),
                                                    )
                                                    .replace(
                                                        "%1",
                                                        &summary.skipped.to_string(),
                                                    ),
                                            );
                                        }
                                        self.persist();
                                    }
                                    Err(error) => {
                                        self.set_logged_status(
                                            LogLevel::Error,
                                            LogTarget::Settings,
                                            tr("rust.status.import.settings.error")
                                                .replace("%0", &error.to_string()),
                                        );
                                    }
                                }
                            }
                        });
                        if ui
                            .small_button(tr("rust.settings.import.settings.default-path"))
                            .clicked()
                        {
                            self.settings.legacy_config_path =
                                default_legacy_config_path().to_string_lossy().into_owned();
                            self.persist();
                        }
                        ui.label(tr("rust.settings.import.settings.hint"));
                        }
                    }
                    "Launcher" => {
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.clientres.label"));
                            let mut changed = false;
                            changed |= ui
                                .add_sized(
                                    Vec2::new(250.0, 28.0),
                                    egui::DragValue::new(&mut self.settings.launcher_window_width)
                                        .range(700..=7680),
                                )
                                .changed();
                            ui.label("×");
                            changed |= ui
                                .add_sized(
                                    Vec2::new(250.0, 28.0),
                                    egui::DragValue::new(&mut self.settings.launcher_window_height)
                                        .range(600..=4320),
                                )
                                .changed();
                            if changed {
                                ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(
                                    Vec2::new(
                                        self.settings.launcher_window_width as f32,
                                        self.settings.launcher_window_height as f32,
                                    ),
                                ));
                                self.persist();
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.direction.label"));
                            let label_key =
                                login_form_direction_label_key(&self.settings.login_form_direction);
                            ComboBox::from_id_salt("login_form_direction")
                                .selected_text(tr(label_key))
                                .width(ui.available_width())
                                .show_ui(ui, |ui| {
                                    for (direction, key) in [
                                        ("top_left", "settings.direction.top_left"),
                                        ("top", "settings.direction.top"),
                                        ("top_right", "settings.direction.top_right"),
                                        ("center_left", "settings.direction.center_left"),
                                        ("center", "settings.direction.center"),
                                        ("center_right", "settings.direction.center_right"),
                                        ("bottom_left", "settings.direction.bottom_left"),
                                        ("bottom", "settings.direction.bottom"),
                                        ("bottom_right", "settings.direction.bottom_right"),
                                    ] {
                                        if ui
                                            .selectable_value(
                                                &mut self.settings.login_form_direction,
                                                direction.to_owned(),
                                                tr(key),
                                            )
                                            .changed()
                                        {
                                            self.persist();
                                        }
                                    }
                                });
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.laf.label"));
                            ComboBox::from_id_salt("look_and_feel")
                                .selected_text(tr(look_and_feel_label_key(
                                    &self.settings.look_and_feel,
                                )))
                                .width(ui.available_width())
                                .show_ui(ui, |ui| {
                                    for (state, key) in [
                                        ("off", "settings.laf.state.off"),
                                        ("system", "settings.laf.state.system"),
                                        ("auto", "settings.laf.state.auto"),
                                        ("dark", "settings.laf.state.dark"),
                                        ("light", "settings.laf.state.light"),
                                    ] {
                                        if ui
                                            .selectable_value(
                                                &mut self.settings.look_and_feel,
                                                state.to_owned(),
                                                tr(key),
                                            )
                                            .changed()
                                        {
                                            self.persist();
                                        }
                                    }
                                });
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.slide.list.label"));
                            let background = ui.add(
                                egui::TextEdit::singleline(&mut self.settings.background_path)
                                    .desired_width(390.0)
                                    .hint_text(tr("settings.slide.list.prompt.image")),
                            );
                            if background.changed() {
                                let background_path = self.settings.background_path.trim().to_owned();
                                if background_path.is_empty() {
                                    self.background_custom_loaded = false;
                                    if let Some(assets) = &mut self.assets {
                                        assets.background = if self.background_theme_dark
                                            .unwrap_or(false)
                                        {
                                            assets.background_dark.clone()
                                        } else {
                                            assets.background_light.clone()
                                        };
                                    }
                                    self.persist();
                                } else if Path::new(&background_path).is_file() {
                                    match load_texture_from_path(
                                        &ctx,
                                        "background",
                                        Path::new(&background_path),
                                    ) {
                                        Ok(texture) => {
                                            self.background_custom_loaded = true;
                                            if let Some(assets) = &mut self.assets {
                                                assets.background = texture;
                                            }
                                            self.persist();
                                        }
                                        Err(error) => self.set_logged_status(
                                            LogLevel::Error,
                                            LogTarget::Settings,
                                            tr("rust.status.background.load-error").replace("%0", &error),
                                        ),
                                    }
                                }
                            }
                            if ui.button(tr("newfolder.select.browse")).clicked() {
                                browse_background_file = true;
                            }
                        });
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.logger.label"));
                            ComboBox::from_id_salt("logger_mode")
                                .selected_text(tr(if self.settings.logger_mode == "global" {
                                    "settings.logger.global"
                                } else {
                                    "settings.logger.none"
                                }))
                                .width(ui.available_width())
                                .show_ui(ui, |ui| {
                                    for (mode, label) in [
                                        ("none", "settings.logger.none"),
                                        ("global", "settings.logger.global"),
                                    ] {
                                        logger_mode_changed |= ui
                                            .selectable_value(
                                                &mut self.settings.logger_mode,
                                                mode.to_owned(),
                                                tr(label),
                                            )
                                            .changed();
                                    }
                                });
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.logger.fullcommand.label"));
                            if ui
                                .checkbox(
                                    &mut self.settings.show_full_game_command,
                                    tr("settings.logger.fullcommand"),
                                )
                                .changed()
                            {
                                self.persist();
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.launch-action.label"));
                            ComboBox::from_id_salt("launch_action")
                                .selected_text(tr(launch_action_label_key(
                                    &self.settings.launch_action,
                                )))
                                .width(ui.available_width())
                                .show_ui(ui, |ui| {
                                    for (value, key) in [
                                        ("hide", "settings.launch-action.hide"),
                                        ("exit", "settings.launch-action.exit"),
                                        ("nothing", "settings.launch-action.nothing"),
                                    ] {
                                        if ui
                                            .selectable_value(
                                                &mut self.settings.launch_action,
                                                value.to_owned(),
                                                tr(key),
                                            )
                                            .changed()
                                        {
                                            self.persist();
                                        }
                                    }
                                });
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.crash.label"));
                            if ui
                                .checkbox(
                                    &mut self.settings.crash_detection_enabled,
                                    tr("settings.crash.enable"),
                                )
                                .changed()
                            {
                                self.persist();
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.alert-on.label"));
                            for (kind, key) in [
                                ("release", "settings.alert-on.release"),
                                ("snapshot", "settings.alert-on.snapshot"),
                                ("modified", "settings.alert-on.modified"),
                            ] {
                                let enabled = self
                                    .settings
                                    .version_alert_types
                                    .entry(kind.to_owned())
                                    .or_insert(false);
                                if ui.checkbox(enabled, tr(key)).changed() {
                                    self.persist();
                                }
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("notice.enable.label"));
                            if ui
                                .checkbox(
                                    &mut self.settings.notices_enabled,
                                    tr("notice.enable"),
                                )
                                .changed()
                            {
                                self.notice_events = None;
                                self.notices_loading = false;
                                self.personal_notice_list.clear();
                                self.refresh_notices();
                                self.rebuild_notice_list(&ctx);
                                self.persist();
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.switch-to-beta.label"));
                            if ui
                                .checkbox(
                                    &mut self.settings.beta_updates,
                                    tr("settings.switch-to-beta"),
                                )
                                .changed()
                            {
                                self.available_release = None;
                                self.update_prompt_pending = false;
                                if !self.update_checking {
                                    self.start_update_check();
                                }
                                self.persist();
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.lang.label"));
                            ComboBox::from_id_salt("launcher_language")
                                .selected_text("English (en_US)")
                                .width(ui.available_width())
                                .show_ui(ui, |ui| {
                                    let _ = ui.selectable_label(true, "English (en_US)");
                                });
                        });
                    }
                    "Minecraft" => {
                        ui.label(tr("settings.client.gamedir.label"));
                        ui.horizontal(|ui| {
                            let directory = ui.add(
                                egui::TextEdit::singleline(&mut self.settings.game_directory)
                                    .desired_width(470.0),
                            );
                            if directory.changed()
                                && PathBuf::from(self.settings.game_directory.trim()).is_absolute()
                            {
                                self.persist();
                            }
                            if ui.button(tr("newfolder.select.browse")).clicked() {
                                browse_game_directory = true;
                            }
                        });
                        ComboBox::from_id_salt("separate_game_directories")
                            .selected_text(tr(match self.settings.separate_game_directories {
                                SeparateDirs::Family => {
                                    "settings.client.gamedir.separatedirs.family"
                                }
                                SeparateDirs::Version => {
                                    "settings.client.gamedir.separatedirs.version"
                                }
                                SeparateDirs::None => "settings.client.gamedir.separatedirs.none",
                            }))
                            .width(ui.available_width())
                            .show_ui(ui, |ui| {
                                for (mode, key) in [
                                    (
                                        SeparateDirs::Family,
                                        "settings.client.gamedir.separatedirs.family",
                                    ),
                                    (
                                        SeparateDirs::Version,
                                        "settings.client.gamedir.separatedirs.version",
                                    ),
                                    (
                                        SeparateDirs::None,
                                        "settings.client.gamedir.separatedirs.none",
                                    ),
                                ] {
                                    if ui
                                        .selectable_value(
                                            &mut self.settings.separate_game_directories,
                                            mode,
                                            tr(key),
                                        )
                                        .changed()
                                    {
                                        self.persist();
                                    }
                                }
                            });

                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.client.resolution.label"));
                            let mut changed = false;
                            changed |= ui
                                .add_sized(
                                    Vec2::new(270.0, 28.0),
                                    egui::DragValue::new(&mut self.settings.screen_width)
                                        .range(320..=7680),
                                )
                                .changed();
                            ui.label("×");
                            changed |= ui
                                .add_sized(
                                    Vec2::new(270.0, 28.0),
                                    egui::DragValue::new(&mut self.settings.screen_height)
                                        .range(240..=4320),
                                )
                                .changed();
                            if changed {
                                self.persist();
                            }
                        });
                        if ui
                            .checkbox(
                                &mut self.settings.fullscreen,
                                tr("settings.client.resolution.fullscreen"),
                            )
                            .changed()
                        {
                            self.persist();
                        }

                        ui.add_space(8.0);
                        let mut filters_changed = false;
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.versions.label"));
                            ui.columns(2, |columns| {
                                filters_changed |= columns[0]
                                    .checkbox(
                                        &mut self.settings.version_include_remote,
                                        tr("settings.versions.sub.remote"),
                                    )
                                    .changed();
                                for (kind, key) in [
                                    ("modified", "settings.versions.modified"),
                                    ("old-alpha", "settings.versions.old_alpha"),
                                    ("pending", "settings.versions.pending"),
                                ] {
                                    let value = self
                                        .settings
                                        .version_catalog_types
                                        .entry(kind.to_owned())
                                        .or_insert_with(|| default_version_type_enabled(kind));
                                    filters_changed |= columns[0].checkbox(value, tr(key)).changed();
                                }
                                for (kind, key) in [
                                    ("snapshot", "settings.versions.snapshot"),
                                    ("old-beta", "settings.versions.old_beta"),
                                    ("launcher", "settings.versions.launcher"),
                                ] {
                                    let value = self
                                        .settings
                                        .version_catalog_types
                                        .entry(kind.to_owned())
                                        .or_insert_with(|| default_version_type_enabled(kind));
                                    filters_changed |= columns[1].checkbox(value, tr(key)).changed();
                                }
                                filters_changed |= columns[1]
                                    .checkbox(
                                        &mut self.settings.version_show_old_releases,
                                        tr("settings.versions.sub.old_release"),
                                    )
                                    .changed();
                                filters_changed |= columns[0]
                                    .checkbox(
                                        &mut self.settings.version_only_installed,
                                        tr("settings.versions.only-installed"),
                                    )
                                    .changed();
                            });
                        });
                        if filters_changed {
                            self.persist();
                        }

                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.jre.type.label"));
                            ComboBox::from_id_salt("minecraft_java_mode")
                                .selected_text(tr(match self.settings.java_mode.as_str() {
                                    "current" => "settings.jre.window.select.current",
                                    "custom" => "settings.jre.window.select.custom",
                                    _ => "settings.jre.type.recommended",
                                }))
                                .show_ui(ui, |ui| {
                                    for (mode, key) in [
                                        ("recommended", "settings.jre.type.recommended"),
                                        ("current", "settings.jre.window.select.current"),
                                        ("custom", "settings.jre.type.custom"),
                                    ] {
                                        if ui
                                            .selectable_value(
                                                &mut self.settings.java_mode,
                                                mode.to_owned(),
                                                tr(key),
                                            )
                                            .changed()
                                        {
                                            if mode == "recommended" {
                                                self.settings.java_path.clear();
                                            }
                                            self.persist();
                                        }
                                    }
                                });
                            ui.menu_button(tr("settings.jre.button.customize"), |ui| {
                                ui.set_min_width(360.0);
                                match self.settings.java_mode.as_str() {
                                    "current" => {
                                        ui.label(tr("settings.jre.window.select.current.hint"));
                                        if ui.button(tr("rust.settings.java.check-current")).clicked() {
                                            match detect_java() {
                                                Ok((path, version)) => self.set_logged_status(
                                                    LogLevel::Info,
                                                    LogTarget::JavaRuntime,
                                                    tr("rust.status.java.found-current")
                                                        .replace("%0", &version)
                                                        .replace("%1", &path.display().to_string()),
                                                ),
                                                Err(error) => self.set_logged_status(
                                                    LogLevel::Error,
                                                    LogTarget::JavaRuntime,
                                                    error,
                                                ),
                                            }
                                        }
                                    }
                                    "custom" => {
                                        ui.label(tr(
                                            "settings.jre.window.configure.path.custom.placeholder.windows",
                                        ));
                                        if ui.text_edit_singleline(&mut self.settings.java_path).changed() {
                                            self.persist();
                                        }
                                        if ui.button(tr("rust.settings.java.detect")).clicked() {
                                            match detect_java() {
                                                Ok((path, version)) => {
                                                    self.settings.java_path =
                                                        path.to_string_lossy().into_owned();
                                                    self.persist();
                                                    self.set_logged_status(
                                                        LogLevel::Info,
                                                        LogTarget::JavaRuntime,
                                                        tr("rust.status.java.found")
                                                            .replace("%0", &version)
                                                            .replace("%1", &path.display().to_string()),
                                                    );
                                                }
                                                Err(error) => self.set_logged_status(
                                                    LogLevel::Error,
                                                    LogTarget::JavaRuntime,
                                                    error,
                                                ),
                                            }
                                        }
                                    }
                                    _ => {
                                        ui.label(tr("settings.jre.window.select.recommended.hint"));
                                    }
                                }
                                ui.separator();
                                ui.label(tr("settings.jre.window.configure.jvm-args"));
                                if ui.text_edit_singleline(&mut self.settings.jvm_args).changed() {
                                    self.persist();
                                }
                                ui.label(tr("settings.jre.window.configure.mc-args"));
                                if ui.text_edit_singleline(&mut self.settings.game_args).changed() {
                                    self.persist();
                                }
                            });
                        });

                        ui.add_space(8.0);
                        let total_memory_mb = diagnostics::total_physical_memory_mb()
                            .unwrap_or(8_192)
                            .min(u64::from(u32::MAX)) as u32;
                        let release_time = self.version_release_time(&self.settings.selected_version);
                        let recommended_memory = legacy_auto_xmx_mib(
                            &self.settings.selected_version,
                            release_time.as_deref(),
                            total_memory_mb,
                            usize::BITS >= 64,
                            matches!(self.settings.loader_type.as_str(), "forge" | "neoforge"),
                        );
                        let saved_memory = parse_memory_size(&self.settings.max_memory)
                            .map(|bytes| (bytes / 1_048_576).min(u64::from(u32::MAX)) as u32)
                            .unwrap_or(4_096);
                        let mut displayed_memory = if self.settings.automatic_memory {
                            recommended_memory
                        } else {
                            saved_memory
                        };
                        let mut memory_changed = false;
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.java.memory.label"));
                            memory_changed |= ui
                                .add_enabled(
                                    !self.settings.automatic_memory,
                                    egui::Slider::new(&mut displayed_memory, 512..=16_384)
                                        .step_by(256.0),
                                )
                                .changed();
                            memory_changed |= ui
                                .add_enabled(
                                    !self.settings.automatic_memory,
                                    egui::DragValue::new(&mut displayed_memory)
                                        .range(512..=65_536)
                                        .speed(256),
                                )
                                .changed();
                            ui.label(tr("settings.java.memory.unit.mib"));
                            if ui
                                .checkbox(
                                    &mut self.settings.automatic_memory,
                                    tr("settings.java.memory.auto"),
                                )
                                .changed()
                            {
                                self.persist();
                            }
                        });
                        if memory_changed {
                            self.settings.max_memory = format!("{displayed_memory}M");
                            self.settings.min_memory = format!("{}M", (displayed_memory / 2).max(128));
                            self.persist();
                        }
                        ui.horizontal(|ui| {
                            ui.label("ⓘ");
                            ui.label(
                                tr("settings.java.memory.status.result.confident")
                                    .replace("%0", &self.settings.selected_version),
                            );
                        });

                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label(tr("settings.extra.label"));
                            if ui
                                .checkbox(
                                    &mut self.settings.promoted_servers_in_game,
                                    tr("settings.promotion.ingame"),
                                )
                                .changed()
                            {
                                self.persist();
                            }
                        });
                    }
                    "Manage versions" => {
                        self.show_version_manager(ui);
                    }
                    "Manage accounts" => {
                        return_home = self.draw_account_manager(ui);
                    }
                    _ => {}
                });
                    if title != "Manage accounts" {
                    ui.separator();
                    ui.horizontal(|ui| {
                        let home_width = 52.0;
                        let footer_width = (ui.available_width() - home_width - ui.spacing().item_spacing.x)
                            .max(120.0);
                        ui.vertical(|ui| {
                            if ui
                                .add_sized(
                                    Vec2::new(footer_width, 28.0),
                                    egui::Button::new(tr("settings.save")).min_size(Vec2::ZERO),
                                )
                                .clicked()
                            {
                                self.persist();
                                return_home = true;
                            }
                            if ui
                                .add_sized(
                                    Vec2::new(footer_width, 28.0),
                                    egui::Button::new(tr("settings.default")),
                                )
                                .clicked()
                            {
                                self.show_settings_reset_confirmation = true;
                            }
                        });
                        if ui
                            .add_sized(
                                Vec2::new(home_width, 56.0),
                                egui::Button::new(RichText::new("⌂").size(28.0)),
                            )
                            .on_hover_text(tr("explorer.button.home.tip"))
                            .clicked()
                        {
                            return_home = true;
                        }
                    });
                    }
                });
            if let Some(page) = navigate_to {
                self.settings_page = Some(page);
            }
            if return_home {
                open = false;
            }
            if let Some(notices::NoticeAction::Url { url, .. }) = about_action {
                self.open_url_with_status(&url, LogTarget::Launcher);
            }
            if logger_mode_changed {
                self.persist();
                if self.settings.logger_mode == "global" {
                    self.show_log_window = true;
                    self.show_live_launcher_log = true;
                    self.show_live_game_log = false;
                } else if self.show_live_game_log || self.show_live_launcher_log {
                    self.show_log_window = false;
                    self.show_live_launcher_log = false;
                    self.show_live_game_log = false;
                } else {
                    self.show_live_launcher_log = false;
                }
            }
            if browse_game_directory {
                let current_directory = self.game_dir_path();
                let mut dialog =
                    rfd::FileDialog::new().set_title(tr("settings.client.gamedir.prompt"));
                if current_directory.is_dir() {
                    dialog = dialog.set_directory(current_directory);
                }
                if let Some(selected) = dialog.pick_folder() {
                    if selected.is_dir() && fs::read_dir(&selected).is_ok() {
                        self.settings.game_directory = selected
                            .canonicalize()
                            .unwrap_or(selected)
                            .to_string_lossy()
                            .into_owned();
                        self.persist();
                    } else {
                        self.set_logged_status(
                            LogLevel::Error,
                            LogTarget::Settings,
                            tr("settings.client.gamedir.noaccess"),
                        );
                    }
                }
            }
            if browse_background_file {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title(tr("settings.slide.list.prompt.image"))
                    .add_filter(
                        tr("rust.settings.background.filter"),
                        &["jpg", "jpeg", "png"],
                    )
                    .pick_file()
                {
                    match load_texture_from_path(&ctx, "background", &path) {
                        Ok(background) => {
                            self.settings.background_path = path.to_string_lossy().into_owned();
                            self.background_custom_loaded = true;
                            if let Some(assets) = &mut self.assets {
                                assets.background = background;
                            }
                            self.persist();
                        }
                        Err(error) => {
                            self.set_logged_status(
                                LogLevel::Error,
                                LogTarget::Settings,
                                tr("rust.status.background.load-error").replace("%0", &error),
                            );
                        }
                    }
                }
            }
            if browse_theme_file {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title(tr("rust.settings.theme.file"))
                    .add_filter(tr("rust.settings.theme.filter"), &["properties"])
                    .pick_file()
                {
                    match fs::read(&path) {
                        Ok(_) => {
                            self.settings.legacy_theme_path = path
                                .canonicalize()
                                .unwrap_or(path)
                                .to_string_lossy()
                                .into_owned();
                            self.persist();
                        }
                        Err(error) => self.set_logged_status(
                            LogLevel::Error,
                            LogTarget::Settings,
                            tr("rust.status.theme.load-error").replace("%0", &error.to_string()),
                        ),
                    }
                }
            }
            if !open {
                self.settings_page = None;
                if title == "Manage accounts" {
                    self.account_manager_view = AccountManagerView::Overview;
                }
            }
        }
    }
}
