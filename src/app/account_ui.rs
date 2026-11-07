use super::*;

impl LauncherApp {
    pub(in crate::app) fn draw_account_manager(&mut self, ui: &mut egui::Ui) -> bool {
        let mut return_home = false;
        let panel_fill = Color32::from_rgba_unmultiplied(43, 62, 72, 150);

        match self.account_manager_view {
            AccountManagerView::Overview => {
                ui.columns(2, |columns| {
                    egui::Frame::new()
                        .fill(panel_fill)
                        .inner_margin(Margin::same(14))
                        .show(&mut columns[0], |ui| {
                            ui.set_min_height(350.0);
                            let accounts = self.manager_account_rows();
                            egui::ScrollArea::vertical()
                                .id_salt("account_manager_accounts")
                                .auto_shrink([false, false])
                                .max_height(310.0)
                                .show(ui, |ui| {
                                    if accounts.is_empty() {
                                        ui.label(tr("account.empty"));
                                    }
                                    for (key, label, icon) in accounts {
                                        let selected = self.settings.selected_account.as_deref()
                                            == Some(key.as_str());
                                        let response = ui.add_sized(
                                            Vec2::new(ui.available_width(), 28.0),
                                            egui::Button::selectable(
                                                selected,
                                                RichText::new(format!("{icon}  {label}"))
                                                    .size(13.0),
                                            ),
                                        );
                                        if response.clicked() {
                                            self.settings.selected_account = Some(key);
                                            self.persist();
                                        }
                                    }
                                });
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                let gap = ui.spacing().item_spacing.x;
                                let width = ((ui.available_width() - gap * 2.0) / 3.0).max(40.0);
                                if ui
                                    .add_sized(Vec2::new(width, 38.0), egui::Button::new("＋"))
                                    .on_hover_text("Add account")
                                    .clicked()
                                {
                                    self.account_manager_view = AccountManagerView::Add;
                                }
                                let can_edit =
                                    self.settings.accounts.iter().any(|account| {
                                        self.settings.selected_account.as_deref()
                                            == Some(account.as_str())
                                    }) || self.settings.ely_legacy_accounts.iter().any(|account| {
                                        self.settings.selected_account.as_deref()
                                            == Some(account.id.as_str())
                                    });
                                if ui
                                    .add_enabled(
                                        can_edit,
                                        egui::Button::new("✎").min_size(Vec2::new(width, 38.0)),
                                    )
                                    .on_hover_text("Edit account")
                                    .clicked()
                                {
                                    let selected =
                                        self.settings.selected_account.clone().unwrap_or_default();
                                    if let Some(account) = self
                                        .settings
                                        .accounts
                                        .iter()
                                        .find(|account| **account == selected)
                                    {
                                        self.new_account = account.clone();
                                        self.new_account_ely_skins =
                                            offline_ely_skins_for(&self.settings, account);
                                        self.account_manager_view = AccountManagerView::Edit;
                                    } else if let Some(account) = self
                                        .settings
                                        .ely_legacy_accounts
                                        .iter()
                                        .find(|account| account.id == selected)
                                        .cloned()
                                    {
                                        self.open_edit_ely_legacy_dialog(&account);
                                    }
                                }
                                if ui
                                    .add_sized(Vec2::new(width, 38.0), egui::Button::new("⌂"))
                                    .on_hover_text(tr("explorer.button.home.tip"))
                                    .clicked()
                                {
                                    return_home = true;
                                }
                            });
                        });
                    egui::Frame::new()
                        .fill(panel_fill)
                        .inner_margin(Margin::same(18))
                        .show(&mut columns[1], |ui| {
                            ui.set_min_height(350.0);
                            ui.heading(tr("rust.settings.page.accounts"));
                            ui.add_space(8.0);
                            render_notice_html(
                                ui,
                                &tr("account.manager.multipane.welcome.body"),
                                true,
                                &mut None,
                            );
                            ui.add_space(12.0);
                            ui.separator();
                            ui.label(RichText::new(self.selected_account_label()).strong());
                            ui.add_space(12.0);
                            ui.collapsing(tr("rust.settings.import.accounts.heading"), |ui| {
                                ui.horizontal(|ui| {
                                    ui.text_edit_singleline(
                                        &mut self.settings.legacy_profiles_path,
                                    );
                                    if ui
                                        .small_button(tr("rust.settings.import.accounts.button"))
                                        .clicked()
                                    {
                                        let path =
                                            if self.settings.legacy_profiles_path.trim().is_empty()
                                            {
                                                self.game_dir_path().join("tlauncher_profiles.json")
                                            } else {
                                                PathBuf::from(
                                                    self.settings.legacy_profiles_path.trim(),
                                                )
                                            };
                                        match import_legacy_accounts(&path, &mut self.settings) {
                                            Ok(summary) => {
                                                self.set_logged_status(
                                                    LogLevel::Info,
                                                    LogTarget::Authentication,
                                                    tr("rust.status.import.accounts.success")
                                                        .replace("%0", &summary.offline.to_string())
                                                        .replace(
                                                            "%1",
                                                            &summary.microsoft.to_string(),
                                                        )
                                                        .replace("%2", &summary.ely.to_string())
                                                        .replace(
                                                            "%3",
                                                            &summary.ely_legacy.to_string(),
                                                        )
                                                        .replace(
                                                            "%4",
                                                            &summary.mojang_migrated.to_string(),
                                                        )
                                                        .replace(
                                                            "%5",
                                                            &summary.skipped.to_string(),
                                                        ),
                                                );
                                                self.persist();
                                            }
                                            Err(error) => self.set_logged_status(
                                                LogLevel::Error,
                                                LogTarget::Authentication,
                                                tr("rust.status.import.accounts.error")
                                                    .replace("%0", &error.to_string()),
                                            ),
                                        }
                                    }
                                });
                            });
                        });
                });
            }
            AccountManagerView::Add => {
                egui::Frame::new()
                    .fill(panel_fill)
                    .inner_margin(Margin::same(18))
                    .show(ui, |ui| {
                        ui.set_min_width(370.0);
                        ui.horizontal(|ui| {
                            if ui.button("←").clicked() {
                                self.account_manager_view = AccountManagerView::Overview;
                            }
                            ui.heading("Add account");
                        });
                        ui.add_space(18.0);
                        for (icon, label, action) in [
                            ("E", "Ely.by skin system", 0),
                            ("M", "Microsoft", 1),
                            ("$", "Buy Minecraft", 2),
                            ("●", "Free (w/o password)", 3),
                            ("i", "I'm confused... Help me", 4),
                        ] {
                            if ui
                                .add_sized(
                                    Vec2::new(ui.available_width(), 46.0),
                                    egui::Button::new(format!("{icon}    {label}")),
                                )
                                .clicked()
                            {
                                self.account_manager_view = AccountManagerView::Overview;
                                match action {
                                    0 => self.open_ely_dialog(),
                                    1 => self.start_microsoft_sign_in(),
                                    2 => {
                                        self.open_url_with_status(
                                            "https://minecraft.net/store/minecraft",
                                            LogTarget::Authentication,
                                        );
                                    }
                                    3 => self.open_account_dialog(),
                                    _ => self.show_account_help = true,
                                }
                            }
                            ui.add_space(5.0);
                        }
                    });
            }
            AccountManagerView::Edit => {
                let selected = self.settings.selected_account.clone().unwrap_or_default();
                let mut delete_account = false;
                let mut save_account = false;
                egui::Frame::new()
                    .fill(panel_fill)
                    .inner_margin(Margin::same(18))
                    .show(ui, |ui| {
                        ui.set_min_width(370.0);
                        ui.horizontal(|ui| {
                            if ui.button("←").clicked() {
                                self.account_manager_view = AccountManagerView::Overview;
                            }
                            ui.heading("Edit");
                        });
                        ui.add_space(24.0);
                        ui.label("Enter your username:");
                        ui.text_edit_singleline(&mut self.new_account);
                        ui.checkbox(&mut self.new_account_ely_skins, "Use Ely.by skins");
                        ui.add_space(8.0);
                        if ui
                            .add_sized(
                                Vec2::new(ui.available_width(), 38.0),
                                egui::Button::new("👤  Edit"),
                            )
                            .clicked()
                        {
                            save_account = true;
                        }
                        ui.add_space(8.0);
                        if ui
                            .add_sized(
                                Vec2::new(ui.available_width(), 32.0),
                                egui::Button::new("✖  Delete account"),
                            )
                            .clicked()
                        {
                            delete_account = true;
                        }
                    });
                if save_account {
                    let name = self.new_account.trim().to_owned();
                    let duplicate = self.settings.accounts.iter().any(|saved| {
                        !saved.eq_ignore_ascii_case(&selected) && saved.eq_ignore_ascii_case(&name)
                    });
                    if name.is_empty() || !valid_plain_name(&name) || duplicate {
                        self.set_logged_status(
                            LogLevel::Warn,
                            LogTarget::Authentication,
                            tr("rust.status.account.offline.nonstandard-disabled"),
                        );
                    } else {
                        let was_migrated = self
                            .settings
                            .migrated_mojang_accounts
                            .iter()
                            .any(|saved| saved.eq_ignore_ascii_case(&selected));
                        let uuid = offline_account_uuid_for(&self.settings, &selected);
                        self.settings
                            .accounts
                            .iter_mut()
                            .filter(|saved| saved.eq_ignore_ascii_case(&selected))
                            .for_each(|saved| *saved = name.clone());
                        self.settings
                            .migrated_mojang_accounts
                            .retain(|saved| !saved.eq_ignore_ascii_case(&selected));
                        if was_migrated {
                            self.settings.migrated_mojang_accounts.push(name.clone());
                        }
                        self.settings
                            .offline_account_uuids
                            .retain(|saved, _| !saved.eq_ignore_ascii_case(&selected));
                        if let Some(uuid) = uuid {
                            self.settings
                                .offline_account_uuids
                                .insert(name.clone(), uuid);
                        }
                        self.settings
                            .offline_ely_skins
                            .retain(|saved, _| !saved.eq_ignore_ascii_case(&selected));
                        self.settings
                            .offline_ely_skins
                            .insert(name.clone(), self.new_account_ely_skins);
                        self.settings.selected_account = Some(name);
                        self.persist();
                        self.account_manager_view = AccountManagerView::Overview;
                    }
                }
                if delete_account {
                    self.settings
                        .accounts
                        .retain(|account| !account.eq_ignore_ascii_case(&selected));
                    self.settings
                        .migrated_mojang_accounts
                        .retain(|account| !account.eq_ignore_ascii_case(&selected));
                    self.settings
                        .offline_account_uuids
                        .retain(|account, _| !account.eq_ignore_ascii_case(&selected));
                    self.settings
                        .offline_ely_skins
                        .retain(|account, _| !account.eq_ignore_ascii_case(&selected));
                    self.settings.selected_account = self.first_account_key();
                    self.persist();
                    self.account_manager_view = AccountManagerView::Overview;
                }
            }
        }
        return_home
    }

    fn manager_account_rows(&self) -> Vec<(String, String, &'static str)> {
        let mut rows = Vec::new();
        rows.extend(
            self.settings
                .microsoft_accounts
                .iter()
                .map(|account| (account.id.clone(), account.username.clone(), "M")),
        );
        rows.extend(
            self.settings
                .ely_accounts
                .iter()
                .map(|account| (account.id.clone(), account.display_name.clone(), "E")),
        );
        rows.extend(
            self.settings
                .ely_legacy_accounts
                .iter()
                .map(|account| (account.id.clone(), account.display_name.clone(), "E")),
        );
        rows.extend(
            self.settings
                .accounts
                .iter()
                .map(|account| (account.clone(), account.clone(), "●")),
        );
        rows
    }

    pub(in crate::app) fn draw_account_dialogs(&mut self, ctx: &Context) {
        if self.show_account_dialog {
            let editing = self.editing_account.is_some();
            let title = if editing {
                tr("rust.ui.window.account.offline-edit")
            } else {
                tr("rust.ui.window.account.offline-add")
            };
            egui::Window::new(title)
                .order(OVERLAY_LAYER)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    ui.label(tr("rust.ui.account.offline-hint"));
                    ui.add_space(6.0);
                    ui.text_edit_singleline(&mut self.new_account);
                    if !valid_plain_name(self.new_account.trim()) {
                        ui.checkbox(
                            &mut self.new_account_allow_invalid,
                            tr("rust.ui.account.allow-nonstandard"),
                        );
                    }
                    ui.checkbox(
                        &mut self.new_account_ely_skins,
                        tr("account.button.ely.toggle"),
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(tr("support.button.cancel")).clicked() {
                            self.show_account_dialog = false;
                            self.editing_account = None;
                        }
                        let save_label = if editing {
                            tr("rust.ui.account.save")
                        } else {
                            tr("rust.ui.account.add")
                        };
                        if ui.button(save_label).clicked() {
                            let name = self.new_account.trim().to_owned();
                            if !name.is_empty()
                                && (valid_plain_name(&name) || self.new_account_allow_invalid)
                            {
                                let was_editing = self.editing_account.is_some();
                                let old_name = self.editing_account.take();
                                let collision = old_name.as_ref().is_some_and(|old_name| {
                                    old_name != &name
                                        && self.settings.accounts.iter().any(|saved| {
                                            !saved.eq_ignore_ascii_case(old_name)
                                                && saved.eq_ignore_ascii_case(&name)
                                        })
                                });
                                if collision {
                                    self.editing_account = old_name;
                                    self.set_logged_status(
                                        LogLevel::Warn,
                                        LogTarget::Authentication,
                                        tr("rust.status.account.offline.duplicate"),
                                    );
                                } else {
                                    if let Some(old_name) = old_name {
                                        if old_name != name {
                                            let was_migrated_mojang =
                                                self.settings.migrated_mojang_accounts.iter().any(
                                                    |saved| saved.eq_ignore_ascii_case(&old_name),
                                                );
                                            let preserved_uuid =
                                                offline_account_uuid_for(&self.settings, &old_name);
                                            self.settings.migrated_mojang_accounts.retain(
                                                |saved| !saved.eq_ignore_ascii_case(&old_name),
                                            );
                                            if was_migrated_mojang {
                                                self.settings
                                                    .migrated_mojang_accounts
                                                    .push(name.clone());
                                            }
                                            self.settings.offline_account_uuids.retain(
                                                |saved, _| !saved.eq_ignore_ascii_case(&old_name),
                                            );
                                            self.settings.offline_ely_skins.retain(|saved, _| {
                                                !saved.eq_ignore_ascii_case(&old_name)
                                            });
                                            if let Some(uuid) = preserved_uuid {
                                                self.settings
                                                    .offline_account_uuids
                                                    .insert(name.clone(), uuid);
                                            }
                                            self.settings
                                                .offline_ely_skins
                                                .insert(name.clone(), self.new_account_ely_skins);
                                            if let Some(saved) =
                                                self.settings.accounts.iter_mut().find(|saved| {
                                                    saved.eq_ignore_ascii_case(&old_name)
                                                })
                                            {
                                                *saved = name.clone();
                                            }
                                        }
                                    } else if !self
                                        .settings
                                        .accounts
                                        .iter()
                                        .any(|account| account.eq_ignore_ascii_case(&name))
                                    {
                                        self.settings.accounts.push(name.clone());
                                    }
                                    self.settings
                                        .offline_ely_skins
                                        .insert(name.clone(), self.new_account_ely_skins);
                                    self.settings.selected_account = Some(name);
                                    self.show_account_dialog = false;
                                    self.account_success_kind = Some(if was_editing {
                                        "success-edit"
                                    } else {
                                        "success-add"
                                    });
                                    self.persist();
                                }
                            } else {
                                let message = if name.is_empty() {
                                    tr("rust.status.account.offline.name-required").to_owned()
                                } else {
                                    tr("rust.status.account.offline.nonstandard-disabled")
                                        .to_owned()
                                };
                                self.set_logged_status(
                                    LogLevel::Warn,
                                    LogTarget::Authentication,
                                    message,
                                );
                            }
                        }
                    });
                });
        } else if self.show_account_help {
            let mut open = true;
            let mut close = false;
            let mut open_minecraft_store = false;
            let mut open_ely_registration = false;
            egui::Window::new(tr("rust.ui.window.account.help"))
                .order(OVERLAY_LAYER)
                .open(&mut open)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    ui.label(tr("rust.ui.account.help.microsoft"));
                    ui.add_space(6.0);
                    ui.label(tr("rust.ui.account.help.offline"));
                    ui.add_space(6.0);
                    ui.label(tr("rust.ui.account.help.ely"));
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui
                            .button(tr("account.manager.multipane.add-account.buy-minecraft"))
                            .clicked()
                        {
                            open_minecraft_store = true;
                        }
                        if ui.button(tr("rust.ui.account.help.ely-register")).clicked() {
                            open_ely_registration = true;
                        }
                        if ui.button(tr("support.button.close")).clicked() {
                            close = true;
                        }
                    });
                });
            self.show_account_help = open && !close;
            if open_minecraft_store {
                self.open_url_with_status(
                    "https://minecraft.net/store/minecraft",
                    LogTarget::Authentication,
                );
            }
            if open_ely_registration {
                self.open_url_with_status("https://ely.by", LogTarget::Authentication);
            }
        } else if self.show_ely_dialog {
            let mut open = true;
            egui::Window::new(tr("rust.ui.window.account.ely"))
                .order(OVERLAY_LAYER)
                .open(&mut open)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    ui.label(tr("rust.ui.account.ely-browser-auth"));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let cancel_label = if self.ely_signin_pending {
                            tr("rust.ui.account.cancel-sign-in")
                        } else {
                            tr("support.button.cancel")
                        };
                        if ui.button(cancel_label).clicked() {
                            if self.ely_signin_pending {
                                self.cancel_requested.store(true, Ordering::Release);
                                self.set_logged_status(
                                    LogLevel::Warn,
                                    LogTarget::Authentication,
                                    tr("rust.status.account.ely.cancelling"),
                                );
                            } else {
                                self.show_ely_dialog = false;
                            }
                        }
                        if ui
                            .add_enabled(
                                !self.ely_signin_pending
                                    && !self.microsoft_signin_pending
                                    && !self.ely_legacy_signin_pending
                                    && !self.launching,
                                egui::Button::new(tr("rust.ui.account.ely-continue-browser")),
                            )
                            .clicked()
                        {
                            self.start_ely_sign_in();
                        }
                        if self.ely_signin_pending {
                            ui.spinner();
                        }
                    });
                });
            if !open && self.ely_signin_pending {
                self.cancel_requested.store(true, Ordering::Release);
            } else if !open {
                self.show_ely_dialog = false;
            }
        } else if self.show_ely_legacy_dialog {
            let mut open = true;
            let title = if self.editing_ely_legacy_account.is_some() {
                tr("rust.ui.window.account.ely-legacy-edit")
            } else {
                tr("rust.ui.window.account.ely-legacy")
            };
            egui::Window::new(title)
                .order(OVERLAY_LAYER)
                .open(&mut open)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    ui.label(tr("rust.ui.account.ely-signin-hint"));
                    ui.add_space(6.0);
                    ui.label(tr("rust.ui.account.username"));
                    ui.text_edit_singleline(&mut self.ely_legacy_username);
                    ui.label(tr("rust.ui.account.password"));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.ely_legacy_password).password(true),
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let cancel_label = if self.ely_legacy_signin_pending {
                            tr("rust.ui.account.cancel-sign-in")
                        } else {
                            tr("support.button.cancel")
                        };
                        if ui.button(cancel_label).clicked() {
                            if self.ely_legacy_signin_pending {
                                self.cancel_requested.store(true, Ordering::Release);
                                self.set_logged_status(
                                    LogLevel::Warn,
                                    LogTarget::Authentication,
                                    tr("rust.status.account.ely.cancelling"),
                                );
                            } else {
                                self.show_ely_legacy_dialog = false;
                                self.editing_ely_legacy_account = None;
                            }
                        }
                        if ui
                            .add_enabled(
                                !self.ely_legacy_signin_pending && !self.launching,
                                egui::Button::new(tr("rust.ui.account.sign-in")),
                            )
                            .clicked()
                        {
                            self.start_ely_legacy_sign_in();
                        }
                        if self.ely_legacy_signin_pending {
                            ui.spinner();
                        }
                    });
                });
            if !open && self.ely_legacy_signin_pending {
                self.cancel_requested.store(true, Ordering::Release);
            } else if !open {
                self.show_ely_legacy_dialog = false;
                self.editing_ely_legacy_account = None;
            }
        } else if self.show_minecraft_profile_dialog {
            let mut open = true;
            egui::Window::new(tr("rust.ui.window.account.minecraft-profile"))
                .order(OVERLAY_LAYER)
                .open(&mut open)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    ui.label(tr("rust.ui.account.microsoft-no-profile"));
                    ui.label(tr("rust.ui.account.microsoft-profile-hint"));
                    ui.add_space(6.0);
                    ui.text_edit_singleline(&mut self.new_minecraft_profile_name);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                !self.profile_creation_pending,
                                egui::Button::new(tr("ui.cancel")),
                            )
                            .clicked()
                        {
                            self.show_minecraft_profile_dialog = false;
                            self.pending_minecraft_profile = None;
                        }
                        if ui
                            .add_enabled(
                                !self.profile_creation_pending,
                                egui::Button::new(tr("rust.ui.account.create-profile")),
                            )
                            .clicked()
                        {
                            self.start_minecraft_profile_creation();
                        }
                        if self.profile_creation_pending {
                            ui.spinner();
                        }
                    });
                });
            if !open && !self.profile_creation_pending {
                self.show_minecraft_profile_dialog = false;
                self.pending_minecraft_profile = None;
            }
        } else if let Some(kind) = self.account_success_kind {
            let mut open = true;
            let title = tr_dynamic(&format!("account.manager.multipane.{kind}.title"));
            let mut close = false;
            let mut html_action = None;
            egui::Window::new(title)
                .order(OVERLAY_LAYER)
                .open(&mut open)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    render_notice_html(
                        ui,
                        &tr("account.manager.multipane.success.body"),
                        true,
                        &mut html_action,
                    );
                    ui.add_space(8.0);
                    if ui.button(tr("support.button.close")).clicked() {
                        close = true;
                    }
                });
            if let Some(action) = html_action {
                self.perform_notice_action(action, &ctx);
            }
            if close || !open {
                self.account_success_kind = None;
            }
        }
    }
}
