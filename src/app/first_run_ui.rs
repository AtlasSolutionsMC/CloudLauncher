use super::*;

impl LauncherApp {
    pub(in crate::app) fn draw_first_run_dialogs(&mut self, ctx: &Context) {
        if self.show_first_run_notice {
            let acknowledged = egui::Modal::new(Id::unique("first_run_notice")).show(&ctx, |ui| {
                ui.set_width(520.0);
                ui.add_space(4.0);
                ui.heading(tr("firstrun.title"));
                ui.label(
                    RichText::new(tr("firstrun.notice.welcome"))
                        .strong()
                        .size(19.0),
                );
                ui.add_space(2.0);
                ui.label(tr("firstrun.notice.body"));
                ui.add_space(7.0);
                ui.separator();
                for key in [
                    "firstrun.notice.body.0",
                    "firstrun.notice.body.1",
                    "firstrun.notice.body.2",
                ] {
                    ui.horizontal_top(|ui| {
                        ui.label(RichText::new("–").color(Color32::from_rgb(111, 173, 203)));
                        ui.add(egui::Label::new(tr(key)).wrap());
                    });
                    ui.add_space(5.0);
                }
                ui.add_space(6.0);
                let mut accepted = false;
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    accepted = ui
                        .add_sized(
                            [170.0, 36.0],
                            egui::Button::new(tr("firstrun.notice.answer.yes")),
                        )
                        .clicked();
                });
                accepted
            });
            if acknowledged.inner {
                self.show_first_run_notice = false;
                self.persist();
            }
        }

        if self.show_first_run_folder_notice && !self.show_first_run_notice {
            let mut open = true;
            let mut browse = false;
            let mut skip = false;
            let mut apply = false;
            egui::Window::new(tr("newfolder.title"))
                .order(OVERLAY_LAYER)
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_size([540.0, 270.0])
                .show(&ctx, |ui| {
                    ui.set_min_width(510.0);
                    ui.heading(tr("newfolder.head"));
                    ui.label(tr("newfolder.body"));
                    ui.add_space(8.0);
                    ui.label(tr("newfolder.select.title"));
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.first_run_folder_path)
                                .desired_width(380.0),
                        );
                        if ui.button(tr("newfolder.select.browse")).clicked() {
                            browse = true;
                        }
                    });
                    if let Some(error) = &self.first_run_folder_error {
                        ui.colored_label(Color32::LIGHT_RED, error);
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(tr("newfolder.button.cancel")).clicked() {
                            skip = true;
                        }
                        if ui.button(tr("newfolder.button.ok")).clicked() {
                            apply = true;
                        }
                    });
                });

            if browse {
                let current_selection = PathBuf::from(self.first_run_folder_path.trim());
                let starting_directory = if current_selection.is_dir() {
                    Some(current_selection)
                } else {
                    current_selection
                        .parent()
                        .filter(|parent| parent.is_dir())
                        .map(Path::to_path_buf)
                };
                let mut dialog = rfd::FileDialog::new().set_title(tr("newfolder.select.prompt"));
                if let Some(directory) = starting_directory {
                    dialog = dialog.set_directory(directory);
                }
                if let Some(selected) = dialog.pick_folder() {
                    self.first_run_folder_path = selected.to_string_lossy().into_owned();
                    self.first_run_folder_error = None;
                }
            }

            if apply {
                let selected = PathBuf::from(self.first_run_folder_path.trim());
                if !selected.is_absolute() || !selected.is_dir() || fs::read_dir(&selected).is_err()
                {
                    self.first_run_folder_error = Some(tr("newfolder.select.error").to_owned());
                } else if path_is_same_or_nested(
                    &selected,
                    &self.first_run_folder_current_directory,
                ) {
                    self.first_run_folder_error =
                        Some(tr("newfolder.select.error.inside").to_owned());
                } else {
                    self.settings.game_directory = selected
                        .canonicalize()
                        .unwrap_or(selected)
                        .to_string_lossy()
                        .into_owned();
                    self.persist();
                    self.show_first_run_folder_notice = false;
                    self.first_run_folder_error = None;
                }
            }

            if skip || !open {
                self.show_first_run_folder_notice = false;
                self.first_run_folder_error = None;
            }
        }

        if self.show_arm_compat_notice
            && !self.show_first_run_notice
            && !self.show_first_run_folder_notice
        {
            let started_at = *self
                .arm_compat_notice_started_at
                .get_or_insert_with(Instant::now);
            let remaining = 5u64.saturating_sub(started_at.elapsed().as_secs());
            if remaining > 0 {
                ctx.request_repaint_after(Duration::from_secs(1));
            }

            let mut open = true;
            let mut open_support = false;
            let mut install_pack = false;
            egui::Window::new(tr("opengl-pack.title"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .order(OVERLAY_LAYER)
                .show(&ctx, |ui| {
                    ui.set_min_width(500.0);
                    ui.heading(tr("opengl-pack.head"));
                    ui.label(tr("opengl-pack.body"));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(tr("loginform.button.support")).clicked() {
                            open_support = true;
                        }

                        let install_label = if remaining > 0 {
                            tr("opengl-pack.button.countdown").replace("%0", &remaining.to_string())
                        } else {
                            tr("opengl-pack.button.install").to_owned()
                        };
                        if ui
                            .add_enabled(
                                remaining == 0,
                                egui::Button::new(install_label).min_size(Vec2::new(150.0, 36.0)),
                            )
                            .clicked()
                        {
                            install_pack = true;
                        }
                    });
                });

            if install_pack {
                open = !self.open_url_with_status(
                    "https://apps.microsoft.com/detail/9nqpsl29bfff",
                    LogTarget::Launcher,
                );
            }
            if open_support {
                self.open_support_flow();
                open = false;
            }
            self.show_arm_compat_notice = open;
        }
    }
}
