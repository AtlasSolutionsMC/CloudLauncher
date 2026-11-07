use super::super::*;

pub(in crate::app) fn render_notice_text(
    ui: &mut egui::Ui,
    notice: &notices::Notice,
    enabled: bool,
    action: &mut Option<notices::NoticeAction>,
) {
    render_rich_text(ui, &notice.text_lines, enabled, action, egui::Align::Center);
}

pub(in crate::app) fn render_notice_html(
    ui: &mut egui::Ui,
    html: &str,
    enabled: bool,
    action: &mut Option<notices::NoticeAction>,
) {
    let (_, lines) = notices::parse_safe_html(html);
    render_rich_text(ui, &lines, enabled, action, egui::Align::Min);
}

pub(in crate::app) fn render_rich_text(
    ui: &mut egui::Ui,
    lines: &[Vec<notices::RichTextSpan>],
    enabled: bool,
    action: &mut Option<notices::NoticeAction>,
    alignment: egui::Align,
) {
    ui.with_layout(egui::Layout::top_down(alignment), |ui| {
        for line in lines {
            if line.is_empty() {
                ui.add_space(8.0);
                continue;
            }
            ui.horizontal_wrapped(|ui| {
                for span in line {
                    let mut text = RichText::new(&span.text);
                    if span.bold {
                        text = text.strong();
                    }
                    if span.italic {
                        text = text.italics();
                    }
                    if span.underline {
                        text = text.underline();
                    }
                    if let Some(url) = &span.link {
                        text = text.color(ui.visuals().hyperlink_color).underline();
                        let response =
                            ui.add_enabled(enabled, egui::Label::new(text).sense(Sense::click()));
                        let response = response.on_hover_text(tr("rust.notice.action.open-link"));
                        if response.clicked() {
                            let label = if span.text.trim().is_empty() {
                                tr("rust.notice.action.open").to_owned()
                            } else {
                                span.text.clone()
                            };
                            *action = Some(notices::NoticeAction::Url {
                                label,
                                url: url.clone(),
                            });
                        }
                    } else {
                        ui.add_enabled_ui(enabled, |ui| {
                            ui.label(text);
                        });
                    }
                }
            });
        }
    });
}

pub(in crate::app) fn settings_page_title(title: &str) -> &'static str {
    tr(match title {
        "About us" => "settings.tab.about",
        "Minecraft" => "settings.tab.minecraft",
        "Launcher" | "Launcher & game preferences" => "settings.tab.tlauncher",
        "Manage versions" => "rust.settings.page.versions",
        "Manage accounts" => "rust.settings.page.accounts",
        _ => "rust.settings.page.preferences",
    })
}

pub(in crate::app) fn look_and_feel_label_key(state: &str) -> &'static str {
    match state {
        "off" => "settings.laf.state.off",
        "system" => "settings.laf.state.system",
        "dark" => "settings.laf.state.dark",
        "light" => "settings.laf.state.light",
        _ => "settings.laf.state.auto",
    }
}

#[derive(Clone, Copy)]
pub(in crate::app) struct JavaSceneLayout {
    pub login_top_left: Pos2,
    pub panel_top_left: Pos2,
    pub show_login: bool,
}

pub(in crate::app) fn java_login_position(rect: Rect, direction: &str, size: Vec2) -> Pos2 {
    const MARGIN: f32 = 10.0;
    let x = if direction.ends_with("right") {
        rect.right() - MARGIN - size.x
    } else if matches!(direction, "top" | "center" | "bottom") {
        rect.center().x - size.x / 2.0
    } else {
        rect.left() + MARGIN
    };
    let y = if direction.starts_with("top") {
        rect.top() + MARGIN
    } else if direction.starts_with("bottom") {
        rect.bottom() - MARGIN - size.y
    } else {
        rect.center().y - size.y / 2.0
    };
    Pos2::new(x, y)
}

pub(in crate::app) fn java_scene_layout(
    rect: Rect,
    direction: &str,
    login_size: Vec2,
    panel_size: Vec2,
) -> JavaSceneLayout {
    const MARGIN: f32 = 10.0;
    const GAP: f32 = 15.0;

    let horizontal = if direction.ends_with("right") {
        -1 // right aligned
    } else if matches!(direction, "top" | "center" | "bottom") {
        0 // centered
    } else {
        1 // left aligned
    };
    let vertical = if direction.starts_with("top") {
        1 // top aligned
    } else if direction.starts_with("bottom") {
        -1 // bottom aligned
    } else {
        0 // centered
    };

    let total_width = login_size.x + GAP + panel_size.x;
    // DefaultScene.updateCoords hides the login form once the combined panels no longer fit.
    let show_login = rect.width() > total_width;
    let content_x = if horizontal < 0 {
        rect.right() - MARGIN - total_width
    } else if horizontal > 0 {
        rect.left() + MARGIN
    } else {
        rect.center().x - total_width / 2.0
    };

    let login_y = if vertical > 0 {
        rect.top() + MARGIN
    } else if vertical < 0 {
        rect.bottom() - MARGIN - login_size.y
    } else {
        rect.center().y - login_size.y / 2.0
    };
    let panel_y = if vertical > 0 {
        rect.top() + MARGIN
    } else if vertical < 0 {
        rect.bottom() - MARGIN - panel_size.y
    } else {
        rect.center().y - panel_size.y / 2.0
    };

    if show_login {
        let login_x = if horizontal < 0 {
            content_x + panel_size.x + GAP
        } else {
            content_x
        };
        let panel_x = if horizontal < 0 {
            content_x
        } else {
            content_x + login_size.x + GAP
        };
        JavaSceneLayout {
            login_top_left: Pos2::new(login_x, login_y),
            panel_top_left: Pos2::new(panel_x, panel_y),
            show_login: true,
        }
    } else {
        JavaSceneLayout {
            login_top_left: Pos2::new(rect.right() + MARGIN, rect.top()),
            panel_top_left: Pos2::new(
                rect.center().x - panel_size.x / 2.0,
                rect.center().y - panel_size.y / 2.0,
            ),
            show_login: false,
        }
    }
}

pub(in crate::app) fn login_form_direction_label_key(direction: &str) -> &'static str {
    match direction {
        "top" => "settings.direction.top",
        "top_right" => "settings.direction.top_right",
        "center_left" => "settings.direction.center_left",
        "center" => "settings.direction.center",
        "center_right" => "settings.direction.center_right",
        "bottom_left" => "settings.direction.bottom_left",
        "bottom" => "settings.direction.bottom",
        "bottom_right" => "settings.direction.bottom_right",
        _ => "settings.direction.top_left",
    }
}

pub(in crate::app) fn parse_launch_progress(message: &str) -> Option<f32> {
    let (_, value) = message.rsplit_once(": ")?;
    if let Some(percent) = value.strip_suffix('%') {
        return percent
            .trim()
            .parse::<f32>()
            .ok()
            .map(|percent| percent.clamp(0.0, 100.0) / 100.0);
    }
    let (current, total) = value.split_once('/')?;
    let current = current.trim().parse::<f32>().ok()?;
    let total = total.trim().parse::<f32>().ok()?;
    (total > 0.0).then(|| (current / total).clamp(0.0, 1.0))
}

pub(in crate::app) fn format_data_rate(bytes_per_second: f64) -> String {
    if !bytes_per_second.is_finite() || bytes_per_second < 0.0 {
        return "unknown".to_owned();
    }
    if bytes_per_second >= 1024.0 * 1024.0 {
        format!("{:.1} MiB/s", bytes_per_second / (1024.0 * 1024.0))
    } else if bytes_per_second >= 1024.0 {
        format!("{:.0} KiB/s", bytes_per_second / 1024.0)
    } else {
        format!("{bytes_per_second:.0} B/s")
    }
}

pub(in crate::app) fn format_duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "unknown".to_owned();
    }
    let seconds = seconds.round() as u64;
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}
