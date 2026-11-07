use super::super::*;

pub(in crate::app) fn settings_path() -> PathBuf {
    let root = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    root.join("LegacyLauncher-Rust").join("config.json")
}

pub(in crate::app) fn launcher_log_path() -> PathBuf {
    settings_path()
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("logs")
        .join("launcher.log")
}

pub(in crate::app) fn default_legacy_config_path() -> PathBuf {
    let root = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    root.join(".tlauncher").join("legacy.properties")
}

pub(in crate::app) fn default_game_directory() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join(".minecraft")
}

pub(in crate::app) fn first_run_game_directory_suggestion(
    current_directory: &Path,
) -> Option<PathBuf> {
    if !source_directory_needs_migration_warning(current_directory) {
        return None;
    }

    let mut suggestions = Vec::new();
    if cfg!(windows) {
        suggestions.push(PathBuf::from(r"D:\Games\Minecraft"));
        suggestions.push(PathBuf::from(r"C:\Games\Minecraft"));
    }

    let system_root = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    if cfg!(target_os = "macos") {
        let application_support = system_root.join("Library").join("Application Support");
        suggestions.push(application_support.join(".minecraft"));
        suggestions.push(application_support.join("Minecraft"));
        suggestions.push(application_support.join(".tlauncher").join("develop"));
    } else {
        suggestions.push(system_root.join(".minecraft"));
        suggestions.push(system_root.join("Minecraft"));
        suggestions.push(system_root.join(".tlauncher").join("develop"));
    }

    suggestions.into_iter().find(|candidate| {
        !path_is_same_or_nested(candidate, current_directory)
            && !source_directory_needs_migration_warning(candidate)
    })
}

pub(in crate::app) fn source_directory_needs_migration_warning(path: &Path) -> bool {
    if !path.exists() {
        return fs::create_dir_all(path).is_err();
    }
    if !path.is_dir() {
        return true;
    }

    let mut entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => return true,
    };
    match entries.next() {
        None => false,
        Some(Err(_)) => true,
        Some(Ok(_)) => !path.join("tlauncher_profiles.json").is_file(),
    }
}

pub(in crate::app) fn path_is_same_or_nested(candidate: &Path, directory: &Path) -> bool {
    fn normalized(path: &Path) -> String {
        let resolved = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let text = resolved.to_string_lossy().replace('/', "\\");
        if cfg!(windows) {
            text.trim_end_matches('\\').to_ascii_lowercase()
        } else {
            text.trim_end_matches('\\').to_owned()
        }
    }

    let candidate = normalized(candidate);
    let directory = normalized(directory);
    candidate == directory
        || candidate
            .strip_prefix(&directory)
            .is_some_and(|remainder| remainder.starts_with('\\'))
}

pub(in crate::app) fn is_windows_arm_host() -> bool {
    if !cfg!(windows) {
        return false;
    }

    ["PROCESSOR_ARCHITEW6432", "PROCESSOR_ARCHITECTURE"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .any(|architecture| architecture.to_ascii_uppercase().starts_with("ARM"))
}

pub(in crate::app) fn load_settings() -> SavedSettings {
    load_settings_from_path(&settings_path())
}

pub(in crate::app) fn load_settings_from_path(path: &Path) -> SavedSettings {
    let saved_bytes = fs::read(path).ok();
    let saved_json = saved_bytes
        .as_deref()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(bytes).ok());
    let mut settings: SavedSettings = saved_bytes
        .as_deref()
        .and_then(|bytes| serde_json::from_slice(bytes).ok())
        .unwrap_or_default();
    if saved_json
        .as_ref()
        .and_then(serde_json::Value::as_object)
        .is_some_and(|values| {
            values.contains_key("max_memory") && !values.contains_key("automatic_memory")
        })
    {
        // Preserve manual values saved by Rust builds that predate Auto mode.
        settings.automatic_memory = false;
    }
    settings.malware_scan_version = settings.malware_scan_version.min(2);
    settings.auto_login_timeout_secs = settings.auto_login_timeout_secs.clamp(2, 10);
    if settings.legacy_config_path.trim().is_empty() {
        settings.legacy_config_path = default_legacy_config_path().to_string_lossy().into_owned();
    }
    settings.launcher_window_width = settings.launcher_window_width.clamp(700, 7680);
    settings.launcher_window_height = settings.launcher_window_height.clamp(600, 4320);
    if !matches!(settings.logger_mode.as_str(), "global" | "none") {
        settings.logger_mode = "none".to_owned();
    }
    if !matches!(
        settings.look_and_feel.as_str(),
        "off" | "system" | "auto" | "dark" | "light"
    ) {
        settings.look_and_feel = "auto".to_owned();
    }
    settings.launcher_font_size = if settings.launcher_font_size.is_finite() {
        settings.launcher_font_size.clamp(12.0, 18.0)
    } else {
        12.0
    };
    settings.legacy_font_size_revert = settings
        .legacy_font_size_revert
        .filter(|size| size.is_finite())
        .map(|size| size.clamp(12.0, 18.0));
    settings.log_window_width = settings.log_window_width.clamp(400, 4096);
    settings.log_window_height = settings.log_window_height.clamp(300, 2160);
    settings.log_window_x = settings.log_window_x.clamp(-16_384, 16_384);
    settings.log_window_y = settings.log_window_y.clamp(-16_384, 16_384);
    if !matches!(
        settings.login_form_direction.as_str(),
        "top_left"
            | "top"
            | "top_right"
            | "center_left"
            | "center"
            | "center_right"
            | "bottom_left"
            | "bottom"
            | "bottom_right"
    ) {
        settings.login_form_direction = "top_left".to_owned();
    }
    if settings.launch_action == "stay" {
        settings.launch_action = "nothing".to_owned();
    }
    if !matches!(settings.launch_action.as_str(), "hide" | "exit" | "nothing") {
        settings.launch_action = "hide".to_owned();
    }
    if !matches!(
        settings.gpu_preference.as_str(),
        "system" | "integrated" | "discrete"
    ) {
        settings.gpu_preference = "discrete".to_owned();
    }
    if !matches!(
        settings.java_mode.as_str(),
        "recommended" | "current" | "custom"
    ) {
        settings.java_mode = if settings.java_path.trim().is_empty() {
            "recommended".to_owned()
        } else {
            "custom".to_owned()
        };
    } else if settings.java_mode == "recommended" && !settings.java_path.trim().is_empty() {
        // Migrate Java paths saved by earlier Rust builds, which used a path as
        // an implicit custom-runtime selection.
        settings.java_mode = "custom".to_owned();
    }
    if !matches!(
        settings.optimized_java_args.as_str(),
        "none" | "default" | "g1gc" | "shenandoah" | "zgc"
    ) {
        settings.optimized_java_args = "default".to_owned();
    }
    if uuid::Uuid::parse_str(settings.client_id.trim()).is_err() {
        settings.client_id = uuid::Uuid::new_v4().to_string();
    }
    for (kind, enabled) in default_version_catalog_types() {
        settings
            .version_catalog_types
            .entry(kind)
            .or_insert(enabled);
    }
    for (kind, enabled) in default_version_alert_types() {
        settings.version_alert_types.entry(kind).or_insert(enabled);
    }
    settings.legacy_experiments.retain(|experiment| {
        matches!(
            experiment.as_str(),
            "zgc_generational"
                | "tenuring"
                | "max_xms"
                | "updated_lwjgl"
                | "updated_jna"
                | "more_gc_threads"
        )
    });
    settings
}

pub(in crate::app) fn load_external_settings_file(
    path: &Path,
    settings: &mut SavedSettings,
) -> Result<ExternalSettingsFile, String> {
    let is_json_path = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"));
    match fs::read(path) {
        Ok(bytes) => {
            if serde_json::from_slice::<SavedSettings>(&bytes).is_ok() {
                *settings = load_settings_from_path(path);
                return Ok(ExternalSettingsFile::RustJson(path.to_path_buf()));
            }
            if is_json_path {
                return Err(tr("rust.error.settings.custom-json")
                    .replace("%0", &path.display().to_string()));
            }
            let properties = parse_java_properties(&String::from_utf8_lossy(&bytes));
            if properties.is_empty() && !bytes.is_empty() {
                return Err(tr("rust.error.settings.custom-format")
                    .replace("%0", &path.display().to_string()));
            }
            if !properties.is_empty() {
                import_legacy_settings(path, settings)?;
            }
            Ok(ExternalSettingsFile::LegacyProperties(path.to_path_buf()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if is_json_path {
                Ok(ExternalSettingsFile::RustJson(path.to_path_buf()))
            } else {
                Ok(ExternalSettingsFile::LegacyProperties(path.to_path_buf()))
            }
        }
        Err(error) => Err(tr("rust.error.import.settings-read")
            .replace("%0", &path.display().to_string())
            .replace("%1", &error.to_string())),
    }
}

pub(in crate::app) fn import_legacy_settings(
    path: &std::path::Path,
    settings: &mut SavedSettings,
) -> Result<LegacySettingsImportSummary, String> {
    let bytes = fs::read(path).map_err(|error| {
        tr("rust.error.import.settings-read")
            .replace("%0", &path.display().to_string())
            .replace("%1", &error.to_string())
    })?;
    let properties = parse_java_properties(&String::from_utf8_lossy(&bytes));
    if properties.is_empty() {
        return Err(tr("rust.error.import.settings-empty").to_owned());
    }
    let compatible_version = properties
        .get("settings.version")
        .map(|value| value.trim().parse::<f64>().ok() == Some(3.0))
        .unwrap_or(true);
    if !compatible_version {
        return Ok(LegacySettingsImportSummary {
            applied: 0,
            skipped: properties.len(),
        });
    }
    let mut summary = LegacySettingsImportSummary::default();
    let mut apply = |key: &str, result: Option<()>| {
        if properties.contains_key(key) {
            if result.is_some() {
                summary.applied += 1;
            } else {
                summary.skipped += 1;
            }
        }
    };

    if let Some(value) = properties.get("client") {
        let client_id = uuid::Uuid::parse_str(value.trim()).ok();
        if let Some(client_id) = client_id.as_ref() {
            settings.client_id = client_id.to_string();
        }
        apply("client", client_id.map(|_| ()))
    }
    if let Some(value) = properties.get("experiments.enabled") {
        let mut experiments = HashSet::new();
        for experiment in value
            .split(';')
            .map(|value| value.trim().to_ascii_lowercase())
        {
            if matches!(
                experiment.as_str(),
                "zgc_generational"
                    | "tenuring"
                    | "max_xms"
                    | "updated_lwjgl"
                    | "updated_jna"
                    | "more_gc_threads"
            ) {
                experiments.insert(experiment);
            }
        }
        settings.legacy_experiments = experiments;
        apply("experiments.enabled", Some(()));
    }
    if let Some(value) = properties.get("jarscanner") {
        let scan_version = value
            .trim()
            .parse::<i64>()
            .ok()
            .map(|version| version.clamp(0, 2) as u32);
        if let Some(scan_version) = scan_version {
            settings.malware_scan_version = scan_version;
        }
        apply("jarscanner", scan_version.map(|_| ()));
    }

    if let Some(value) = properties.get("minecraft.gamedir") {
        let path = PathBuf::from(value.trim());
        let valid = !value.trim().is_empty() && path.is_absolute();
        if valid {
            settings.game_directory = path.to_string_lossy().into_owned();
        }
        apply("minecraft.gamedir", valid.then_some(()));
    }
    if let Some(value) = properties.get("login.version") {
        let version = value.trim();
        let valid = valid_version_id(version);
        if valid {
            settings.selected_version = version.to_owned();
        }
        apply("login.version", valid.then_some(()));
    }
    if let Some(value) = properties.get("minecraft.gamedir.separate") {
        let mode = match value.trim().to_ascii_lowercase().as_str() {
            "none" | "false" => Some(SeparateDirs::None),
            "family" | "true" => Some(SeparateDirs::Family),
            "version" => Some(SeparateDirs::Version),
            _ => None,
        };
        if let Some(mode) = mode {
            settings.separate_game_directories = mode;
        }
        apply("minecraft.gamedir.separate", mode.map(|_| ()));
    }
    if let Some(value) = properties.get("minecraft.onlaunch") {
        let action = value.trim().to_ascii_lowercase();
        let valid = matches!(action.as_str(), "hide" | "exit" | "nothing");
        if valid {
            settings.launch_action = action;
        }
        apply("minecraft.onlaunch", valid.then_some(()));
    }
    if let Some(value) = properties.get("minecraft.crash") {
        let enabled = parse_legacy_bool(value);
        if let Some(enabled) = enabled {
            settings.crash_detection_enabled = enabled;
        }
        apply("minecraft.crash", enabled.map(|_| ()));
    }
    if let Some(value) = properties.get("gui.logger.fullcommand") {
        let enabled = parse_legacy_bool(value);
        if let Some(enabled) = enabled {
            settings.show_full_game_command = enabled;
        }
        apply("gui.logger.fullcommand", enabled.map(|_| ()));
    }
    if let Some(value) = properties.get("gui.logger") {
        let mode = value.trim().to_ascii_lowercase();
        let valid = matches!(mode.as_str(), "global" | "none");
        if valid {
            settings.logger_mode = mode;
        }
        apply("gui.logger", valid.then_some(()));
    }
    if let Some(value) = properties.get("login.auto") {
        let enabled = parse_legacy_bool(value);
        if let Some(enabled) = enabled {
            settings.auto_login = enabled;
        }
        apply("login.auto", enabled.map(|_| ()));
    }
    if let Some(value) = properties.get("login.auto.timeout") {
        let timeout = value
            .trim()
            .parse::<u8>()
            .ok()
            .filter(|timeout| (2..=10).contains(timeout));
        if let Some(timeout) = timeout {
            settings.auto_login_timeout_secs = timeout;
        }
        apply("login.auto.timeout", timeout.map(|_| ()));
    }
    if let Some(value) = properties.get("gui.settings.blocked") {
        let blocked = parse_legacy_bool(value);
        if let Some(blocked) = blocked {
            settings.settings_blocked = blocked;
        }
        apply("gui.settings.blocked", blocked.map(|_| ()));
    }
    if let Some(value) = properties.get("feedback") {
        let acknowledged = parse_legacy_bool(value);
        if let Some(acknowledged) = acknowledged {
            settings.feedback_acknowledged = acknowledged;
        }
        apply("feedback", acknowledged.map(|_| ()));
    }
    if let Some(value) = properties.get("gui.size") {
        let dimensions = value
            .split(|character: char| matches!(character, 'x' | 'X' | '×' | ',' | ';' | ' '))
            .filter_map(|part| part.trim().parse::<u32>().ok())
            .collect::<Vec<_>>();
        let valid = dimensions.len() >= 2
            && (540..=7680).contains(&dimensions[0])
            && (360..=4320).contains(&dimensions[1]);
        if valid {
            settings.launcher_window_width = dimensions[0];
            settings.launcher_window_height = dimensions[1];
        }
        apply("gui.size", valid.then_some(()));
    }
    if let Some(value) = properties.get("gui.window") {
        let maximized = match value.trim().parse::<i32>().ok() {
            Some(0) => Some(false),
            Some(2 | 4 | 6) => Some(true),
            _ => None,
        };
        if let Some(maximized) = maximized {
            settings.launcher_maximized = maximized;
        }
        apply("gui.window", maximized.map(|_| ()))
    }
    for (key, destination, minimum, maximum) in [
        (
            "gui.logger.width",
            &mut settings.log_window_width,
            400,
            4096,
        ),
        (
            "gui.logger.height",
            &mut settings.log_window_height,
            300,
            2160,
        ),
    ] {
        if let Some(value) = properties.get(key) {
            let size = value
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|size| (minimum..=maximum).contains(size));
            if let Some(size) = size {
                *destination = size;
            }
            apply(key, size.map(|_| ()));
        }
    }
    for (key, destination) in [
        ("gui.logger.x", &mut settings.log_window_x),
        ("gui.logger.y", &mut settings.log_window_y),
    ] {
        if let Some(value) = properties.get(key) {
            let position = value
                .trim()
                .parse::<i32>()
                .ok()
                .filter(|position| (-16_384..=16_384).contains(position));
            if let Some(position) = position {
                *destination = position;
            }
            apply(key, position.map(|_| ()));
        }
    }
    if let Some(value) = properties.get("gui.font") {
        let font_size = value
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|size| size.is_finite() && (12.0..=18.0).contains(size));
        if let Some(font_size) = font_size {
            settings.launcher_font_size = font_size;
        }
        apply("gui.font", font_size.map(|_| ()))
    }
    if let Some(value) = properties.get("gui.font.old") {
        let previous_size = value
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|size| size.is_finite());
        if let Some(previous_size) = previous_size {
            settings.legacy_font_size_revert = Some(if (12.0..=18.0).contains(&previous_size) {
                previous_size
            } else {
                18.0
            });
        }
        apply("gui.font.old", previous_size.map(|_| ()))
    }
    if let Some(value) = properties.get("gui.background") {
        let path = PathBuf::from(value.trim());
        let valid = path.is_file()
            && fs::metadata(&path).is_ok_and(|metadata| metadata.len() <= 64 * 1024 * 1024)
            && image::image_dimensions(&path).is_ok();
        if valid {
            settings.background_path = path.to_string_lossy().into_owned();
        }
        apply("gui.background", valid.then_some(()));
    }
    if let Some(value) = properties.get("gui.theme") {
        let path = PathBuf::from(value.trim());
        let valid = !value.trim().is_empty() && path.is_file();
        if valid {
            settings.legacy_theme_path = path.to_string_lossy().into_owned();
        }
        apply("gui.theme", valid.then_some(()));
    }
    if let Some(value) = properties.get("gui.laf.v1.state") {
        let state = value.trim().to_ascii_lowercase();
        let valid = matches!(state.as_str(), "off" | "system" | "auto" | "dark" | "light");
        if valid {
            settings.look_and_feel = state;
        }
        apply("gui.laf.v1.state", valid.then_some(()));
    } else if let Some(value) = properties.get("gui.systemlookandfeel") {
        let enabled = parse_legacy_bool(value);
        if let Some(enabled) = enabled {
            settings.look_and_feel = if enabled { "system" } else { "off" }.to_owned();
        }
        apply("gui.systemlookandfeel", enabled.map(|_| ()))
    }
    if let Some(value) = properties.get("gui.direction.loginform") {
        let direction = value.trim().to_ascii_lowercase();
        let valid = matches!(
            direction.as_str(),
            "top_left"
                | "top"
                | "top_right"
                | "center_left"
                | "center"
                | "center_right"
                | "bottom_left"
                | "bottom"
                | "bottom_right"
        );
        if valid {
            settings.login_form_direction = direction;
        }
        apply("gui.direction.loginform", valid.then_some(()));
    }
    if let Some(value) = properties.get("minecraft.size") {
        let dimensions = value
            .split(|character: char| matches!(character, 'x' | 'X' | '×' | ',' | ';' | ' '))
            .filter_map(|part| part.trim().parse::<u32>().ok())
            .collect::<Vec<_>>();
        let valid = dimensions.len() >= 2
            && (320..=7680).contains(&dimensions[0])
            && (240..=4320).contains(&dimensions[1]);
        if valid {
            settings.screen_width = dimensions[0];
            settings.screen_height = dimensions[1];
        }
        apply("minecraft.size", valid.then_some(()));
    }
    if let Some(value) = properties.get("minecraft.fullscreen") {
        let setting = parse_legacy_bool(value);
        if let Some(setting) = setting {
            settings.fullscreen = setting;
        }
        apply("minecraft.fullscreen", setting.map(|_| ()));
    }
    if let Some(value) = properties.get("minecraft.gpu") {
        let preference = match value.trim().to_ascii_lowercase().as_str() {
            "system" | "default" => Some("system"),
            "integrated" => Some("integrated"),
            "discrete" => Some("discrete"),
            _ => None,
        };
        if let Some(preference) = preference {
            settings.gpu_preference = preference.to_owned();
        }
        apply("minecraft.gpu", preference.map(|_| ()));
    }
    if let Some(value) = properties.get("windows.gpuperf") {
        let enabled = parse_legacy_bool(value);
        if let Some(enabled) = enabled {
            settings.windows_gpu_performance = enabled;
        }
        apply("windows.gpuperf", enabled.map(|_| ()));
    }
    if let Some(value) = properties.get("minecraft.jre.dir") {
        let directory = value.trim();
        let valid = !directory.is_empty();
        if valid {
            settings.java_runtime_directory = directory.to_owned();
        }
        apply("minecraft.jre.dir", valid.then_some(()));
    }
    if let Some(value) = properties.get("minecraft.improvedargs.type") {
        let optimized_type = value.trim().to_ascii_lowercase();
        let valid = matches!(
            optimized_type.as_str(),
            "none" | "default" | "g1gc" | "shenandoah" | "zgc"
        );
        if valid {
            settings.optimized_java_args = optimized_type;
        }
        apply("minecraft.improvedargs.type", valid.then_some(()));
    } else if let Some(value) = properties.get("minecraft.improvedargs") {
        let enabled = parse_legacy_bool(value);
        if let Some(enabled) = enabled {
            settings.optimized_java_args = if enabled { "default" } else { "none" }.to_owned();
        }
        apply("minecraft.improvedargs", enabled.map(|_| ()))
    }
    if let Some(value) = properties.get("minecraft.jre.change-trust-store") {
        let enabled = parse_legacy_bool(value);
        if let Some(enabled) = enabled {
            settings.use_current_trust_store = enabled;
        }
        apply("minecraft.jre.change-trust-store", enabled.map(|_| ()))
    }
    if let Some(value) = properties.get("minecraft.wrapper-command") {
        settings.wrapper_command = value.trim().to_owned();
        apply("minecraft.wrapper-command", Some(()));
    }
    if let Some(value) = properties.get("connection.ssl") {
        let enabled = parse_legacy_bool(value);
        if let Some(enabled) = enabled {
            settings.verify_tls_certificates = enabled;
        }
        apply("connection.ssl", enabled.map(|_| ()));
    }
    if let Some(value) = properties.get("minecraft.jre.type") {
        let mode = value.trim().to_ascii_lowercase();
        let valid = matches!(mode.as_str(), "recommended" | "current" | "custom");
        if valid {
            settings.java_mode = mode;
            if settings.java_mode == "recommended" {
                settings.java_path.clear();
            }
        }
        apply("minecraft.jre.type", valid.then_some(()));
    }
    let imported_jre_type = properties
        .get("minecraft.jre.type")
        .map(|value| value.trim().to_ascii_lowercase());
    let custom_java_path = match imported_jre_type.as_deref() {
        Some("custom") => properties.get("minecraft.jre.custom.path"),
        Some(_) => None,
        None => properties.get("minecraft.cmd"),
    };
    if let Some(value) = custom_java_path {
        let path = value.trim();
        let valid = !path.is_empty();
        if valid {
            settings.java_mode = "custom".to_owned();
            settings.java_path = path.to_owned();
        }
        if imported_jre_type.as_deref() == Some("custom") {
            apply("minecraft.jre.custom.path", valid.then_some(()));
        } else {
            apply("minecraft.cmd", valid.then_some(()));
        }
    }
    for (key, destination) in [
        ("minecraft.javaargs", &mut settings.jvm_args),
        ("minecraft.args", &mut settings.game_args),
    ] {
        if let Some(value) = properties.get(key) {
            *destination = value.trim().to_owned();
            apply(key, Some(()));
        }
    }
    if let Some((key, value)) = properties
        .get("minecraft.xmx")
        .map(|value| ("minecraft.xmx", value))
        .or_else(|| {
            properties
                .get("minecraft.memory")
                .map(|value| ("minecraft.memory", value))
        })
    {
        let memory = value.trim();
        let megabytes = memory.parse::<u32>().ok();
        let total_memory_mb = diagnostics::total_physical_memory_mb()
            .unwrap_or(8192)
            .min(i32::MAX as u64) as u32;
        let old_memory_was_auto = key == "minecraft.memory"
            && megabytes == Some(default_legacy_xmx_mib(total_memory_mb, usize::BITS >= 64));
        if memory.eq_ignore_ascii_case("auto") || old_memory_was_auto || megabytes == Some(0) {
            settings.automatic_memory = true;
            apply(key, Some(()));
        } else if let Some(megabytes) = megabytes.filter(|value| (256..=262_144).contains(value)) {
            settings.automatic_memory = false;
            settings.max_memory = format!("{megabytes}M");
            settings.min_memory = format!("{}M", (megabytes / 2).clamp(128, 1024));
            apply(key, Some(()));
        } else if key == "minecraft.xmx" {
            // The Java MemorySlider falls back to Auto for unrecognized values.
            settings.automatic_memory = true;
            apply(key, Some(()));
        } else {
            apply(key, None);
        }
    }

    for (source_key, kind) in [
        ("minecraft.versions.snapshot", "snapshot"),
        ("minecraft.versions.modified", "modified"),
        ("minecraft.versions.old_beta", "old-beta"),
        ("minecraft.versions.old_alpha", "old-alpha"),
        ("minecraft.versions.launcher", "launcher"),
        ("minecraft.versions.pending", "pending"),
    ] {
        if let Some(value) = properties.get(source_key) {
            let enabled = parse_legacy_bool(value);
            if let Some(enabled) = enabled {
                settings
                    .version_catalog_types
                    .insert(kind.to_owned(), enabled);
            }
            apply(source_key, enabled.map(|_| ()));
        }
    }
    if let Some(value) = properties.get("minecraft.versions.sub.old_release") {
        let enabled = parse_legacy_bool(value);
        if let Some(enabled) = enabled {
            settings.version_show_old_releases = enabled;
        }
        apply("minecraft.versions.sub.old_release", enabled.map(|_| ()));
    }
    if let Some(value) = properties.get("minecraft.versions.only-installed") {
        let enabled = parse_legacy_bool(value);
        if let Some(enabled) = enabled {
            settings.version_only_installed = enabled;
        }
        apply("minecraft.versions.only-installed", enabled.map(|_| ()));
    }
    if let Some(value) = properties.get("minecraft.versions.sub.remote") {
        let enabled = parse_legacy_bool(value);
        if let Some(enabled) = enabled {
            settings.version_include_remote = enabled;
        }
        apply("minecraft.versions.sub.remote", enabled.map(|_| ()));
    }

    for kind in [
        "release",
        "snapshot",
        "modified",
        "old-beta",
        "old-alpha",
        "launcher",
        "pending",
    ] {
        let key = format!("gui.alerton.{kind}");
        if let Some(value) = properties.get(&key) {
            let enabled = parse_legacy_bool(value);
            if let Some(enabled) = enabled {
                settings
                    .version_alert_types
                    .insert(kind.to_owned(), enabled);
            }
            apply(&key, enabled.map(|_| ()));
        }
    }

    if let Some(value) = properties.get("notice.enabled") {
        let enabled = parse_legacy_bool(value);
        if let Some(enabled) = enabled {
            settings.notices_enabled = enabled;
        }
        apply("notice.enabled", enabled.map(|_| ()));
    }
    if let Some(value) = properties.get("gui.notices.enabled") {
        let enabled = parse_legacy_bool(value);
        if let Some(enabled) = enabled {
            settings.notice_side_panel_enabled = enabled;
        }
        apply("gui.notices.enabled", enabled.map(|_| ()));
    }
    if let Some(value) = properties.get("bootstrap.switchToBeta") {
        let enabled = parse_legacy_bool(value);
        if let Some(enabled) = enabled {
            settings.beta_updates = enabled;
        }
        apply("bootstrap.switchToBeta", enabled.map(|_| ()));
    }
    if let Some(value) = properties.get("notice.promoted") {
        let enabled = parse_legacy_bool(value);
        if let Some(enabled) = enabled {
            settings.promoted_notices_enabled = enabled;
        }
        apply("notice.promoted", enabled.map(|_| ()));
    }
    if let Some(value) = properties.get("minecraft.servers.promoted.ingame") {
        let enabled = parse_legacy_bool(value);
        if let Some(enabled) = enabled {
            settings.promoted_servers_in_game = enabled;
        }
        apply("minecraft.servers.promoted.ingame", enabled.map(|_| ()));
    }
    if let Some(value) = properties.get("minecraft.mods.removeUndesirable") {
        let enabled = parse_legacy_bool(value);
        if let Some(enabled) = enabled {
            settings.remove_undesirable_mods = enabled;
        }
        apply("minecraft.mods.removeUndesirable", enabled.map(|_| ()));
    } else if let Some(value) = properties.get("minecraft.deleteTlSkinCape") {
        let enabled = parse_legacy_bool(value);
        if let Some(enabled) = enabled {
            settings.remove_undesirable_mods = enabled;
        }
        apply("minecraft.deleteTlSkinCape", enabled.map(|_| ()));
    }
    if let Some(value) = properties.get("minecraft.deletePatchy") {
        let enabled = parse_legacy_bool(value);
        if let Some(enabled) = enabled {
            settings.delete_patchy = enabled;
        }
        apply("minecraft.deletePatchy", enabled.map(|_| ()));
    }
    for (key, value) in &properties {
        let Some(id) = key.strip_prefix("notice.id.") else {
            continue;
        };
        if let Ok(expiry) = value.trim().parse::<u64>() {
            settings.hidden_notices_until.insert(id.to_owned(), expiry);
            summary.applied += 1;
        } else {
            summary.skipped += 1;
        }
    }

    Ok(summary)
}

pub(in crate::app) fn write_external_settings_file(
    file: &ExternalSettingsFile,
    settings: &SavedSettings,
) -> Result<(), String> {
    match file {
        ExternalSettingsFile::RustJson(path) => {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                fs::create_dir_all(parent).map_err(|error| {
                    tr("rust.error.settings.create-directory")
                        .replace("%0", &parent.display().to_string())
                        .replace("%1", &error.to_string())
                })?;
            }
            let bytes = serde_json::to_vec_pretty(settings).map_err(|error| {
                tr("rust.error.settings.serialize-json").replace("%0", &error.to_string())
            })?;
            fs::write(path, bytes).map_err(|error| {
                tr("rust.error.settings.write-file")
                    .replace("%0", &path.display().to_string())
                    .replace("%1", &error.to_string())
            })
        }
        ExternalSettingsFile::LegacyProperties(path) => write_legacy_properties(path, settings),
    }
}

pub(in crate::app) fn legacy_properties_from_settings(
    settings: &SavedSettings,
) -> BTreeMap<String, String> {
    let mut properties = BTreeMap::new();
    let mut put = |key: &str, value: String| {
        properties.insert(key.to_owned(), value);
    };
    put("settings.version", "3".to_owned());
    put("client", settings.client_id.clone());
    let mut experiments = settings
        .legacy_experiments
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    experiments.sort();
    put("experiments.enabled", experiments.join(";"));
    put("jarscanner", settings.malware_scan_version.to_string());
    put("minecraft.gamedir", settings.game_directory.clone());
    put("login.version", settings.selected_version.clone());
    put(
        "minecraft.gamedir.separate",
        match settings.separate_game_directories {
            SeparateDirs::None => "none",
            SeparateDirs::Family => "family",
            SeparateDirs::Version => "version",
        }
        .to_owned(),
    );
    put("minecraft.onlaunch", settings.launch_action.clone());
    put(
        "minecraft.crash",
        settings.crash_detection_enabled.to_string(),
    );
    put(
        "gui.logger.fullcommand",
        settings.show_full_game_command.to_string(),
    );
    put("gui.logger", settings.logger_mode.clone());
    put("login.auto", settings.auto_login.to_string());
    put(
        "login.auto.timeout",
        settings.auto_login_timeout_secs.to_string(),
    );
    put(
        "gui.settings.blocked",
        settings.settings_blocked.to_string(),
    );
    put("feedback", settings.feedback_acknowledged.to_string());
    put(
        "gui.size",
        format!(
            "{}x{}",
            settings.launcher_window_width, settings.launcher_window_height
        ),
    );
    put(
        "gui.window",
        if settings.launcher_maximized {
            "6"
        } else {
            "0"
        }
        .to_owned(),
    );
    put("gui.logger.width", settings.log_window_width.to_string());
    put("gui.logger.height", settings.log_window_height.to_string());
    put("gui.logger.x", settings.log_window_x.to_string());
    put("gui.logger.y", settings.log_window_y.to_string());
    put("gui.font", settings.launcher_font_size.to_string());
    if let Some(size) = settings.legacy_font_size_revert {
        put("gui.font.old", size.to_string());
    }
    if !settings.background_path.trim().is_empty() {
        put("gui.background", settings.background_path.clone());
    }
    if !settings.legacy_theme_path.trim().is_empty() {
        put("gui.theme", settings.legacy_theme_path.clone());
    }
    put("gui.laf.v1.state", settings.look_and_feel.clone());
    put(
        "gui.direction.loginform",
        settings.login_form_direction.clone(),
    );
    put(
        "minecraft.size",
        format!("{}x{}", settings.screen_width, settings.screen_height),
    );
    put("minecraft.fullscreen", settings.fullscreen.to_string());
    put("minecraft.gpu", settings.gpu_preference.clone());
    put(
        "windows.gpuperf",
        settings.windows_gpu_performance.to_string(),
    );
    put("minecraft.jre.dir", settings.java_runtime_directory.clone());
    put(
        "minecraft.improvedargs.type",
        settings.optimized_java_args.clone(),
    );
    put(
        "minecraft.jre.change-trust-store",
        settings.use_current_trust_store.to_string(),
    );
    put(
        "connection.ssl",
        settings.verify_tls_certificates.to_string(),
    );
    put(
        "minecraft.wrapper-command",
        settings.wrapper_command.clone(),
    );
    put("minecraft.jre.type", settings.java_mode.clone());
    if settings.java_mode == "custom" && !settings.java_path.trim().is_empty() {
        put("minecraft.jre.custom.path", settings.java_path.clone());
    }
    put("minecraft.javaargs", settings.jvm_args.clone());
    put("minecraft.args", settings.game_args.clone());
    if settings.automatic_memory {
        put("minecraft.xmx", "auto".to_owned());
    } else if let Some(memory_mb) = parse_memory_size(&settings.max_memory) {
        put("minecraft.xmx", (memory_mb / 1024_u64.pow(2)).to_string());
    }
    for (kind, enabled) in &settings.version_catalog_types {
        let key = match kind.as_str() {
            "old-beta" => "minecraft.versions.old_beta",
            "old-alpha" => "minecraft.versions.old_alpha",
            other => match other {
                "snapshot" => "minecraft.versions.snapshot",
                "modified" => "minecraft.versions.modified",
                "launcher" => "minecraft.versions.launcher",
                "pending" => "minecraft.versions.pending",
                _ => continue,
            },
        };
        put(key, enabled.to_string());
    }
    put(
        "minecraft.versions.sub.old_release",
        settings.version_show_old_releases.to_string(),
    );
    put(
        "minecraft.versions.only-installed",
        settings.version_only_installed.to_string(),
    );
    put(
        "minecraft.versions.sub.remote",
        settings.version_include_remote.to_string(),
    );
    for (kind, enabled) in &settings.version_alert_types {
        if matches!(
            kind.as_str(),
            "release" | "snapshot" | "modified" | "old-beta" | "old-alpha" | "launcher" | "pending"
        ) {
            put(&format!("gui.alerton.{kind}"), enabled.to_string());
        }
    }
    put("notice.enabled", settings.notices_enabled.to_string());
    put(
        "gui.notices.enabled",
        settings.notice_side_panel_enabled.to_string(),
    );
    put("bootstrap.switchToBeta", settings.beta_updates.to_string());
    put(
        "notice.promoted",
        settings.promoted_notices_enabled.to_string(),
    );
    put(
        "minecraft.servers.promoted.ingame",
        settings.promoted_servers_in_game.to_string(),
    );
    put(
        "minecraft.mods.removeUndesirable",
        settings.remove_undesirable_mods.to_string(),
    );
    put("minecraft.deletePatchy", settings.delete_patchy.to_string());
    for (id, expiry) in &settings.hidden_notices_until {
        put(&format!("notice.id.{id}"), expiry.to_string());
    }
    properties
}

pub(in crate::app) fn escape_java_property_component(
    value: &str,
    escape_all_spaces: bool,
) -> String {
    let mut escaped = String::new();
    let mut first = true;
    for unit in value.encode_utf16() {
        match unit {
            0x20 if escape_all_spaces || first => escaped.push_str("\\ "),
            0x5c => escaped.push_str("\\\\"),
            0x3a | 0x3d if escape_all_spaces => {
                escaped.push('\\');
                escaped.push(char::from_u32(unit as u32).unwrap_or('?'));
            }
            0x23 | 0x21 if escape_all_spaces && first => {
                escaped.push('\\');
                escaped.push(char::from_u32(unit as u32).unwrap_or('?'));
            }
            0x09 => escaped.push_str("\\t"),
            0x0a => escaped.push_str("\\n"),
            0x0d => escaped.push_str("\\r"),
            0x0c => escaped.push_str("\\f"),
            0x21..=0x7e if !matches!(unit, 0x3a | 0x3d) || !escape_all_spaces => {
                escaped.push(char::from_u32(unit as u32).unwrap_or('?'));
            }
            _ => escaped.push_str(&format!("\\u{unit:04X}")),
        }
        first = false;
    }
    escaped
}

pub(in crate::app) fn finish_legacy_property_group(
    preserved_lines: &mut Vec<String>,
    group_lines: &mut Vec<String>,
    logical_line: &mut String,
    managed_keys: &HashSet<String>,
) {
    if !group_lines.is_empty() {
        let key = parse_java_properties(logical_line).into_keys().next();
        if key
            .as_ref()
            .is_none_or(|key| !managed_keys.contains(key) && !key.starts_with("notice.id."))
        {
            preserved_lines.append(group_lines);
        } else {
            group_lines.clear();
        }
    }
    logical_line.clear();
}

pub(in crate::app) fn write_legacy_properties(
    path: &Path,
    settings: &SavedSettings,
) -> Result<(), String> {
    let original = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(tr("rust.error.settings.read-file")
                .replace("%0", &path.display().to_string())
                .replace("%1", &error.to_string()));
        }
    };
    let properties = legacy_properties_from_settings(settings);
    let mut managed_keys = properties.keys().cloned().collect::<HashSet<_>>();
    managed_keys.extend(
        [
            "gui.background",
            "gui.theme",
            "gui.font.old",
            "gui.systemlookandfeel",
            "minecraft.improvedargs",
            "minecraft.deleteTlSkinCape",
            "minecraft.memory",
            "minecraft.xmx",
            "minecraft.cmd",
            "minecraft.jre.custom.path",
        ]
        .into_iter()
        .map(str::to_owned),
    );

    let mut preserved_lines = Vec::new();
    let mut group_lines = Vec::new();
    let mut logical_line = String::new();
    for raw_line in original.lines() {
        if logical_line.is_empty() {
            let trimmed = raw_line.trim_start();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
                preserved_lines.push(raw_line.to_owned());
                continue;
            }
            group_lines.push(raw_line.to_owned());
            logical_line.push_str(trimmed);
        } else {
            group_lines.push(raw_line.to_owned());
            logical_line.push_str(raw_line.trim_start());
        }
        let trailing_slashes = logical_line
            .chars()
            .rev()
            .take_while(|character| *character == '\\')
            .count();
        if trailing_slashes % 2 == 1 {
            logical_line.pop();
            continue;
        }
        finish_legacy_property_group(
            &mut preserved_lines,
            &mut group_lines,
            &mut logical_line,
            &managed_keys,
        );
    }
    finish_legacy_property_group(
        &mut preserved_lines,
        &mut group_lines,
        &mut logical_line,
        &managed_keys,
    );

    let mut contents = preserved_lines.join("\n");
    if !contents.is_empty() && !contents.ends_with('\n') {
        contents.push('\n');
    }
    for (key, value) in properties {
        contents.push_str(&escape_java_property_component(&key, true));
        contents.push('=');
        contents.push_str(&escape_java_property_component(&value, false));
        contents.push('\n');
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| {
            tr("rust.error.settings.create-directory")
                .replace("%0", &parent.display().to_string())
                .replace("%1", &error.to_string())
        })?;
    }
    fs::write(path, contents).map_err(|error| {
        tr("rust.error.settings.write-file")
            .replace("%0", &path.display().to_string())
            .replace("%1", &error.to_string())
    })
}
