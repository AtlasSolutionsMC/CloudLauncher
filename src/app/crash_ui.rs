use super::*;

impl LauncherApp {
    pub(in crate::app) fn draw_crash_dialogs(&mut self, ctx: &Context) {
        if let Some(dialog) = self.launch_error_dialog.as_ref() {
            let title = dialog.title.clone();
            let message = dialog.message.clone();
            let details = dialog.details.clone();
            let mut close = false;
            egui::Modal::new(Id::unique("launch_error_dialog")).show(&ctx, |ui| {
                ui.set_width(520.0);
                ui.heading(title);
                ui.add_space(6.0);
                ui.add(egui::Label::new(RichText::new(message).color(Color32::LIGHT_RED)).wrap());
                if let Some(mut details) = details {
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add(
                        egui::TextEdit::multiline(&mut details)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(500.0)
                            .desired_rows(8)
                            .interactive(false),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(tr("support.button.close")).clicked() {
                            close = true;
                        }
                        if ui.button(tr("launcher.error.copy-details")).clicked() {
                            ctx.copy_text(details.clone());
                        }
                    });
                } else {
                    ui.add_space(10.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        close = ui.button(tr("support.button.close")).clicked();
                    });
                }
            });
            if close {
                self.launch_error_dialog = None;
            }
        }

        if let Some(detection) = self.malware_detection.clone() {
            let mut open = true;
            let mut close = false;
            egui::Window::new(tr("rust.malware.detected.title"))
                .order(OVERLAY_LAYER)
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    ui.colored_label(Color32::LIGHT_RED, tr("jarscanner.detected"));
                    ui.separator();
                    ui.label(
                        tr("rust.malware.detected.jar")
                            .replace("%0", &detection.jar.display().to_string()),
                    );
                    ui.label(tr("rust.malware.detected.entry").replace("%0", &detection.entry));
                    ui.add_space(8.0);
                    if ui.button(tr("rust.malware.button.ok")).clicked() {
                        close = true;
                    }
                });
            if !open || close {
                self.malware_detection = None;
            }
        }

        if self.malware_scan_confirmation.is_some() {
            let mut open = true;
            let mut answer = None;
            egui::Window::new(tr("rust.malware.wait.title"))
                .order(OVERLAY_LAYER)
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    ui.label(tr("jarscanner.takes-time"));
                    ui.horizontal(|ui| {
                        if ui.button(tr("rust.malware.button.continue")).clicked() {
                            answer = Some(true);
                        }
                        if ui.button(tr("rust.malware.button.skip")).clicked() {
                            answer = Some(false);
                        }
                    });
                });
            if !open {
                answer = Some(false);
            }
            if let Some(answer) = answer {
                if let Some(reply) = self.malware_scan_confirmation.take() {
                    let _ = reply.send(answer);
                }
            }
        }

        if let Some(mut report) = self.crash_report.clone() {
            let mut open = true;
            let mut selected_action = None;
            let mut request_crash_help = false;
            let mut open_crash_logs = false;
            let mut dismiss_crash = false;
            let suggestion_permits_help = report
                .suggestions
                .first()
                .is_none_or(|suggestion| suggestion.permit_help);
            let crash_build = format!("{} {}", report.version, report.loader).to_ascii_lowercase();
            let is_forge_or_fabric =
                crash_build.contains("forge") || crash_build.contains("fabric");
            let is_local_only_version = !is_forge_or_fabric
                && self.version_manifest.as_ref().is_some_and(|manifest| {
                    !manifest
                        .versions
                        .iter()
                        .any(|entry| entry.id == report.version)
                });
            let permit_help = suggestion_permits_help && !is_local_only_version;
            egui::Window::new(tr("rust.ui.window.crash"))
                .order(OVERLAY_LAYER)
                .open(&mut open)
                .default_size([660.0, 520.0])
                .resizable(true)
                .show(&ctx, |ui| {
                    ui.colored_label(Color32::LIGHT_RED, &report.summary);
                    if let Some(path) = &report.path {
                        ui.horizontal(|ui| {
                            ui.label(
                                tr("rust.ui.logs.path").replace("%0", &path.display().to_string()),
                            );
                            if ui.small_button(tr("rust.ui.logs.open-file")).clicked() {
                                self.open_path_with_status(path, LogTarget::Crash);
                            }
                        });
                    }
                    ui.separator();
                    if !report.suggestions.is_empty() {
                        ui.heading(tr("rust.ui.crash.matches"));
                        for suggestion in &report.suggestions {
                            ui.group(|ui| {
                                let crash_icon = self.assets.as_ref().map(|assets| {
                                    if suggestion.image.as_deref() == Some("logo-duke") {
                                        &assets.crash_duke
                                    } else {
                                        &assets.crash_bug
                                    }
                                });
                                ui.horizontal(|ui| {
                                    if let Some(icon) = crash_icon {
                                        ui.image((icon.id(), Vec2::splat(32.0)));
                                    }
                                    ui.vertical(|ui| {
                                        ui.strong(&suggestion.title);
                                        if !suggestion.body.is_empty() {
                                            ui.label(&suggestion.body);
                                        }
                                    });
                                });
                                ui.horizontal_wrapped(|ui| {
                                    for action in &suggestion.actions {
                                        match &action.kind {
                                            crash::CrashActionKind::OpenUrl(url) => {
                                                ui.hyperlink_to(&action.label, url);
                                            }
                                            kind => {
                                                if ui.small_button(&action.label).clicked() {
                                                    selected_action = Some(kind.clone());
                                                }
                                            }
                                        }
                                    }
                                });
                            });
                        }
                        ui.separator();
                    }
                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut report.log)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(18)
                                    .interactive(false),
                            );
                        });
                    ui.separator();
                    ui.horizontal(|ui| {
                        if permit_help && ui.button(tr("crash.buttons.support")).clicked() {
                            request_crash_help = true;
                        }
                        if ui.button(tr("crash.buttons.logs")).clicked() {
                            open_crash_logs = true;
                        }
                        if ui.button(tr("crash.buttons.exit")).clicked() {
                            dismiss_crash = true;
                        }
                    });
                });
            if !open || dismiss_crash {
                self.crash_report = None;
                if self.settings.launch_action == "exit" {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            if request_crash_help {
                self.open_support_flow();
            }
            if open_crash_logs {
                if let Some(path) = report.path.as_ref().filter(|path| path.is_file()) {
                    self.open_path_with_status(path, LogTarget::Crash);
                } else {
                    self.show_log_window = true;
                }
            }
            if let Some(action) = selected_action {
                match action {
                    crash::CrashActionKind::OpenUrl(url) => {
                        self.open_url_with_status(&url, LogTarget::Crash);
                    }
                    crash::CrashActionKind::OpenFolder(folder) => {
                        if let Err(error) = open_crash_folder(&report.game_directory, &folder) {
                            self.set_logged_status(LogLevel::Error, LogTarget::Crash, error);
                        }
                    }
                    crash::CrashActionKind::OpenLogs => {
                        self.selected_log_path = report.path.clone();
                        self.show_log_window = true;
                    }
                    crash::CrashActionKind::OpenSettings => {
                        self.settings_page = Some("Launcher");
                    }
                    crash::CrashActionKind::ForceUpdate => {
                        self.settings.force_update = true;
                        self.persist();
                        self.set_logged_status(
                            LogLevel::Info,
                            LogTarget::Crash,
                            tr("rust.status.crash.force-update"),
                        );
                    }
                    crash::CrashActionKind::RecommendedJava => {
                        self.settings.java_mode = "recommended".to_owned();
                        self.settings.java_path.clear();
                        self.persist();
                        self.set_logged_status(
                            LogLevel::Info,
                            LogTarget::Crash,
                            tr("rust.status.crash.recommended-java"),
                        );
                    }
                    crash::CrashActionKind::SetJvmArgument(argument) => {
                        let current = split_arguments(&self.settings.jvm_args);
                        if !current.iter().any(|saved| saved == &argument) {
                            self.settings.jvm_args = if self.settings.jvm_args.trim().is_empty() {
                                argument.clone()
                            } else {
                                format!("{} {argument}", self.settings.jvm_args.trim())
                            };
                            self.persist();
                        }
                        self.set_logged_status(
                            LogLevel::Info,
                            LogTarget::Crash,
                            tr("rust.status.crash.jvm-argument").replace("%0", &argument),
                        );
                    }
                }
            }
        }
    }
}
