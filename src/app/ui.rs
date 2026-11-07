use super::*;

impl eframe::App for LauncherApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut toolbar_popup_opened_this_frame = false;
        self.process_frame_events(&ctx);
        self.draw_login_panel(ui, &ctx, &mut toolbar_popup_opened_this_frame);

        if let Some(notification) = self.bootstrap_notification.clone() {
            let notification_texture = self.bootstrap_notification_texture.clone();
            let clicked = egui::Area::new(Id::unique((
                "bootstrap_notification",
                notification.id.as_str(),
            )))
            .anchor(Align2::RIGHT_TOP, Vec2::new(-12.0, 12.0))
            .order(OVERLAY_LAYER)
            .show(&ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .inner_margin(Margin::same(6))
                    .show(ui, |ui| {
                        let response = if let Some(texture) = notification_texture {
                            ui.add(
                                egui::Image::new((texture.id(), Vec2::splat(36.0)))
                                    .sense(Sense::click()),
                            )
                        } else {
                            ui.add_sized(Vec2::splat(36.0), egui::Button::new("!"))
                        };
                        response
                            .on_hover_text(tr("bootstrap.notification.open"))
                            .clicked()
                    })
                    .inner
            })
            .inner;
            if clicked {
                if self.open_url_with_status(&notification.url, LogTarget::Bootstrap) {
                    self.bootstrap_notification = None;
                    self.bootstrap_notification_texture = None;
                }
            }
        }

        if self.show_folder_menu {
            let popup = egui::Area::new(Id::unique("folder_popup"))
                .order(OVERLAY_LAYER)
                .fixed_pos(self.folder_menu_pos)
                .show(&ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(220.0);
                        if self.settings.separate_game_directories != SeparateDirs::None {
                            let folder_name = self
                                .game_root_for_version(&self.settings.selected_version)
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_else(|| self.settings.selected_version.clone());
                            let label =
                                tr("loginform.button.folder.family").replace("%0", &folder_name);
                            if ui.button(label).clicked() {
                                let path =
                                    self.game_root_for_version(&self.settings.selected_version);
                                self.open_path_with_status(&path, LogTarget::Launcher);
                                self.show_folder_menu = false;
                            }
                        }
                        let version_id = self.settings.selected_version.to_ascii_lowercase();
                        if (version_id.contains("forge") || version_id.contains("fabric"))
                            && ui.button(tr("loginform.button.folder.mods")).clicked()
                        {
                            let mods = self
                                .game_data_directory_for_version(&self.settings.selected_version)
                                .join("mods");
                            match fs::create_dir_all(&mods) {
                                Ok(()) => self.open_path_with_status(&mods, LogTarget::Mods),
                                Err(error) => {
                                    self.set_logged_status(
                                        LogLevel::Error,
                                        LogTarget::Mods,
                                        tr("rust.status.game.mods-folder-error")
                                            .replace("%0", &error.to_string()),
                                    );
                                }
                            }
                            self.show_folder_menu = false;
                        }
                        if ui.button(tr("loginform.button.folder.root")).clicked() {
                            let path = self.game_dir_path();
                            self.open_path_with_status(&path, LogTarget::Launcher);
                            self.show_folder_menu = false;
                        }
                    });
                });
            if !toolbar_popup_opened_this_frame
                && toolbar_popup_should_close(&ctx, popup.response.rect)
            {
                self.show_folder_menu = false;
            }
        }

        if self.show_support_menu {
            let popup = egui::Area::new(Id::unique("support_popup"))
                .order(OVERLAY_LAYER)
                .fixed_pos(self.support_menu_pos)
                .show(&ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(220.0);
                        if ui.button(tr("loginform.button.support.discord")).clicked() {
                            if self.open_url_with_status(
                                "https://llaun.ch/discord/intl",
                                LogTarget::Diagnostics,
                            ) {
                                self.show_support_menu = false;
                            }
                        }
                        ui.separator();
                        if ui.button(tr("loginform.button.support")).clicked() {
                            self.open_support_flow();
                            self.show_support_menu = false;
                        }
                    });
                });
            if !toolbar_popup_opened_this_frame
                && toolbar_popup_should_close(&ctx, popup.response.rect)
            {
                self.show_support_menu = false;
            }
        }

        if self.show_support_window {
            let mut open = true;
            egui::Window::new(tr("support.contact.title"))
                .order(OVERLAY_LAYER)
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    ui.label(tr("support.contact.body"));
                    ui.horizontal(|ui| {
                        if ui.button(tr("support.contact.buttons.discord")).clicked() {
                            self.open_url_with_status(
                                "https://llaun.ch/support/discord/intl",
                                LogTarget::Diagnostics,
                            );
                        }
                        if ui.button(tr("support.contact.buttons.mail")).clicked() {
                            self.open_url_with_status(
                                "mailto:support@llaun.ch",
                                LogTarget::Diagnostics,
                            );
                        }
                    });
                });
            self.show_support_window = open;
        }

        if self.show_diagnostic_consent
            || self.diagnostic_pending
            || self.diagnostic_result_url.is_some()
            || self.diagnostic_error.is_some()
        {
            let mut open = true;
            let diagnostic_result_url = self.diagnostic_result_url.clone();
            let diagnostic_error = self.diagnostic_error.clone();
            let title = if self.diagnostic_pending {
                tr("support.sending.title")
            } else if diagnostic_result_url.is_some() {
                tr("support.sending.success.title")
            } else if diagnostic_error.is_some() {
                tr("support.sending.error.title")
            } else {
                tr("support.pre.title")
            };
            egui::Window::new(title)
                .order(OVERLAY_LAYER)
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    if self.diagnostic_pending {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(tr("support.sending.head"));
                        });
                    } else if let Some(url) = &diagnostic_result_url {
                        ui.label(tr("support.sending.success"));
                        ui.hyperlink(url);
                        ui.horizontal(|ui| {
                            if ui.button(tr("support.sending.open")).clicked() {
                                self.open_url_with_status(url, LogTarget::Diagnostics);
                            }
                            if ui.button(tr("support.sending.copy")).clicked() {
                                ctx.copy_text(url.clone());
                            }
                            if ui.button(tr("support.button.close")).clicked() {
                                self.diagnostic_result_url = None;
                                self.diagnostic_payload = None;
                            }
                        });
                    } else if let Some(error) = &diagnostic_error {
                        ui.colored_label(Color32::LIGHT_RED, error);
                        ui.horizontal(|ui| {
                            if ui.button(tr("support.sending.save")).clicked() {
                                self.save_diagnostic_report();
                            }
                            if ui.button(tr("support.sending.retry")).clicked() {
                                self.start_diagnostic_upload();
                            }
                            if ui.button(tr("support.button.close")).clicked() {
                                self.diagnostic_error = None;
                                self.diagnostic_payload = None;
                            }
                        });
                    } else {
                        ui.label(tr("support.pre.body"));
                        if ui
                            .checkbox(
                                &mut self.diagnostic_send_enabled,
                                tr("support.pre.diag.checkbox"),
                            )
                            .changed()
                        {
                            // The user can opt out before continuing to the regular contact links.
                        }
                        if ui.button(tr("support.pre.diag.whatisit")).clicked() {
                            self.open_url_with_status(
                                &tr("support.pre.diag.url"),
                                LogTarget::Diagnostics,
                            );
                        }
                        ui.horizontal(|ui| {
                            if ui.button(tr("support.pre.continue")).clicked() {
                                self.show_diagnostic_consent = false;
                                if self.diagnostic_send_enabled {
                                    self.start_diagnostic_upload();
                                } else {
                                    self.show_support_window = true;
                                }
                            }
                            if ui.button(tr("support.button.cancel")).clicked() {
                                self.show_diagnostic_consent = false;
                            }
                        });
                    }
                });
            if !open {
                self.show_diagnostic_consent = false;
                self.diagnostic_result_url = None;
                self.diagnostic_error = None;
                if !self.diagnostic_pending {
                    self.diagnostic_payload = None;
                }
            }
        }

        if self.show_feedback_dialog {
            let feedback_url = self.bootstrap_feedback_url.clone();
            let mut open = true;
            let mut close_launcher = false;
            egui::Window::new(tr("feedback.title"))
                .order(OVERLAY_LAYER)
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    ui.label(tr("feedback.body"));
                    ui.horizontal(|ui| {
                        if ui.button(tr("feedback.button.okay")).clicked() {
                            close_launcher = feedback_url.as_ref().is_none_or(|url| {
                                self.open_url_with_status(url, LogTarget::Bootstrap)
                            });
                        }
                        if ui.button(tr("feedback.button.no")).clicked() {
                            close_launcher = true;
                        }
                    });
                });
            if !open || close_launcher {
                self.show_feedback_dialog = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        if self.show_settings_menu {
            let popup = egui::Area::new(Id::unique("settings_popup"))
                .order(OVERLAY_LAYER)
                .fixed_pos(self.settings_menu_pos)
                .show(&ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(220.0);
                        if ui
                            .button(tr("loginform.button.settings.launcher"))
                            .clicked()
                        {
                            self.settings_page = Some("Minecraft");
                            self.show_settings_menu = false;
                        }
                        if ui.button(tr("loginform.button.settings.version")).clicked() {
                            self.settings_page = Some("Manage versions");
                            self.show_settings_menu = false;
                        }
                        if ui.button(tr("loginform.button.settings.account")).clicked() {
                            self.account_manager_view = AccountManagerView::Overview;
                            self.settings_page = Some("Manage accounts");
                            self.show_settings_menu = false;
                        }
                    });
                });
            if !toolbar_popup_opened_this_frame
                && toolbar_popup_should_close(&ctx, popup.response.rect)
            {
                self.show_settings_menu = false;
            }
        }

        if self.show_update_dialog {
            if let Some(release) = self.available_release.clone() {
                let mut open = true;
                let mut close = false;
                let mut start_install = false;
                let install_error = self.update_install_error.clone();
                egui::Window::new(tr("update.title"))
                    .order(OVERLAY_LAYER)
                    .open(&mut open)
                    .collapsible(false)
                    .resizable(true)
                    .default_width(460.0)
                    .show(&ctx, |ui| {
                        ui.heading(tr("update.head").replace("%0", &release.tag_name));
                        if !release.name.is_empty() {
                            ui.label(&release.name);
                        }
                        ui.separator();
                        if self.update_installing {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(tr("update.downloading"));
                            });
                        }
                        if let Some(error) = &install_error {
                            ui.colored_label(Color32::LIGHT_RED, error);
                        }
                        if release.body.trim().is_empty() {
                            ui.label(tr("update.notes.empty"));
                        } else {
                            egui::ScrollArea::vertical()
                                .max_height(300.0)
                                .show(ui, |ui| ui.label(&release.body));
                        }
                        ui.separator();
                        ui.horizontal(|ui| {
                            let can_install = release.executable_url.is_some()
                                && release.checksums_url.is_some()
                                && !self.update_installing;
                            if ui
                                .add_enabled(can_install, egui::Button::new(tr("update.install")))
                                .clicked()
                            {
                                start_install = true;
                            }
                            if ui.button(tr("update.download")).clicked() {
                                self.open_url_with_status(&release.html_url, LogTarget::Update);
                            }
                            if ui
                                .add_enabled(
                                    !self.update_installing,
                                    egui::Button::new(tr("update.footer.continue")),
                                )
                                .clicked()
                            {
                                close = true;
                            }
                        });
                    });
                if start_install {
                    self.start_update_install(release);
                }
                if (!open || close) && !self.update_installing {
                    self.show_update_dialog = false;
                }
            }
        }

        if self.settings.notices_enabled
            && self.settings.notice_side_panel_enabled
            && !self.show_notices_window
        {
            let visible = self.visible_notice_indices();
            if let Some(index) = visible
                .get(self.notice_index % visible.len().max(1))
                .copied()
            {
                let notice = self.notice_list[index].clone();
                let mut action = None;
                egui::Area::new(Id::unique("legacy_notice_card"))
                    .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-12.0, -48.0))
                    .order(SCENE_LAYER)
                    .show(&ctx, |ui| {
                        egui::Frame::popup(ui.style())
                            .inner_margin(Margin::same(12))
                            .show(ui, |ui| {
                                ui.set_max_width(330.0);
                                ui.horizontal(|ui| {
                                    ui.strong(if notice.promoted {
                                        tr("rust.ui.notices.suggested")
                                    } else {
                                        tr("rust.ui.notices.notice")
                                    });
                                    if ui.small_button(tr("rust.ui.notices.all")).clicked() {
                                        self.notice_scene_index = 0;
                                        self.show_notice_scene = true;
                                    }
                                });
                                ui.separator();
                                if let Some(texture) = self.notice_images.get(&notice.id) {
                                    let size = texture.size_vec2();
                                    if size.x > 0.0 && size.y > 0.0 {
                                        let scale = (300.0 / size.x).min(120.0 / size.y).min(1.0);
                                        ui.image((texture.id(), size * scale));
                                    }
                                }
                                render_notice_text(ui, &notice, true, &mut action);
                                ui.add_space(5.0);
                                ui.horizontal_wrapped(|ui| {
                                    if let Some(notice_action) = notice.action.clone() {
                                        let label = match &notice_action {
                                            notices::NoticeAction::Url { label, .. }
                                            | notices::NoticeAction::Scene { label, .. }
                                            | notices::NoticeAction::Launcher { label, .. }
                                            | notices::NoticeAction::Server { label, .. } => label,
                                        };
                                        if ui.button(label).clicked() {
                                            action = Some(notice_action);
                                        }
                                    }
                                    if ui.small_button(tr("rust.ui.notices.hide-week")).clicked() {
                                        self.hide_notice_for_week(&notice.id);
                                    }
                                    if visible.len() > 1
                                        && ui.small_button(tr("rust.ui.next")).clicked()
                                    {
                                        self.notice_index = self.notice_index.saturating_add(1);
                                    }
                                });
                            });
                    });
                if let Some(action) = action {
                    self.perform_notice_action(action, &ctx);
                }
            } else if self.notices_loading || self.bootstrap_loading {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        }

        if self.show_notices_window {
            let visible = self.visible_notice_indices();
            let mut open = true;
            let mut action = None;
            egui::Window::new(tr("rust.ui.window.notices"))
                .order(OVERLAY_LAYER)
                .open(&mut open)
                .default_size([540.0, 430.0])
                .resizable(true)
                .show(&ctx, |ui| {
                    let was_enabled = self.settings.notices_enabled;
                    let notices_changed = ui
                        .add_enabled(
                            self.notices_disable_allowed,
                            egui::Checkbox::new(
                                &mut self.settings.notices_enabled,
                                tr("rust.ui.notices.show"),
                            ),
                        )
                        .changed();
                    let mut changed = notices_changed;
                    if notices_changed && was_enabled != self.settings.notices_enabled {
                        self.refresh_notices();
                    }
                    changed |= ui
                        .checkbox(
                            &mut self.settings.promoted_notices_enabled,
                            tr("rust.ui.notices.show-promoted"),
                        )
                        .changed();
                    changed |= ui
                        .checkbox(
                            &mut self.settings.notice_side_panel_enabled,
                            tr("rust.ui.notices.show-launcher-panel"),
                        )
                        .changed();
                    if changed {
                        self.persist();
                    }
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                !self.notices_loading,
                                egui::Button::new(tr("loginform.button.refresh")),
                            )
                            .clicked()
                        {
                            self.refresh_notices();
                        }
                        if self.notices_loading || self.bootstrap_loading {
                            ui.spinner();
                            ui.label(tr("rust.ui.notices.loading"));
                        }
                        if ui.button(tr("rust.ui.notices.restore")).clicked() {
                            self.settings.hidden_notices_until.clear();
                            self.notice_index = 0;
                            self.persist();
                        }
                    });
                    if let Some(error) = &self.notice_error {
                        ui.colored_label(Color32::LIGHT_RED, error);
                    }
                    if let Some(error) = &self.bootstrap_error {
                        ui.colored_label(
                            Color32::LIGHT_RED,
                            tr("notices.bootstrap.error").replace("%0", error),
                        );
                    }
                    ui.separator();
                    if self.notice_list.is_empty()
                        && !self.notices_loading
                        && !self.bootstrap_loading
                    {
                        ui.label(tr("rust.ui.notices.empty"));
                    }
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for index in visible {
                            let Some(notice) = self.notice_list.get(index).cloned() else {
                                continue;
                            };
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.strong(if notice.promoted {
                                        tr("rust.ui.notices.suggested")
                                    } else {
                                        tr("rust.ui.notices.notice")
                                    });
                                    ui.label(
                                        tr("rust.ui.notices.id")
                                            .replace("%0", &notice.id.to_string()),
                                    );
                                    if ui.small_button(tr("rust.ui.notices.hide-week")).clicked() {
                                        self.hide_notice_for_week(&notice.id);
                                    }
                                });
                                if let Some(texture) = self.notice_images.get(&notice.id) {
                                    let size = texture.size_vec2();
                                    if size.x > 0.0 && size.y > 0.0 {
                                        let scale = (420.0 / size.x).min(200.0 / size.y).min(1.0);
                                        ui.image((texture.id(), size * scale));
                                    }
                                }
                                render_notice_text(ui, &notice, true, &mut action);
                                if let Some(notice_action) = notice.action.clone() {
                                    let label = match &notice_action {
                                        notices::NoticeAction::Url { label, .. }
                                        | notices::NoticeAction::Scene { label, .. }
                                        | notices::NoticeAction::Launcher { label, .. }
                                        | notices::NoticeAction::Server { label, .. } => label,
                                    };
                                    if ui.button(label).clicked() {
                                        action = Some(notice_action);
                                    }
                                }
                            });
                        }
                    });
                });
            self.show_notices_window = open;
            if let Some(action) = action {
                self.perform_notice_action(action, &ctx);
            }
        }

        if let Some(report) = self.connectivity_report.clone()
            && report.has_critical_failures()
        {
            egui::Area::new(Id::unique("connectivity_warning_badge"))
                .anchor(Align2::RIGHT_TOP, Vec2::new(-12.0, 58.0))
                .order(OVERLAY_LAYER)
                .show(&ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        if ui
                            .button(
                                RichText::new(format!("⚠ {}", tr("connectivity.warning.title")))
                                    .color(Color32::from_rgb(242, 195, 86)),
                            )
                            .clicked()
                        {
                            self.show_connectivity_window = true;
                        }
                    });
                });

            if self.show_connectivity_window {
                let mut open = true;
                let mut html_action = None;
                egui::Window::new(tr("connectivity.warning.title"))
                    .order(OVERLAY_LAYER)
                    .open(&mut open)
                    .default_size([500.0, 460.0])
                    .resizable(true)
                    .show(&ctx, |ui| {
                        if let Some(message) = report.doomsday_message.as_deref() {
                            render_notice_html(ui, message, true, &mut html_action);
                        } else {
                            let no_connection = report.all_services_unreachable();
                            let body_key = if no_connection {
                                "connectivity.warning.body.empty"
                            } else {
                                "connectivity.warning.body.text"
                            };
                            render_notice_html(ui, &tr(body_key), true, &mut html_action);
                            if ui.link(tr("connectivity.warning.body.link")).clicked() {
                                let url = if no_connection {
                                    "https://wiki.llaun.ch/en:connectivity:none"
                                } else {
                                    "https://wiki.llaun.ch/en:connectivity:some"
                                };
                                self.open_url_with_status(url, LogTarget::Connectivity);
                            }
                            ui.separator();
                            let official_repo_unavailable = report
                                .services
                                .iter()
                                .find(|service| service.id == "official_repo")
                                .is_some_and(|service| !service.reachable);
                            let official_repo_proxy_unavailable = report
                                .services
                                .iter()
                                .find(|service| service.id == "official_repo_proxy")
                                .is_some_and(|service| !service.reachable);
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                for service in report
                                    .services
                                    .iter()
                                    .filter(|service| !service.reachable)
                                {
                                    let (name_key, hint_key) = match service.id {
                                        "official_repo" => (
                                            "connectivity.warning.list.name.official_repo",
                                            if official_repo_proxy_unavailable {
                                                "connectivity.warning.list.hint.official_repo.not_ok"
                                            } else {
                                                "connectivity.warning.list.hint.official_repo.ok"
                                            },
                                        ),
                                        "official_repo_proxy" => (
                                            "connectivity.warning.list.name.official_repo_proxy",
                                            if official_repo_unavailable {
                                                "connectivity.warning.list.hint.official_repo_proxy.not_ok"
                                            } else {
                                                "connectivity.warning.list.hint.official_repo_proxy.ok"
                                            },
                                        ),
                                        "repo" => (
                                            "connectivity.warning.list.name.repo",
                                            "connectivity.warning.list.hint.repo",
                                        ),
                                        "boot" => (
                                            "connectivity.warning.list.name.boot",
                                            "connectivity.warning.list.hint.boot",
                                        ),
                                        "account.ely.by" => (
                                            "connectivity.warning.list.name.account.ely.by",
                                            "connectivity.warning.list.hint.account.ely.by",
                                        ),
                                        _ => (
                                            "connectivity.warning.list.name.web",
                                            "rust.connectivity.hint.official_repo",
                                        ),
                                    };
                                    ui.group(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.strong(tr(name_key));
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    ui.colored_label(
                                                        Color32::LIGHT_RED,
                                                        tr("connectivity.warning.list.status.unreachable"),
                                                    );
                                                },
                                            );
                                        });
                                        ui.label(tr(hint_key));
                                    });
                                }
                            });
                            let unavailable_hosts = report.unavailable_hosts();
                            if !unavailable_hosts.is_empty() {
                                egui::CollapsingHeader::new(tr("connectivity.warning.hosts.button"))
                                    .show(ui, |ui| {
                                        for host in unavailable_hosts {
                                            ui.monospace(host);
                                        }
                                    });
                            }
                        }
                    });
                self.show_connectivity_window = open;
                if let Some(notices::NoticeAction::Url { url, .. }) = html_action {
                    self.open_url_with_status(&url, LogTarget::Connectivity);
                }
            }
        }

        if self.show_notice_scene {
            let screen_rect = ctx.content_rect();
            let selected_notice = self
                .notice_list
                .get(self.notice_scene_index % self.notice_list.len().max(1))
                .cloned();
            let selected_is_hidden = selected_notice.as_ref().is_some_and(|notice| {
                self.settings
                    .hidden_notices_until
                    .get(&notice.id)
                    .is_some_and(|expiry| *expiry > unix_time_millis())
            });
            let mut go_home = false;
            let mut change_by = None;
            let mut toggle_notice = None;
            let mut action = None;
            egui::Area::new(Id::unique("legacy_notice_scene"))
                .order(OVERLAY_LAYER)
                .fixed_pos(screen_rect.min)
                .interactable(true)
                .show(&ctx, |ui| {
                    ui.set_min_size(screen_rect.size());
                    egui::Frame::new()
                        .fill(Color32::from_rgb(27, 31, 34))
                        .inner_margin(Margin::same(24))
                        .show(ui, |ui| {
                            ui.allocate_ui_with_layout(
                                screen_rect.size() - Vec2::splat(48.0),
                                egui::Layout::top_down(egui::Align::Center),
                                |ui| {
                                    ui.heading(tr("rust.ui.window.notices"));
                                    ui.add_space(10.0);
                                    ui.separator();
                                    ui.add_space(10.0);

                                    if let Some(notice) = &selected_notice {
                                        let content_width = (screen_rect.width() - 112.0).clamp(240.0, 760.0);
                                        ui.allocate_ui_with_layout(
                                            Vec2::new(content_width, ui.available_height()),
                                            egui::Layout::top_down(egui::Align::Center),
                                            |ui| {
                                                egui::Frame::popup(ui.style())
                                                    .inner_margin(Margin::same(18))
                                                    .show(ui, |ui| {
                                                        ui.set_min_width(content_width - 36.0);
                                                        ui.horizontal(|ui| {
                                                            ui.strong(if notice.promoted {
                                                                tr("rust.ui.notices.suggested")
                                                            } else {
                                                                tr("rust.ui.notices.notice")
                                                            });
                                                            ui.label(
                                                                tr("rust.ui.notices.id")
                                                                    .replace("%0", &notice.id),
                                                            );
                                                            if selected_is_hidden {
                                                                ui.label(
                                                                    RichText::new(tr(
                                                                        "rust.ui.notice-scene.hidden",
                                                                    ))
                                                                    .weak(),
                                                                );
                                                            }
                                                        });
                                                        ui.separator();
                                                        if let Some(texture) =
                                                            self.notice_images.get(&notice.id)
                                                        {
                                                            let size = texture.size_vec2();
                                                            if size.x > 0.0 && size.y > 0.0 {
                                                                let max_height = (screen_rect
                                                                    .height()
                                                                    - 300.0)
                                                                .clamp(96.0, 300.0);
                                                                let scale = (content_width / size.x)
                                                                    .min(max_height / size.y)
                                                                    .min(1.0);
                                                                ui.image((texture.id(), size * scale));
                                                            }
                                                        }
                                                        egui::ScrollArea::vertical()
                                                            .max_height((screen_rect.height() - 300.0).max(100.0))
                                                            .show(ui, |ui| {
                                                                ui.add_enabled_ui(
                                                                    !selected_is_hidden,
                                                                    |ui| {
                                                                        render_notice_text(
                                                                            ui,
                                                                            notice,
                                                                            true,
                                                                            &mut action,
                                                                        );
                                                                    },
                                                                );
                                                            });
                                                        if let Some(notice_action) =
                                                            notice.action.clone()
                                                        {
                                                            let label = match &notice_action {
                                                                notices::NoticeAction::Url {
                                                                    label, ..
                                                                }
                                                                | notices::NoticeAction::Scene {
                                                                    label, ..
                                                                }
                                                                | notices::NoticeAction::Launcher {
                                                                    label, ..
                                                                }
                                                                | notices::NoticeAction::Server {
                                                                    label, ..
                                                                } => label,
                                                            };
                                                            if ui
                                                                .add_enabled(
                                                                    !selected_is_hidden,
                                                                    egui::Button::new(label),
                                                                )
                                                                .clicked()
                                                            {
                                                                action = Some(notice_action);
                                                            }
                                                        }
                                                    });
                                            },
                                        );
                                    } else if self.notices_loading || self.bootstrap_loading {
                                        ui.spinner();
                                        ui.label(tr("rust.ui.notices.loading"));
                                        ctx.request_repaint_after(Duration::from_millis(250));
                                    } else {
                                        ui.label(tr("rust.ui.notices.empty"));
                                    }

                                    ui.with_layout(
                                        egui::Layout::bottom_up(egui::Align::Center),
                                        |ui| {
                                            ui.horizontal(|ui| {
                                                let notice_count = self.notice_list.len();
                                                if ui
                                                    .add_enabled(
                                                        notice_count > 1,
                                                        egui::Button::new(tr(
                                                            "rust.ui.notice-scene.previous",
                                                        )),
                                                    )
                                                    .clicked()
                                                {
                                                    change_by = Some(-1isize);
                                                }
                                                if ui
                                                    .add_enabled(
                                                        notice_count > 1,
                                                        egui::Button::new(tr(
                                                            "rust.ui.notice-scene.next",
                                                        )),
                                                    )
                                                    .clicked()
                                                {
                                                    change_by = Some(1isize);
                                                }
                                                if let Some(notice) = &selected_notice {
                                                    let key = if selected_is_hidden {
                                                        "rust.ui.notice-scene.restore"
                                                    } else {
                                                        "rust.ui.notice-scene.hide"
                                                    };
                                                    if ui.button(tr(key)).clicked() {
                                                        toggle_notice = Some(notice.id.clone());
                                                    }
                                                }
                                                if ui
                                                    .button(tr("rust.ui.notice-scene.home"))
                                                    .clicked()
                                                {
                                                    go_home = true;
                                                }
                                            });
                                        },
                                    );
                                },
                            );
                        });
                });
            if let Some(delta) = change_by {
                let count = self.notice_list.len();
                if count > 0 {
                    self.notice_scene_index = if delta < 0 {
                        (self.notice_scene_index + count - 1) % count
                    } else {
                        (self.notice_scene_index + 1) % count
                    };
                }
            }
            if let Some(id) = toggle_notice {
                self.toggle_notice_visibility_for_week(&id);
            }
            if go_home {
                self.show_notice_scene = false;
            }
            if let Some(action) = action {
                self.perform_notice_action(action, &ctx);
            }
        }

        if let Some(pending) = self.pending_promoted_server.clone() {
            let installed_versions: HashSet<String> =
                self.installed_versions().into_iter().collect();
            let mut open = true;
            let mut cancelled = false;
            let mut selected_version = None;
            egui::Window::new(tr("rust.ui.window.servers.choose-version"))
                .order(OVERLAY_LAYER)
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    ui.label(
                        tr("rust.ui.servers.choose-version").replace("%0", &pending.server.name),
                    );
                    if ui
                        .button(
                            tr("rust.ui.versions.current-version")
                                .replace("%0", &pending.current_version),
                        )
                        .clicked()
                    {
                        selected_version = Some(pending.current_version.clone());
                    }
                    for version in &pending.family_versions {
                        let suffix = if installed_versions.contains(version) {
                            format!(" ({})", tr("rust.ui.versions.installed"))
                        } else {
                            String::new()
                        };
                        if ui.button(format!("{version}{suffix}")).clicked() {
                            selected_version = Some(version.clone());
                        }
                    }
                    ui.separator();
                    if ui.button(tr("support.button.cancel")).clicked() {
                        cancelled = true;
                    }
                });
            if !open || cancelled {
                self.pending_promoted_server = None;
            }
            if let Some(version) = selected_version {
                self.settings.selected_version = version;
                self.persist();
                self.pending_promoted_server = None;
                self.start_selected_game_on_server(Some(pending.server));
            }
        }

        if self.show_log_window {
            let game_directory =
                self.game_data_directory_for_version(&self.settings.selected_version);
            let log_files = list_recent_logs(&game_directory);
            if self
                .selected_log_path
                .as_ref()
                .is_none_or(|path| !log_files.iter().any(|(candidate, _)| candidate == path))
            {
                self.selected_log_path = log_files.first().map(|(path, _)| path.clone());
            }
            let mut selected = self.selected_log_path.clone();
            let mut open = true;
            let mut copy_text = None;
            let window_response = egui::Window::new(tr("rust.ui.window.logs"))
                .order(OVERLAY_LAYER)
                .open(&mut open)
                .default_pos(Pos2::new(
                    self.settings.log_window_x as f32,
                    self.settings.log_window_y as f32,
                ))
                .default_size([
                    self.settings.log_window_width as f32,
                    self.settings.log_window_height as f32,
                ])
                .resizable(true)
                .show(&ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button(tr("rust.ui.logs.open-folder")).clicked() {
                            let directory = game_directory.join("logs");
                            self.create_and_open_folder_with_status(
                                &directory,
                                LogTarget::Launcher,
                            );
                        }
                        if ui.button(tr("rust.ui.logs.open-launcher-folder")).clicked() {
                            let directory = launcher_log_path().parent().map(Path::to_path_buf);
                            if let Some(directory) = directory {
                                self.create_and_open_folder_with_status(
                                    &directory,
                                    LogTarget::Launcher,
                                );
                            }
                        }
                        if ui.button(tr("rust.ui.logs.open-crash-folder")).clicked() {
                            let directory = game_directory.join("crash-reports");
                            self.create_and_open_folder_with_status(&directory, LogTarget::Crash);
                        }
                        if ui
                            .add_enabled(
                                self.game_running,
                                egui::Button::new(tr("rust.ui.logs.stop-game")),
                            )
                            .clicked()
                        {
                            self.cancel_requested.store(true, Ordering::Release);
                            self.set_logged_status(
                                LogLevel::Warn,
                                LogTarget::Launcher,
                                tr("rust.status.operation.cancelling"),
                            );
                        }
                        ui.label(
                            tr("rust.ui.logs.file-count")
                                .replace("%0", &log_files.len().to_string()),
                        );
                    });
                    ui.separator();
                    ui.columns(2, |columns| {
                        columns[0].set_min_width(220.0);
                        egui::ScrollArea::vertical().show(&mut columns[0], |ui| {
                            if ui
                                .selectable_label(
                                    self.show_live_launcher_log,
                                    tr("rust.ui.logs.live-launcher"),
                                )
                                .clicked()
                            {
                                self.show_live_launcher_log = true;
                                self.show_live_game_log = false;
                                selected = None;
                            }
                            if ui
                                .selectable_label(
                                    self.show_live_game_log,
                                    tr("rust.ui.logs.live-session"),
                                )
                                .clicked()
                            {
                                self.show_live_launcher_log = false;
                                self.show_live_game_log = true;
                                selected = None;
                            }
                            if log_files.is_empty() {
                                ui.label(tr("rust.ui.logs.empty"));
                            }
                            for (path, modified) in &log_files {
                                let age = SystemTime::now()
                                    .duration_since(*modified)
                                    .unwrap_or_default();
                                let (age_key, age_value) = if age.as_secs() < 60 {
                                    ("rust.ui.logs.age.seconds", age.as_secs())
                                } else if age.as_secs() < 3_600 {
                                    ("rust.ui.logs.age.minutes", age.as_secs() / 60)
                                } else if age.as_secs() < 86_400 {
                                    ("rust.ui.logs.age.hours", age.as_secs() / 3_600)
                                } else {
                                    ("rust.ui.logs.age.days", age.as_secs() / 86_400)
                                };
                                let age_label = tr(age_key).replace("%0", &age_value.to_string());
                                let label = format!(
                                    "{}  ·  {age_label}",
                                    path.file_name()
                                        .map(|name| name.to_string_lossy())
                                        .unwrap_or_default()
                                );
                                if ui
                                    .selectable_value(&mut selected, Some(path.clone()), label)
                                    .clicked()
                                {
                                    self.show_live_launcher_log = false;
                                    self.show_live_game_log = false;
                                }
                            }
                        });
                        columns[1].vertical(|ui| {
                            if self.show_live_launcher_log {
                                let text = self
                                    .launcher_log
                                    .lock()
                                    .map(|lines| {
                                        lines
                                            .iter()
                                            .map(LogRecord::render)
                                            .collect::<Vec<_>>()
                                            .join("\n")
                                    })
                                    .unwrap_or_default();
                                ui.horizontal(|ui| {
                                    ui.strong(tr("rust.ui.logs.live-launcher"));
                                    if ui.small_button(tr("rust.ui.logs.copy")).clicked() {
                                        copy_text = Some(text.clone());
                                    }
                                });
                                if text.is_empty() {
                                    ui.label(tr("rust.ui.logs.live-launcher-empty"));
                                } else {
                                    let mut display_text = text;
                                    egui::ScrollArea::vertical().stick_to_bottom(true).show(
                                        ui,
                                        |ui| {
                                            ui.add(
                                                egui::TextEdit::multiline(&mut display_text)
                                                    .font(egui::TextStyle::Monospace)
                                                    .desired_width(f32::INFINITY)
                                                    .desired_rows(24)
                                                    .interactive(false),
                                            );
                                        },
                                    );
                                }
                            } else if self.show_live_game_log {
                                let text = self
                                    .game_log
                                    .lock()
                                    .map(|lines| {
                                        lines.iter().cloned().collect::<Vec<_>>().join("\n")
                                    })
                                    .unwrap_or_default();
                                ui.horizontal(|ui| {
                                    ui.strong(tr("rust.ui.logs.live-session"));
                                    if ui.small_button(tr("rust.ui.logs.copy")).clicked() {
                                        copy_text = Some(text.clone());
                                    }
                                });
                                if text.is_empty() {
                                    ui.label(tr("rust.ui.logs.live-empty"));
                                } else {
                                    let mut display_text = text;
                                    egui::ScrollArea::vertical().stick_to_bottom(true).show(
                                        ui,
                                        |ui| {
                                            ui.add(
                                                egui::TextEdit::multiline(&mut display_text)
                                                    .font(egui::TextStyle::Monospace)
                                                    .desired_width(f32::INFINITY)
                                                    .desired_rows(24)
                                                    .interactive(false),
                                            );
                                        },
                                    );
                                }
                            } else if let Some(path) = &selected {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        path.file_name().unwrap_or_default().to_string_lossy(),
                                    );
                                    if ui.small_button(tr("rust.ui.logs.open-file")).clicked() {
                                        self.open_path_with_status(path, LogTarget::Launcher);
                                    }
                                    if ui.small_button(tr("rust.ui.logs.copy")).clicked() {
                                        copy_text = read_log_tail(path);
                                    }
                                });
                                let mut text = read_log_tail(path)
                                    .unwrap_or_else(|| tr("rust.ui.logs.read-error").to_owned());
                                egui::ScrollArea::vertical().show(ui, |ui| {
                                    ui.add(
                                        egui::TextEdit::multiline(&mut text)
                                            .font(egui::TextStyle::Monospace)
                                            .desired_width(f32::INFINITY)
                                            .desired_rows(24)
                                            .interactive(false),
                                    );
                                });
                            } else {
                                ui.label(tr("rust.ui.logs.select-preview"));
                            }
                        });
                    });
                });
            if let Some(response) = window_response {
                let rect = response.response.rect;
                let width = (rect.width().round() as u32).clamp(400, 4096);
                let height = (rect.height().round() as u32).clamp(300, 2160);
                let x = (rect.left().round() as i32).clamp(-16_384, 16_384);
                let y = (rect.top().round() as i32).clamp(-16_384, 16_384);
                let changed = width != self.settings.log_window_width
                    || height != self.settings.log_window_height
                    || x != self.settings.log_window_x
                    || y != self.settings.log_window_y;
                if changed && (!open || ctx.input(|input| input.pointer.any_released())) {
                    self.settings.log_window_width = width;
                    self.settings.log_window_height = height;
                    self.settings.log_window_x = x;
                    self.settings.log_window_y = y;
                    self.persist();
                }
            }
            if !open && self.settings.logger_mode == "global" {
                self.settings.logger_mode = "none".to_owned();
                self.show_live_launcher_log = false;
                self.show_live_game_log = false;
                self.persist();
            }
            self.show_log_window = open;
            self.selected_log_path = selected;
            if let Some(text) = copy_text {
                ctx.copy_text(text);
                self.set_logged_status(
                    LogLevel::Info,
                    LogTarget::Diagnostics,
                    tr("rust.status.logs.copied"),
                );
            }
        }

        if self.show_require_minecraft_account_notice {
            let mut open = true;
            let mut close = false;
            egui::Window::new(tr("support.require_minecraft_account.title"))
                .order(OVERLAY_LAYER)
                .open(&mut open)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    ui.heading(tr("support.require_minecraft_account.title"));
                    ui.label(tr("support.require_minecraft_account.body"));
                    ui.add_space(8.0);
                    if ui.button(tr("support.button.close")).clicked() {
                        close = true;
                    }
                });
            self.show_require_minecraft_account_notice = open && !close;
        }

        if let Some(alert) = self.version_update_alert.clone() {
            let mut open = true;
            let mut close = false;
            egui::Window::new(tr("version.manager.alert.title"))
                .order(OVERLAY_LAYER)
                .open(&mut open)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(true)
                .default_size([440.0, 360.0])
                .show(&ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(280.0)
                        .show(ui, |ui| {
                            ui.label(&alert);
                        });
                    ui.add_space(8.0);
                    if ui.button(tr("support.button.close")).clicked() {
                        close = true;
                    }
                });
            if close || !open {
                self.version_update_alert = None;
            }
        }

        self.draw_account_dialogs(&ctx);

        self.draw_settings_page(&ctx);

        if self.show_settings_reset_confirmation {
            let mut confirm_reset = false;
            let mut close_reset = false;
            egui::Modal::new(Id::unique("settings_reset_confirmation")).show(&ctx, |ui| {
                ui.set_width(500.0);
                ui.heading(tr("settings.default.warning.title"));
                ui.add_space(6.0);
                ui.label(tr("rust.settings.reset.body"));
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(tr("ui.no")).clicked() {
                        close_reset = true;
                    }
                    if ui.button(tr("ui.yes")).clicked() {
                        confirm_reset = true;
                    }
                });
            });
            if close_reset || confirm_reset {
                self.show_settings_reset_confirmation = false;
            }
            if confirm_reset {
                self.reset_preferences_to_defaults(&ctx);
            }
        }

        if let Some(version) = self.confirm_remove_version.clone() {
            let mut close_dialog = false;
            let mut remove_libraries = None;
            egui::Window::new(tr("rust.ui.window.version.remove"))
                .order(OVERLAY_LAYER)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    ui.label(tr("rust.ui.versions.remove-confirm").replace("%0", &version));
                    ui.horizontal(|ui| {
                        if ui.button(tr("support.button.cancel")).clicked() {
                            close_dialog = true;
                        }
                        if ui.button(tr("version.manager.delete.menu.jar")).clicked() {
                            remove_libraries = Some(false);
                        }
                        if ui
                            .button(tr("version.manager.delete.menu.libraries"))
                            .clicked()
                        {
                            remove_libraries = Some(true);
                        }
                    });
                });
            if close_dialog {
                self.confirm_remove_version = None;
            }
            if let Some(remove_libraries) = remove_libraries {
                self.remove_version_from_disk(&version, remove_libraries);
            }
        }

        if let Some(version) = self.version_install_force_confirmation.clone() {
            let mut decision = None;
            egui::Modal::new(Id::unique(("version_install_force_confirmation", &version))).show(
                &ctx,
                |ui| {
                    ui.set_width(440.0);
                    ui.heading(tr("version.manager.downloader.warning.title"));
                    ui.add_space(6.0);
                    ui.label(
                        tr("version.manager.downloader.warning.force.single")
                            .replace("%0", &version),
                    );
                    ui.add_space(10.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(tr("ui.no")).clicked() {
                            decision = Some(false);
                        }
                        if ui.button(tr("ui.yes")).clicked() {
                            decision = Some(true);
                        }
                    });
                },
            );
            if let Some(confirmed) = decision {
                self.version_install_force_confirmation = None;
                if confirmed {
                    self.start_version_install(version, true);
                }
            }
        }

        if let Some(confirmation) = self.force_update_confirmation.clone() {
            let mut decision = None;
            egui::Modal::new(Id::unique("force_update_confirmation")).show(&ctx, |ui| {
                ui.set_width(500.0);
                ui.heading(tr("forceupdate.question.title"));
                ui.add_space(6.0);
                ui.label(tr("forceupdate.question"));
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(tr("ui.no")).clicked() {
                        decision = Some(false);
                    }
                    if ui.button(tr("ui.yes")).clicked() {
                        decision = Some(true);
                    }
                });
            });
            if let Some(confirmed) = decision {
                self.force_update_confirmation = None;
                if confirmed && self.settings.selected_version == confirmation.version {
                    self.force_update_confirmed_version = Some(confirmation.version);
                    self.start_selected_game_on_server(confirmation.server);
                }
            }
        }

        if let Some(confirmation) = self.outdated_version_confirmation.clone() {
            let mut decision = None;
            egui::Modal::new(Id::unique("outdated_version_confirmation")).show(&ctx, |ui| {
                ui.set_width(560.0);
                ui.heading(tr("versions.found-update.title"));
                ui.add_space(6.0);
                ui.label(tr("versions.found-update"));
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(tr("ui.no")).clicked() {
                        decision = Some(false);
                    }
                    if ui.button(tr("ui.yes")).clicked() {
                        decision = Some(true);
                    }
                });
            });
            if let Some(accept_update) = decision {
                self.outdated_version_confirmation = None;
                if self.settings.selected_version == confirmation.version {
                    if accept_update {
                        self.settings.force_update = true;
                        self.persist();
                        self.start_selected_game_on_server(confirmation.server);
                    } else {
                        match self.update_installed_version_timestamp(&confirmation.version) {
                            Ok(()) => {
                                self.start_selected_game_on_server(confirmation.server);
                            }
                            Err(error) => {
                                let title = tr("versions.found-update.error.title").to_owned();
                                let message = tr("versions.found-update.error").to_owned();
                                self.version_update_prompt_bypassed =
                                    Some(confirmation.version.clone());
                                self.set_logged_status(
                                    LogLevel::Error,
                                    LogTarget::VersionManager,
                                    format!("{message} {error}"),
                                );
                                self.launch_error_dialog = Some(LaunchErrorDialog {
                                    title,
                                    message,
                                    details: Some(error),
                                });
                                self.start_selected_game_on_server(confirmation.server);
                            }
                        }
                    }
                }
            }
        }

        if self.show_autologin_tip {
            let mut close_tip = false;
            egui::Modal::new(Id::unique("autologin_tip")).show(&ctx, |ui| {
                ui.set_width(460.0);
                ui.heading(tr("loginform.checkbox.autologin.tip.title"));
                ui.add_space(6.0);
                ui.label(tr("loginform.checkbox.autologin.tip"));
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(tr("feedback.button.okay")).clicked() {
                        close_tip = true;
                    }
                });
            });
            if close_tip {
                self.show_autologin_tip = false;
            }
        }

        self.draw_crash_dialogs(&ctx);

        let legacy_font_revert = self
            .settings
            .legacy_font_size_revert
            .filter(|previous| (previous - self.settings.launcher_font_size).abs() > 0.01);
        if let Some(previous_size) = legacy_font_revert {
            let mut revert = false;
            let mut dismiss = false;
            egui::Area::new(Id::unique("legacy_font_revert"))
                .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 8.0))
                .order(SCENE_LAYER)
                .show(&ctx, |ui| {
                    egui::Frame::popup(ui.style())
                        .inner_margin(Margin::symmetric(8, 5))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if ui
                                    .button(
                                        RichText::new(tr("revert.font.approve"))
                                            .size(previous_size),
                                    )
                                    .clicked()
                                {
                                    revert = true;
                                }
                                if ui
                                    .small_button(tr("revert.font.close"))
                                    .on_hover_text(tr("revert.font.close.hint"))
                                    .clicked()
                                {
                                    dismiss = true;
                                }
                            });
                        });
                });
            if revert {
                self.settings.launcher_font_size = previous_size;
                self.settings.legacy_font_size_revert = None;
                ctx.set_zoom_factor(previous_size / 12.0);
                self.persist();
            } else if dismiss {
                self.settings.legacy_font_size_revert = None;
                self.persist();
            }
        }

        if !self.status.is_empty() {
            let status = self.status.clone();
            egui::Area::new(Id::unique("status_toast"))
                .anchor(Align2::LEFT_BOTTOM, Vec2::new(12.0, -10.0))
                .order(SCENE_LAYER)
                .show(&ctx, |ui| {
                    egui::Frame::new()
                        .fill(Color32::from_rgba_unmultiplied(20, 24, 28, 215))
                        .inner_margin(Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(status).size(12.0).color(Color32::WHITE));
                                if (self.launching
                                    || self.microsoft_signin_pending
                                    || self.ely_legacy_signin_pending)
                                    && !self.cancel_requested.load(Ordering::Acquire)
                                    && ui.small_button(tr("support.button.cancel")).clicked()
                                {
                                    self.cancel_requested.store(true, Ordering::Release);
                                    self.set_logged_status(
                                        LogLevel::Warn,
                                        LogTarget::Launcher,
                                        tr("rust.status.operation.cancelling"),
                                    );
                                }
                            });
                            if let Some(progress) = self.launch_progress {
                                ui.add(
                                    egui::ProgressBar::new(progress)
                                        .desired_width(250.0)
                                        .show_percentage(),
                                );
                            }
                        });
                });
        }

        self.draw_first_run_dialogs(&ctx);
    }
}
