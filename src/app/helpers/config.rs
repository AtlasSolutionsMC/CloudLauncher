use super::super::*;

pub(in crate::app) fn install_system_ui_font(ctx: &egui::Context) {
    let ctx = ctx.clone();
    let _ = std::thread::Builder::new()
        .name("system-ui-font-selection".to_owned())
        .spawn(move || {
            let mut collection = fontique::Collection::new(fontique::CollectionOptions::default());
            let mut source_cache =
                fontique::SourceCache::new(fontique::SourceCacheOptions::default());
            let mut query = collection.query(&mut source_cache);
            query.set_families([
                fontique::GenericFamily::SystemUi,
                fontique::GenericFamily::SansSerif,
            ]);
            let mut selected_font = None;
            query.matches_with(|font| {
                selected_font = Some((font.blob.as_ref().to_vec(), font.index));
                fontique::QueryStatus::Stop
            });

            let Some((bytes, index)) = selected_font else {
                return;
            };
            let mut fonts = egui::FontDefinitions::default();
            let font_name = "system-ui";
            fonts.font_data.insert(
                font_name.to_owned(),
                Arc::new(egui::FontData::from_blob(Arc::new(bytes), index)),
            );
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, font_name.to_owned());
            ctx.set_fonts(fonts);
            ctx.request_repaint();
        });
}

pub(in crate::app) fn parse_legacy_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" => Some(true),
        "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

pub(in crate::app) fn parse_cli_window_size(value: &str) -> Option<(u32, u32)> {
    let mut dimensions = value
        .split(|character| matches!(character, ';' | 'x' | 'X' | ','))
        .map(str::trim)
        .map(str::parse::<u32>);
    let width = dimensions.next()?.ok()?;
    let height = dimensions.next()?.ok()?;
    if dimensions.next().is_some()
        || !(320..=7680).contains(&width)
        || !(240..=4320).contains(&height)
    {
        return None;
    }
    Some((width, height))
}

pub(in crate::app) fn parse_command_line_overrides(arguments: &[String]) -> CommandLineOverrides {
    let mut overrides = CommandLineOverrides::default();
    let mut java_directory = None;
    let mut index = 0;

    while index < arguments.len() {
        let argument = arguments[index].as_str();
        index += 1;
        let Some(argument) = argument.strip_prefix("--") else {
            continue;
        };
        let (name, inline_value) = argument
            .split_once('=')
            .map_or((argument, None), |(name, value)| {
                (name, Some(value.to_owned()))
            });
        if name == "launch" {
            overrides.launch_automatically = true;
            continue;
        }
        if name == "debug" {
            overrides.debug_mode = true;
            continue;
        }
        let value = inline_value.or_else(|| {
            arguments
                .get(index)
                .filter(|value| !value.starts_with("--"))
                .cloned()
                .inspect(|_| index += 1)
        });
        let Some(value) = value else { continue };
        match name {
            "directory" => overrides.game_directory = Some(value),
            "java-executable" => overrides.java_path = Some(value),
            "java-directory" => java_directory = Some(value),
            "version" => overrides.selected_version = Some(value),
            "javaargs" => overrides.jvm_args = Some(value),
            "margs" => overrides.game_args = Some(value),
            "window" => overrides.window_size = parse_cli_window_size(&value),
            "background" => overrides.background_path = Some(value),
            "fullscreen" => overrides.fullscreen = parse_legacy_bool(&value),
            "theme" => overrides.theme_path = Some(value),
            "jre-dir" => overrides.java_runtime_directory = Some(value),
            "settings" => overrides.settings_path = Some(value),
            "username" => overrides.username = Some(value),
            "usertype" => overrides.user_type = Some(value),
            _ => {}
        }
    }

    if overrides.java_path.is_none()
        && let Some(directory) = java_directory
    {
        let java_executable = if cfg!(windows) { "javaw.exe" } else { "java" };
        overrides.java_path = Some(
            PathBuf::from(directory)
                .join("bin")
                .join(java_executable)
                .to_string_lossy()
                .into_owned(),
        );
    }
    overrides
}

pub(in crate::app) fn select_command_line_account(
    settings: &mut SavedSettings,
    username: &str,
    user_type: Option<&str>,
) {
    let mut accounts = Vec::new();
    for account in &settings.accounts {
        let kind = if settings
            .migrated_mojang_accounts
            .iter()
            .any(|name| name.eq_ignore_ascii_case(account))
        {
            "mojang"
        } else {
            "plain"
        };
        if account == username {
            accounts.push((account.clone(), kind));
        }
    }
    accounts.extend(
        settings
            .microsoft_accounts
            .iter()
            .filter(|account| account.username == username)
            .map(|account| (account.id.clone(), "minecraft")),
    );
    accounts.extend(
        settings
            .ely_accounts
            .iter()
            .filter(|account| account.display_name == username)
            .map(|account| (account.id.clone(), "ely")),
    );
    accounts.extend(
        settings
            .ely_legacy_accounts
            .iter()
            .filter(|account| account.display_name == username)
            .map(|account| (account.id.clone(), "ely_legacy")),
    );

    match accounts.as_slice() {
        [] if user_type.is_none_or(|kind| kind == "plain") => {
            if username.trim().is_empty() {
                return;
            }
            if !settings.accounts.iter().any(|account| account == username) {
                settings.accounts.push(username.to_owned());
            }
            settings.selected_account = Some(username.to_owned());
        }
        [] => {}
        [account] => settings.selected_account = Some(account.0.clone()),
        matches => {
            let matching = user_type
                .map(|kind| {
                    matches
                        .iter()
                        .filter(|(_, account_kind)| *account_kind == kind)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if matching.len() == 1 {
                settings.selected_account = Some(matching[0].0.clone());
            } else if user_type.is_none() {
                settings.selected_account = Some(matches[0].0.clone());
            }
        }
    }
}

pub(in crate::app) fn print_command_line_help() {
    println!(
        "Legacy Launcher — Rust\n\
Usage: legacylauncher-rust [options]\n\
  --directory <path>             Minecraft game directory\n\
  --java-executable <path>        Java executable\n\
  --java-directory <path>         Deprecated Java directory option\n\
  --version <id>                  Minecraft version to launch\n\
  --username <name>               Account display name to select\n\
  --usertype <type>               Account type for ambiguous names\n\
  --javaargs <args>               JVM arguments\n\
  --margs <args>                  Minecraft arguments\n\
  --window <width;height>         Game window size\n\
  --background <path>             Launcher background image\n\
  --fullscreen <true|false>       Toggle fullscreen\n\
  --theme <path>                  Legacy theme properties file\n\
  --jre-dir <path>                Mojang Java runtime directory\n\
  --settings <path>               Launcher settings file\n\
  --launch                        Launch automatically after startup\n\
  --debug                         Open the launcher log window\n\
  --requireMinecraftAccount       Require a licensed Minecraft account\n\
  --help                          Print this help and exit"
    );
}

pub(in crate::app) fn load_legacy_theme_palette(path: &str) -> Option<LegacyThemePalette> {
    if path.trim().is_empty() {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let properties = parse_java_properties(&String::from_utf8_lossy(&bytes));
    let color = |key: &str| {
        properties
            .get(key)
            .and_then(|value| parse_legacy_theme_color(value))
    };
    let arc = |key: &str| {
        properties
            .get(key)
            .and_then(|value| value.trim().parse::<u8>().ok())
            .filter(|value| *value <= 64)
    };
    let shadow_uses_border = properties
        .get("shadow")
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("border"));
    let icon_colors = properties
        .iter()
        .filter_map(|(key, value)| {
            let icon_name = key.strip_prefix("icon.color.")?;
            Some((icon_name.to_owned(), parse_legacy_theme_color(value)?))
        })
        .collect();
    Some(LegacyThemePalette {
        foreground: color("foreground"),
        semi_foreground: color("semiforeground"),
        background: color("background"),
        dark_theme_enabled: properties
            .get("darkTheme.enabled")
            .map(|value| value.trim().eq_ignore_ascii_case("true")),
        panel_background: color("panelbackground"),
        success: color("success"),
        failure: color("failure"),
        border: color("border.color"),
        border_main: color("border.main_panel"),
        border_additional: color("border.additional_panel"),
        border_settings: color("border.settings_panel"),
        border_size: properties
            .get("border.size")
            .and_then(|value| value.trim().parse::<u8>().ok())
            .filter(|value| *value <= 24)
            .map(f32::from),
        shadow: (!shadow_uses_border).then(|| color("shadow")).flatten(),
        shadow_uses_border,
        shadow_main: color("shadow.main_panel"),
        shadow_additional: color("shadow.additional_panel"),
        shadow_settings: color("shadow.settings_panel"),
        arc: arc("arc"),
        arc_main: arc("arc.main_panel"),
        arc_additional: arc("arc.additional_panel"),
        arc_settings: arc("arc.settings_panel"),
        icon_default_color: color("icon.defaultColor"),
        icon_colors,
    })
}

pub(in crate::app) fn parse_legacy_theme_color(value: &str) -> Option<Color32> {
    let channels = value
        .trim()
        .split('.')
        .map(str::trim)
        .map(str::parse::<u8>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if !(3..=4).contains(&channels.len()) {
        return None;
    }
    Some(Color32::from_rgba_unmultiplied(
        channels[0],
        channels[1],
        channels[2],
        channels.get(3).copied().unwrap_or(255),
    ))
}

pub(in crate::app) fn legacy_background_prefers_dark_assets(background: Color32) -> bool {
    background.r() <= 64 && background.g() <= 64 && background.b() <= 64
}

pub(in crate::app) fn apply_legacy_theme_palette(
    style: &mut egui::Style,
    palette: &LegacyThemePalette,
) {
    let visuals = &mut style.visuals;
    if let Some(background) = palette.background {
        visuals.window_fill = background;
        visuals.panel_fill = background;
    }
    if let Some(panel_background) = palette.panel_background {
        visuals.widgets.noninteractive.bg_fill = panel_background;
        visuals.faint_bg_color = panel_background;
        visuals.extreme_bg_color = panel_background;
        visuals.text_edit_bg_color = Some(panel_background);
    }
    if let Some(foreground) = palette.foreground {
        visuals.override_text_color = Some(foreground);
        visuals.widgets.noninteractive.fg_stroke.color = foreground;
        visuals.widgets.inactive.fg_stroke.color = foreground;
        visuals.widgets.hovered.fg_stroke.color = foreground;
        visuals.widgets.active.fg_stroke.color = foreground;
        visuals.widgets.open.fg_stroke.color = foreground;
    }
    if let Some(semi_foreground) = palette.semi_foreground {
        visuals.weak_text_color = Some(semi_foreground);
    }
    if let Some(success) = palette.success {
        visuals.hyperlink_color = success;
        visuals.selection.bg_fill = success;
    }
    if let Some(failure) = palette.failure {
        visuals.error_fg_color = failure;
    }
    if let Some(border) = palette.border_settings.or(palette.border) {
        visuals.window_stroke.color = border;
        visuals.widgets.noninteractive.bg_stroke.color = border;
    }
    if let Some(border) = palette.border_main {
        visuals.selection.bg_fill = border;
    }
    if let Some(border) = palette.border_additional {
        visuals.widgets.hovered.bg_stroke.color = border;
    }
    if let Some(border) = palette.border_size {
        visuals.window_stroke.width = border;
        visuals.widgets.noninteractive.bg_stroke.width = border;
    }
    if let Some(radius) = palette.arc_settings.or(palette.arc) {
        visuals.window_corner_radius = egui::CornerRadius::same(radius);
    }
    if let Some(radius) = palette.arc_additional.or(palette.arc) {
        visuals.menu_corner_radius = egui::CornerRadius::same(radius);
    }
    if let Some(shadow) = palette.shadow_for(
        palette.shadow_settings,
        palette.border_settings.or(palette.border),
    ) {
        visuals.window_shadow.color = shadow;
    }
    if let Some(shadow) = palette.shadow_for(
        palette.shadow_additional,
        palette.border_additional.or(palette.border),
    ) {
        visuals.popup_shadow.color = shadow;
    }
}

pub(in crate::app) fn default_version_catalog_types() -> HashMap<String, bool> {
    [
        ("snapshot", true),
        ("modified", true),
        ("old-beta", false),
        ("old-alpha", false),
        ("launcher", false),
        ("pending", true),
    ]
    .into_iter()
    .map(|(kind, enabled)| (kind.to_owned(), enabled))
    .collect()
}

pub(in crate::app) fn default_version_alert_types() -> HashMap<String, bool> {
    [
        "release",
        "snapshot",
        "modified",
        "old-beta",
        "old-alpha",
        "launcher",
        "pending",
        "unknown",
    ]
    .into_iter()
    .map(|kind| (kind.to_owned(), false))
    .collect()
}

pub(in crate::app) fn refresh_version_alert_index(
    manifest: &MojangVersionManifest,
    settings: &SavedSettings,
    first_update: bool,
) -> Option<String> {
    let cache_path = version_alert_cache_path(settings);
    let alerts_enabled = settings
        .version_alert_types
        .values()
        .any(|enabled| *enabled);
    if !settings.version_include_remote || !alerts_enabled {
        let _ = fs::remove_file(cache_path);
        return None;
    }

    let current = CachedVersionList {
        versions: manifest
            .versions
            .iter()
            .filter(|version| !version.id.is_empty())
            .map(|version| CachedVersion {
                id: version.id.clone(),
                version_type: version.version_type.to_ascii_lowercase(),
                time: (!version.time.trim().is_empty()).then(|| version.time.clone()),
            })
            .collect(),
    };
    let previous = fs::read(&cache_path)
        .ok()
        .filter(|bytes| bytes.len() <= 8 * 1024 * 1024)
        .and_then(|bytes| serde_json::from_slice::<CachedVersionList>(&bytes).ok());
    let had_previous = previous.is_some();
    let alert = previous.and_then(|previous| {
        let old_ids: HashSet<&str> = previous
            .versions
            .iter()
            .map(|version| version.id.as_str())
            .collect();
        let newly_available = current
            .versions
            .iter()
            .filter(|version| {
                !old_ids.contains(version.id.as_str())
                    && settings
                        .version_alert_types
                        .get(&version.version_type)
                        .copied()
                        .unwrap_or(false)
            })
            .cloned()
            .collect::<Vec<_>>();
        (!newly_available.is_empty())
            .then(|| format_version_update_alert(&newly_available, first_update))
    });
    if had_previous && alert.is_none() {
        return None;
    }

    if let Some(parent) = cache_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(&current) {
        let _ = fs::write(cache_path, bytes);
    }
    alert
}

pub(in crate::app) fn version_alert_cache_path(settings: &SavedSettings) -> PathBuf {
    let configured = PathBuf::from(settings.game_directory.trim());
    let game_directory = if configured.is_absolute() {
        configured
    } else {
        default_game_directory()
    };
    game_directory.join("versions").join("versions.json")
}

pub(in crate::app) fn format_version_update_alert(
    versions: &[CachedVersion],
    first_update: bool,
) -> String {
    let mut groups: BTreeMap<&str, Vec<&CachedVersion>> = BTreeMap::new();
    for version in versions {
        let category = match version.version_type.as_str() {
            "release" => "release",
            "snapshot" => "snapshot",
            _ => "unknown",
        };
        groups.entry(category).or_default().push(version);
    }

    let mut message = tr(if first_update {
        "version.manager.alert.header.found.welcome"
    } else {
        "version.manager.alert.header.found"
    })
    .to_owned();
    message.push(' ');

    if versions.len() == 1 {
        let version = &versions[0];
        let category = match version.version_type.as_str() {
            "release" => "release",
            "snapshot" => "snapshot",
            _ => "unknown",
        };
        message.push_str(&tr_dynamic(&format!(
            "version.manager.alert.header.single.{category}"
        )));
        message.push('\n');
        append_alert_version(&mut message, version);
        return message;
    }

    message.push_str(tr("version.manager.alert.header.multiple"));
    message.push('\n');
    for (category, grouped) in groups {
        let plural = if grouped.len() == 1 {
            "single"
        } else {
            "multiple"
        };
        message.push('\n');
        message.push_str(&tr_dynamic(&format!(
            "version.manager.alert.{category}.{plural}"
        )));
        message.push('\n');
        for version in grouped.iter().take(4) {
            append_alert_version(&mut message, version);
        }
        if grouped.len() > 4 {
            message.push_str(
                &tr("version.manager.alert.more").replace("%0", &(grouped.len() - 4).to_string()),
            );
            message.push('\n');
        }
    }
    message
}

pub(in crate::app) fn append_alert_version(message: &mut String, version: &CachedVersion) {
    message.push_str("– ");
    message.push_str(&version.id);
    message.push_str(" (");
    message.push_str(&version_time_ago(version.time.as_deref()));
    message.push_str(")\n");
}

pub(in crate::app) fn version_time_ago(value: Option<&str>) -> String {
    let Some(value) = value else {
        return tr("version.manager.alert.time.unknown").to_owned();
    };
    let Ok(published) = OffsetDateTime::parse(value, &Rfc3339) else {
        return tr("version.manager.alert.time.unknown").to_owned();
    };
    let seconds = (OffsetDateTime::now_utc() - published)
        .whole_seconds()
        .max(0) as u64;
    if seconds < 60 {
        return tr("version.manager.alert.time.justnow").to_owned();
    }
    let (unit, count) = if seconds < 3_600 {
        ("minute", seconds / 60)
    } else if seconds < 86_400 {
        ("hour", seconds / 3_600)
    } else if seconds < 14 * 86_400 {
        ("day", seconds / 86_400)
    } else {
        return tr("version.manager.alert.time.longtimeago").to_owned();
    };
    let plural_key = format!("version.manager.alert.time.{unit}.0.plural");
    let plural_forms = tr_dynamic(&plural_key);
    let forms = plural_forms.split(';').collect::<Vec<_>>();
    let form = forms
        .get(usize::from(count != 1).min(forms.len().saturating_sub(1)))
        .copied()
        .unwrap_or("%n");
    let quantity = form.replace("%n", &count.to_string());
    tr_dynamic(&format!("version.manager.alert.time.{unit}")).replace("%0", &quantity)
}

pub(in crate::app) fn default_version_type_enabled(kind: &str) -> bool {
    matches!(kind, "snapshot" | "modified" | "pending")
}

pub(in crate::app) fn normalize_catalog_type(kind: &str) -> String {
    kind.trim().to_ascii_lowercase().replace('_', "-")
}

pub(in crate::app) fn parse_java_properties(contents: &str) -> HashMap<String, String> {
    let mut properties = HashMap::new();
    let mut logical_line = String::new();
    for raw_line in contents.lines() {
        let line = if logical_line.is_empty() {
            let trimmed = raw_line.trim_start();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
                continue;
            }
            trimmed
        } else {
            raw_line.trim_start()
        };
        logical_line.push_str(line);
        let trailing_slashes = logical_line
            .chars()
            .rev()
            .take_while(|ch| *ch == '\\')
            .count();
        if trailing_slashes % 2 == 1 {
            logical_line.pop();
            continue;
        }

        let mut escaped = false;
        let mut separator = None;
        for (index, character) in logical_line.char_indices() {
            if !escaped && (character == '=' || character == ':' || character.is_whitespace()) {
                separator = Some(index);
                break;
            }
            if character == '\\' {
                escaped = !escaped;
            } else {
                escaped = false;
            }
        }
        if let Some(index) = separator {
            let key = unescape_java_property(&logical_line[..index]);
            let mut value_start = index;
            while logical_line[value_start..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
            {
                value_start += logical_line[value_start..]
                    .chars()
                    .next()
                    .unwrap()
                    .len_utf8();
            }
            if logical_line[value_start..].starts_with('=')
                || logical_line[value_start..].starts_with(':')
            {
                value_start += 1;
            }
            while logical_line[value_start..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
            {
                value_start += logical_line[value_start..]
                    .chars()
                    .next()
                    .unwrap()
                    .len_utf8();
            }
            properties.insert(key, unescape_java_property(&logical_line[value_start..]));
        } else if !logical_line.is_empty() {
            properties.insert(unescape_java_property(&logical_line), String::new());
        }
        logical_line.clear();
    }
    if !logical_line.is_empty() {
        properties.insert(unescape_java_property(&logical_line), String::new());
    }
    properties
}

pub(in crate::app) fn unescape_java_property(value: &str) -> String {
    let mut units = Vec::new();
    let mut chars = value.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '\\' {
            let mut encoded = [0; 2];
            units.extend(character.encode_utf16(&mut encoded).iter().copied());
            continue;
        }
        let Some(escaped) = chars.next() else {
            units.push('\\' as u16);
            break;
        };
        match escaped {
            't' => units.push('\t' as u16),
            'n' => units.push('\n' as u16),
            'r' => units.push('\r' as u16),
            'f' => units.push('\u{000c}' as u16),
            'u' => {
                while chars.peek() == Some(&'u') {
                    chars.next();
                }
                let mut hex = String::new();
                for _ in 0..4 {
                    if let Some(character) = chars.next() {
                        hex.push(character);
                    }
                }
                if let Ok(unit) = u16::from_str_radix(&hex, 16) {
                    units.push(unit);
                }
            }
            other => units.push(other as u16),
        }
    }
    String::from_utf16_lossy(&units)
}

pub(in crate::app) fn legacy_mojang_profile(user: &serde_json::Value) -> Option<(String, String)> {
    let selected = user.get("selectedProfile")?;
    let selected_id = selected
        .get("id")
        .and_then(serde_json::Value::as_str)
        .or_else(|| selected.as_str())?;
    let profile_uuid = uuid::Uuid::parse_str(selected_id).ok()?;
    let normalized_uuid = profile_uuid.to_string();
    let selected_name = selected
        .get("name")
        .and_then(serde_json::Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned);
    let name = selected_name.or_else(|| {
        let profiles = user.get("profiles")?;
        let candidates: Vec<&serde_json::Value> = match profiles {
            serde_json::Value::Array(profiles) => profiles.iter().collect(),
            serde_json::Value::Object(profiles) => profiles.values().collect(),
            _ => return None,
        };
        candidates
            .into_iter()
            .find(|profile| {
                profile
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|id| uuid::Uuid::parse_str(id).ok())
                    .is_some_and(|id| id == profile_uuid)
            })
            .and_then(|profile| profile.get("name"))
            .and_then(serde_json::Value::as_str)
            .filter(|name| !name.trim().is_empty())
            .map(str::to_owned)
    })?;
    Some((name, normalized_uuid))
}
