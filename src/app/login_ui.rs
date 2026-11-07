use super::*;

impl LauncherApp {
    pub(in crate::app) fn draw_login_panel(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &Context,
        toolbar_popup_opened_this_frame: &mut bool,
    ) {
        let Some(assets) = &self.assets else { return };
        let background = assets.background.clone();
        let support_icon = assets.support.clone();
        let folder_icon = assets.folder.clone();
        let refresh_icon = assets.refresh.clone();
        let settings_icon = assets.settings.clone();
        let login_panel_fill = self
            .legacy_theme_palette
            .as_ref()
            .and_then(|palette| palette.panel_background)
            .unwrap_or(Color32::from_rgba_unmultiplied(46, 67, 78, 194));
        let login_panel_border = self
            .legacy_theme_palette
            .as_ref()
            .and_then(|palette| palette.border_main.or(palette.border));
        let login_panel_border_size = self
            .legacy_theme_palette
            .as_ref()
            .and_then(|palette| palette.border_size)
            .unwrap_or(1.0);
        let login_panel_arc = self
            .legacy_theme_palette
            .as_ref()
            .and_then(|palette| palette.arc_main.or(palette.arc));
        let login_panel_shadow = self.legacy_theme_palette.as_ref().and_then(|palette| {
            palette.shadow_for(palette.shadow_main, palette.border_main.or(palette.border))
        });
        let default_panel_shadow = ctx.style_of(ctx.theme()).visuals.window_shadow;
        let icon_color = |name: &str| {
            self.legacy_theme_palette
                .as_ref()
                .and_then(|palette| palette.icon_color(name))
        };
        let support_icon_tint = icon_color("comments-o");
        let folder_icon_tint = icon_color("folder-open");
        let refresh_icon_tint = icon_color("refresh");
        let cancel_icon_tint = icon_color("remove");
        let settings_icon_tint = icon_color("bars");
        let account_menu_label = self.selected_account_label();
        let selected_version = self.settings.selected_version.clone();
        let install_label = if self.launching {
            tr("rust.status.game.installing-launching")
        } else if self.version_is_installed() {
            tr("loginform.enter")
        } else {
            tr("loginform.enter.install")
        };

        let rect = ui.max_rect();
        let uv = cover_uv(background.size_vec2(), rect.size());
        ui.painter()
            .image(background.id(), rect, uv, Color32::WHITE);
        ui.painter()
            .rect_filled(rect, 0.0, Color32::from_black_alpha(68));

        let screen = ctx.content_rect();
        let ui_scale = self.settings.launcher_font_size / 12.0;
        let login_size = JAVA_LOGIN_FORM_SIZE * ui_scale;
        let paired_settings = matches!(
            self.settings_page,
            Some("Minecraft" | "Launcher" | "About us")
        );
        let manager_scene = matches!(
            self.settings_page,
            Some("Manage accounts" | "Manage versions")
        );
        let paired_layout = paired_settings.then(|| {
            java_scene_layout(
                screen,
                &self.settings.login_form_direction,
                login_size,
                JAVA_SETTINGS_PANEL_SIZE * ui_scale,
            )
        });
        let show_login = !manager_scene
            && paired_layout
                .as_ref()
                .is_none_or(|layout| layout.show_login);
        if !show_login {
            return;
        }
        let login_position = paired_layout
            .map(|layout| layout.login_top_left)
            .unwrap_or_else(|| {
                java_login_position(screen, &self.settings.login_form_direction, login_size)
            });

        egui::Area::new(Id::unique("legacy_login_panel"))
            // The Java launcher keeps the login form in the default scene layer.
            // Dialogs and scene overlays must be able to draw above it.
            .order(SCENE_LAYER)
            .pivot(Align2::LEFT_TOP)
            .fixed_pos(login_position)
            .show(ctx, |ui| {
                let mut panel_frame = egui::Frame::new()
                    .fill(login_panel_fill)
                    .stroke(Stroke::new(
                        login_panel_border_size,
                        login_panel_border.unwrap_or(Color32::from_white_alpha(16)),
                    ))
                    .inner_margin(Margin::symmetric(24, 24));
                if let Some(radius) = login_panel_arc {
                    panel_frame = panel_frame.corner_radius(radius);
                }
                if let Some(color) = login_panel_shadow {
                    panel_frame = panel_frame.shadow(egui::Shadow {
                        color,
                        ..default_panel_shadow
                    });
                }
                panel_frame.show(ui, |ui| {
                    // Java's 285 px outer form includes the same 24 px side margins.
                    ui.set_width(JAVA_LOGIN_CONTENT_WIDTH);
                    let line_rect = ui
                        .allocate_exact_size(
                            Vec2::new(JAVA_LOGIN_CONTENT_WIDTH, 1.0),
                            Sense::hover(),
                        )
                        .0;
                    ui.painter().line_segment(
                        [line_rect.left_top(), line_rect.right_top()],
                        Stroke::new(1.0, Color32::from_white_alpha(110)),
                    );
                    ui.add_space(5.0);

                    ComboBox::from_id_salt("account_selector")
                        .width(JAVA_LOGIN_CONTENT_WIDTH)
                        .selected_text(RichText::new(account_menu_label).size(12.0))
                        .show_ui(ui, |ui| {
                            if self.settings.accounts.is_empty()
                                && self.settings.microsoft_accounts.is_empty()
                                && self.settings.ely_accounts.is_empty()
                                && self.settings.ely_legacy_accounts.is_empty()
                            {
                                ui.label(tr("account.empty"));
                            }
                            for account in &self.settings.accounts {
                                if ui
                                    .selectable_value(
                                        &mut self.settings.selected_account,
                                        Some(account.clone()),
                                        account,
                                    )
                                    .changed()
                                {
                                    self.persist();
                                }
                            }
                            for account in &self.settings.microsoft_accounts {
                                if ui
                                    .selectable_value(
                                        &mut self.settings.selected_account,
                                        Some(account.id.clone()),
                                        &account.username,
                                    )
                                    .changed()
                                {
                                    self.persist();
                                }
                            }
                            for account in &self.settings.ely_accounts {
                                if ui
                                    .selectable_value(
                                        &mut self.settings.selected_account,
                                        Some(account.id.clone()),
                                        tr("rust.ui.account.ely-label")
                                            .replace("%0", &account.display_name),
                                    )
                                    .changed()
                                {
                                    self.persist();
                                }
                            }
                            for account in &self.settings.ely_legacy_accounts {
                                if ui
                                    .selectable_value(
                                        &mut self.settings.selected_account,
                                        Some(account.id.clone()),
                                        tr("rust.ui.account.ely-legacy-label")
                                            .replace("%0", &account.display_name),
                                    )
                                    .changed()
                                {
                                    self.persist();
                                }
                            }
                            ui.separator();
                            if ui.button(tr("rust.ui.account.add-offline")).clicked() {
                                self.open_account_dialog();
                            }
                            if ui
                                .add_enabled(
                                    !self.microsoft_signin_pending
                                        && !self.ely_signin_pending
                                        && !self.ely_legacy_signin_pending
                                        && !self.launching,
                                    egui::Button::new(tr("rust.ui.account.add-microsoft")),
                                )
                                .clicked()
                            {
                                self.start_microsoft_sign_in();
                            }
                            if ui
                                .add_enabled(
                                    !self.microsoft_signin_pending
                                        && !self.ely_signin_pending
                                        && !self.ely_legacy_signin_pending
                                        && !self.launching,
                                    egui::Button::new(tr("rust.ui.account.add-ely")),
                                )
                                .clicked()
                            {
                                self.open_ely_dialog();
                            }
                            if ui
                                .add_enabled(
                                    !self.microsoft_signin_pending
                                        && !self.ely_signin_pending
                                        && !self.ely_legacy_signin_pending
                                        && !self.launching,
                                    egui::Button::new(tr("rust.ui.account.add-ely-legacy")),
                                )
                                .clicked()
                            {
                                self.open_ely_legacy_dialog();
                            }
                            ui.separator();
                            if ui
                                .button(tr("account.manager.multipane.add-account.hint"))
                                .clicked()
                            {
                                self.show_account_help = true;
                            }
                            if ui
                                .button(tr("account.manager.multipane.add-account.buy-minecraft"))
                                .clicked()
                            {
                                self.open_url_with_status(
                                    "https://minecraft.net/store/minecraft",
                                    LogTarget::Authentication,
                                );
                            }
                            if ui.button(tr("loginform.button.settings.account")).clicked() {
                                self.account_manager_view = AccountManagerView::Overview;
                                self.settings_page = Some("Manage accounts");
                            }
                        });

                    ComboBox::from_id_salt("version_selector")
                        .width(JAVA_LOGIN_CONTENT_WIDTH)
                        .selected_text(RichText::new(selected_version).size(12.0))
                        .show_ui(ui, |ui| {
                            for version in self.available_versions() {
                                if ui
                                    .selectable_value(
                                        &mut self.settings.selected_version,
                                        version.clone(),
                                        &version,
                                    )
                                    .changed()
                                {
                                    self.persist();
                                }
                            }
                            ui.separator();
                            if ui.button(tr("loginform.button.settings.version")).clicked() {
                                self.settings_page = Some("Manage versions");
                            }
                        });

                    ui.add_space(5.0);
                    if ui
                        .checkbox(
                            &mut self.settings.auto_login,
                            tr("loginform.checkbox.autologin"),
                        )
                        .changed()
                    {
                        if self.settings.auto_login {
                            self.show_autologin_tip = true;
                        } else {
                            self.auto_launch_deadline = None;
                        }
                        self.persist();
                    }
                    let force_update_label = if self.force_update_easter_egg {
                        "This ain't easter egg."
                    } else {
                        tr("loginform.checkbox.forceupdate")
                    };
                    if ui
                        .checkbox(&mut self.settings.force_update, force_update_label)
                        .changed()
                    {
                        self.force_update_toggle_count =
                            self.force_update_toggle_count.saturating_add(1);
                        if self.force_update_toggle_count == 10 {
                            self.force_update_toggle_count = 0;
                            self.force_update_easter_egg = true;
                        }
                        self.persist();
                    }
                    ui.add_space(3.0);

                    let auto_login_remaining = self
                        .auto_launch_deadline
                        .map(|deadline| deadline.saturating_duration_since(Instant::now()))
                        .map(|remaining| remaining.as_secs_f64().ceil().clamp(1.0, 10.0) as u8);
                    let play_label = auto_login_remaining
                        .map(|seconds| tr("loginform.cancel").replace("%0", &seconds.to_string()))
                        .unwrap_or_else(|| install_label.to_owned());
                    let play_button =
                        egui::Button::new(RichText::new(play_label).size(18.0).strong())
                            .fill(Color32::from_rgb(73, 78, 80))
                            .stroke(Stroke::new(1.0, Color32::from_white_alpha(36)))
                            .corner_radius(2.0);
                    if ui
                        .add_enabled(
                            !self.launching,
                            play_button.min_size(Vec2::new(JAVA_LOGIN_CONTENT_WIDTH, 46.0)),
                        )
                        .clicked()
                    {
                        if self.auto_launch_deadline.take().is_some() {
                            self.settings.auto_login = false;
                            self.persist();
                        } else {
                            self.start_selected_game();
                        }
                    }

                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        let support = icon_button(ui, &support_icon, support_icon_tint);
                        if support.clicked() {
                            let opening = !self.show_support_menu;
                            self.show_support_menu = opening;
                            if opening {
                                self.show_folder_menu = false;
                                self.show_settings_menu = false;
                                *toolbar_popup_opened_this_frame = true;
                            }
                            self.support_menu_pos =
                                support.rect.left_bottom() + Vec2::new(0.0, 1.0);
                        }
                        support.on_hover_text(tr("loginform.button.support"));

                        if !self.settings.settings_blocked {
                            let folder = icon_button(ui, &folder_icon, folder_icon_tint);
                            if folder.clicked() {
                                if self.version_is_installed() {
                                    let opening = !self.show_folder_menu;
                                    self.show_folder_menu = opening;
                                    if opening {
                                        self.show_support_menu = false;
                                        self.show_settings_menu = false;
                                        *toolbar_popup_opened_this_frame = true;
                                    }
                                    self.folder_menu_pos =
                                        folder.rect.left_bottom() + Vec2::new(0.0, 1.0);
                                } else {
                                    self.show_folder_menu = false;
                                    let path = self.game_dir_path();
                                    self.open_path_with_status(&path, LogTarget::Launcher);
                                }
                            }
                            folder.on_hover_text(tr("loginform.button.folder"));
                        }

                        if self.version_catalog_loading {
                            let cancel = cancel_icon_button(ui, cancel_icon_tint);
                            if cancel.clicked() {
                                self.cancel_version_catalog_fetch();
                            }
                            cancel.on_hover_text(tr("loginform.button.refresh-cancel"));
                        } else {
                            let refresh = icon_button(ui, &refresh_icon, refresh_icon_tint);
                            if refresh.clicked() {
                                let count = self.installed_versions().len();
                                self.refresh_version_catalog();
                                self.set_logged_status(
                                    LogLevel::Info,
                                    LogTarget::VersionCatalog,
                                    tr("rust.status.catalog.refreshed-local")
                                        .replace("%0", &count.to_string()),
                                );
                            }
                            refresh.on_hover_text(tr("loginform.button.refresh"));
                        }

                        if !self.settings.settings_blocked {
                            let settings = icon_button(ui, &settings_icon, settings_icon_tint);
                            if settings.clicked() {
                                let opening = !self.show_settings_menu;
                                self.show_settings_menu = opening;
                                if opening {
                                    self.show_support_menu = false;
                                    self.show_folder_menu = false;
                                    *toolbar_popup_opened_this_frame = true;
                                }
                                self.settings_menu_pos =
                                    settings.rect.left_bottom() + Vec2::new(0.0, 1.0);
                            }
                            if !self.show_settings_menu {
                                settings.on_hover_text(tr("loginform.button.settings.launcher"));
                            }
                        }
                    });
                });
            });
    }
}
