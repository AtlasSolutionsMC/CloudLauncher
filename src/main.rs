mod auth;
mod bootstrap;
mod catalog;
mod connectivity;
mod crash;
mod diagnostics;
mod ely;
mod ely_legacy;
mod github_update;
mod http;
mod malware;
mod notices;
mod support;
mod vault;

use eframe::egui::{
    self, Align2, Color32, ComboBox, Context, Id, Margin, Pos2, Rect, RichText, Sense, Stroke,
    TextureHandle, TextureOptions, Vec2,
};
use minecraft_java_rs_core::{
    launcher::{
        Launcher,
        events::LaunchEvent,
        options::{JavaOptions, LaunchOptions, LoaderConfig, MemoryConfig, ScreenConfig},
    },
    models::loader::LoaderType,
    models::minecraft::{Authenticator, MojangVersionManifest, XboxAccount},
    utils::auth::offline_uuid,
};
use serde::{Deserialize, Deserializer, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

const EN_US: &str = include_str!("../assets/lang/lang_en_US.properties");
const FALLBACK_VERSIONS: &[&str] = &["26.3", "1.21.11", "1.21.10", "1.20.1", "1.8.9"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct SavedSettings {
    accounts: Vec<String>,
    migrated_mojang_accounts: Vec<String>,
    offline_account_uuids: HashMap<String, String>,
    offline_ely_skins: HashMap<String, bool>,
    microsoft_accounts: Vec<MicrosoftAccountProfile>,
    ely_accounts: Vec<ElyAccountProfile>,
    ely_legacy_accounts: Vec<ElyLegacyAccountProfile>,
    selected_account: Option<String>,
    selected_version: String,
    auto_login: bool,
    auto_login_timeout_secs: u8,
    settings_blocked: bool,
    feedback_acknowledged: bool,
    force_update: bool,
    beta_updates: bool,
    client_id: String,
    notices_enabled: bool,
    notice_side_panel_enabled: bool,
    promoted_notices_enabled: bool,
    promoted_servers_in_game: bool,
    logger_mode: String,
    show_full_game_command: bool,
    crash_detection_enabled: bool,
    remove_undesirable_mods: bool,
    malware_scan_version: u32,
    delete_patchy: bool,
    launcher_font_size: f32,
    legacy_font_size_revert: Option<f32>,
    launcher_window_width: u32,
    launcher_window_height: u32,
    launcher_maximized: bool,
    look_and_feel: String,
    log_window_width: u32,
    log_window_height: u32,
    log_window_x: i32,
    log_window_y: i32,
    login_form_direction: String,
    hidden_notices_until: HashMap<String, u64>,
    servers: Vec<SavedServer>,
    servers_cache_root: String,
    background_path: String,
    legacy_theme_path: String,
    game_directory: String,
    legacy_profiles_path: String,
    legacy_config_path: String,
    min_memory: String,
    max_memory: String,
    automatic_memory: bool,
    java_path: String,
    java_mode: String,
    java_runtime_directory: String,
    optimized_java_args: String,
    use_current_trust_store: bool,
    verify_tls_certificates: bool,
    wrapper_command: String,
    legacy_experiments: HashSet<String>,
    screen_width: u32,
    screen_height: u32,
    fullscreen: bool,
    gpu_preference: String,
    windows_gpu_performance: bool,
    jvm_args: String,
    game_args: String,
    loader_type: String,
    loader_build: String,
    #[serde(default, deserialize_with = "deserialize_separate_dirs")]
    separate_game_directories: SeparateDirs,
    launch_action: String,
    version_only_installed: bool,
    version_include_remote: bool,
    version_show_old_releases: bool,
    version_catalog_types: HashMap<String, bool>,
    version_alert_types: HashMap<String, bool>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum SeparateDirs {
    Family,
    Version,
    #[default]
    None,
}

fn deserialize_separate_dirs<'de, D>(deserializer: D) -> Result<SeparateDirs, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        // Java's legacy boolean migration maps `true` to FAMILY.
        serde_json::Value::Bool(true) => SeparateDirs::Family,
        serde_json::Value::Bool(false) => SeparateDirs::None,
        serde_json::Value::String(value) => match value.to_ascii_lowercase().as_str() {
            "family" => SeparateDirs::Family,
            "version" => SeparateDirs::Version,
            _ => SeparateDirs::None,
        },
        _ => SeparateDirs::None,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MicrosoftAccountProfile {
    id: String,
    username: String,
    uuid: String,
    xuid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ElyLegacyAccountProfile {
    id: String,
    username: String,
    display_name: String,
    uuid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ElyAccountProfile {
    id: String,
    account_id: i64,
    username: String,
    display_name: String,
    uuid: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct SavedServer {
    name: String,
    address: String,
    #[serde(default)]
    hide_address: bool,
    #[serde(default)]
    accept_textures: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CachedVersion {
    id: String,
    #[serde(rename = "type")]
    version_type: String,
    #[serde(default)]
    time: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct CachedVersionList {
    versions: Vec<CachedVersion>,
}

#[derive(Clone)]
struct PendingPromotedServerLaunch {
    server: SavedServer,
    current_version: String,
    family_versions: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct MinecraftServerList {
    servers: Vec<MinecraftServerRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MinecraftServerRecord {
    name: String,
    #[serde(rename = "ip")]
    address: String,
    #[serde(default, rename = "hideAddress")]
    hide_address: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "acceptTextures"
    )]
    accept_textures: Option<bool>,
}

#[derive(Default)]
struct LegacyImportSummary {
    offline: usize,
    microsoft: usize,
    ely: usize,
    ely_legacy: usize,
    mojang_migrated: usize,
    skipped: usize,
}

#[derive(Default)]
struct LegacySettingsImportSummary {
    applied: usize,
    skipped: usize,
}

#[derive(Debug, Clone)]
struct CrashReport {
    path: Option<PathBuf>,
    game_directory: PathBuf,
    summary: String,
    suggestions: Vec<crash::CrashSuggestion>,
    log: String,
}

#[derive(Clone, Copy)]
enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    fn label(self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }
}

#[derive(Clone, Copy)]
enum LogTarget {
    Launcher,
    Authentication,
    VersionCatalog,
    VersionManager,
    MinecraftDownload,
    Minecraft,
    MinecraftCommand,
    JavaRuntime,
    Mods,
    Update,
    Bootstrap,
    Notices,
    Connectivity,
    Diagnostics,
    Servers,
    Settings,
    Crash,
}

impl LogTarget {
    fn label(self) -> &'static str {
        match self {
            Self::Launcher => "LegacyLauncher",
            Self::Authentication => "Authentication",
            Self::VersionCatalog => "VersionCatalog",
            Self::VersionManager => "VersionManager",
            Self::MinecraftDownload => "MinecraftDownload",
            Self::Minecraft => "Minecraft",
            Self::MinecraftCommand => "MinecraftCommand",
            Self::JavaRuntime => "JavaRuntime",
            Self::Mods => "Mods",
            Self::Update => "Update",
            Self::Bootstrap => "Bootstrap",
            Self::Notices => "Notices",
            Self::Connectivity => "Connectivity",
            Self::Diagnostics => "Diagnostics",
            Self::Servers => "Servers",
            Self::Settings => "Settings",
            Self::Crash => "Crash",
        }
    }
}

#[derive(Clone)]
struct LogRecord {
    timestamp: String,
    level: LogLevel,
    target: LogTarget,
    message: String,
}

impl LogRecord {
    fn render(&self) -> String {
        format!(
            "> {} [{}] [{}] {}",
            self.timestamp,
            self.target.label(),
            self.level.label(),
            self.message
        )
    }

    fn rendered_len(&self) -> usize {
        9 + self.timestamp.len()
            + self.target.label().len()
            + self.level.label().len()
            + self.message.len()
    }
}

struct LaunchUiMessage {
    message: String,
    level: LogLevel,
    target: LogTarget,
}

enum LaunchUiEvent {
    Message(LaunchUiMessage),
    LongMalwareScanConfirmation(tokio::sync::oneshot::Sender<bool>),
    MalwareDetected(malware::MalwareDetection),
    GameStarted,
    Finished,
}

impl MicrosoftAccountProfile {
    fn storage_key(&self) -> String {
        format!("microsoft:{}", self.uuid)
    }
}

impl ElyLegacyAccountProfile {
    fn storage_key(&self) -> String {
        format!("ely-legacy:{}", self.uuid)
    }
}

impl ElyAccountProfile {
    fn storage_key(&self) -> String {
        format!("ely:{}", self.uuid)
    }
}

impl Default for SavedSettings {
    fn default() -> Self {
        Self {
            accounts: Vec::new(),
            migrated_mojang_accounts: Vec::new(),
            offline_account_uuids: HashMap::new(),
            offline_ely_skins: HashMap::new(),
            microsoft_accounts: Vec::new(),
            ely_accounts: Vec::new(),
            ely_legacy_accounts: Vec::new(),
            selected_account: None,
            selected_version: "26.3".to_owned(),
            auto_login: false,
            auto_login_timeout_secs: 3,
            settings_blocked: false,
            feedback_acknowledged: false,
            force_update: false,
            beta_updates: false,
            client_id: uuid::Uuid::new_v4().to_string(),
            notices_enabled: true,
            notice_side_panel_enabled: true,
            promoted_notices_enabled: true,
            promoted_servers_in_game: true,
            logger_mode: "none".to_owned(),
            show_full_game_command: false,
            crash_detection_enabled: true,
            remove_undesirable_mods: true,
            malware_scan_version: 0,
            delete_patchy: true,
            launcher_font_size: 12.0,
            legacy_font_size_revert: None,
            launcher_window_width: 1000,
            launcher_window_height: 600,
            launcher_maximized: false,
            look_and_feel: "auto".to_owned(),
            log_window_width: 720,
            log_window_height: 500,
            log_window_x: 30,
            log_window_y: 30,
            login_form_direction: "top_left".to_owned(),
            hidden_notices_until: HashMap::new(),
            servers: Vec::new(),
            servers_cache_root: String::new(),
            background_path: String::new(),
            legacy_theme_path: String::new(),
            game_directory: default_game_directory().to_string_lossy().into_owned(),
            legacy_profiles_path: String::new(),
            legacy_config_path: default_legacy_config_path().to_string_lossy().into_owned(),
            min_memory: "512M".to_owned(),
            max_memory: "2G".to_owned(),
            automatic_memory: true,
            java_path: String::new(),
            java_mode: "recommended".to_owned(),
            java_runtime_directory: String::new(),
            optimized_java_args: "default".to_owned(),
            use_current_trust_store: true,
            verify_tls_certificates: true,
            wrapper_command: String::new(),
            legacy_experiments: HashSet::new(),
            screen_width: 925,
            screen_height: 530,
            fullscreen: false,
            gpu_preference: "discrete".to_owned(),
            windows_gpu_performance: true,
            jvm_args: String::new(),
            game_args: String::new(),
            loader_type: "none".to_owned(),
            loader_build: "latest".to_owned(),
            separate_game_directories: SeparateDirs::None,
            launch_action: "hide".to_owned(),
            version_only_installed: false,
            version_include_remote: true,
            version_show_old_releases: true,
            version_catalog_types: default_version_catalog_types(),
            version_alert_types: default_version_alert_types(),
        }
    }
}

#[derive(Default)]
struct CommandLineOverrides {
    game_directory: Option<String>,
    java_path: Option<String>,
    selected_version: Option<String>,
    jvm_args: Option<String>,
    game_args: Option<String>,
    window_size: Option<(u32, u32)>,
    background_path: Option<String>,
    fullscreen: Option<bool>,
    theme_path: Option<String>,
    java_runtime_directory: Option<String>,
    username: Option<String>,
    user_type: Option<String>,
    settings_path: Option<String>,
    debug_mode: bool,
    launch_automatically: bool,
}

impl CommandLineOverrides {
    fn apply(&self, settings: &mut SavedSettings) {
        if let Some(value) = &self.game_directory {
            settings.game_directory.clone_from(value);
        }
        if let Some(value) = &self.java_path {
            settings.java_path.clone_from(value);
            settings.java_mode = "custom".to_owned();
        }
        if let Some(value) = &self.selected_version {
            settings.selected_version.clone_from(value);
        }
        if let Some(value) = &self.jvm_args {
            settings.jvm_args.clone_from(value);
        }
        if let Some(value) = &self.game_args {
            settings.game_args.clone_from(value);
        }
        if let Some((width, height)) = self.window_size {
            settings.screen_width = width;
            settings.screen_height = height;
        }
        if let Some(value) = &self.background_path {
            settings.background_path.clone_from(value);
        }
        if let Some(value) = self.fullscreen {
            settings.fullscreen = value;
        }
        if let Some(value) = &self.theme_path {
            settings.legacy_theme_path.clone_from(value);
        }
        if let Some(value) = &self.java_runtime_directory {
            settings.java_runtime_directory.clone_from(value);
        }
        if let Some(username) = &self.username {
            select_command_line_account(settings, username, self.user_type.as_deref());
        }
        if self.launch_automatically {
            settings.auto_login = true;
        }
    }

    fn restore_persisted_values(&self, settings: &mut SavedSettings, persisted: &SavedSettings) {
        if self.game_directory.is_some() {
            settings
                .game_directory
                .clone_from(&persisted.game_directory);
        }
        if self.java_path.is_some() {
            settings.java_path.clone_from(&persisted.java_path);
            settings.java_mode.clone_from(&persisted.java_mode);
        }
        if self.selected_version.is_some() {
            settings
                .selected_version
                .clone_from(&persisted.selected_version);
        }
        if self.jvm_args.is_some() {
            settings.jvm_args.clone_from(&persisted.jvm_args);
        }
        if self.game_args.is_some() {
            settings.game_args.clone_from(&persisted.game_args);
        }
        if self.window_size.is_some() {
            settings.screen_width = persisted.screen_width;
            settings.screen_height = persisted.screen_height;
        }
        if self.background_path.is_some() {
            settings
                .background_path
                .clone_from(&persisted.background_path);
        }
        if self.fullscreen.is_some() {
            settings.fullscreen = persisted.fullscreen;
        }
        if self.theme_path.is_some() {
            settings
                .legacy_theme_path
                .clone_from(&persisted.legacy_theme_path);
        }
        if self.java_runtime_directory.is_some() {
            settings
                .java_runtime_directory
                .clone_from(&persisted.java_runtime_directory);
        }
        if self.launch_automatically {
            settings.auto_login = persisted.auto_login;
        }
    }
}

#[derive(Clone)]
enum ExternalSettingsFile {
    LegacyProperties(PathBuf),
    RustJson(PathBuf),
}

struct Assets {
    background: TextureHandle,
    background_light: TextureHandle,
    background_dark: TextureHandle,
    support: TextureHandle,
    folder: TextureHandle,
    refresh: TextureHandle,
    settings: TextureHandle,
}

#[derive(Default)]
struct LegacyThemePalette {
    foreground: Option<Color32>,
    semi_foreground: Option<Color32>,
    background: Option<Color32>,
    dark_theme_enabled: Option<bool>,
    panel_background: Option<Color32>,
    success: Option<Color32>,
    failure: Option<Color32>,
    border: Option<Color32>,
    border_main: Option<Color32>,
    border_additional: Option<Color32>,
    border_settings: Option<Color32>,
    border_size: Option<f32>,
    shadow: Option<Color32>,
    shadow_uses_border: bool,
    shadow_main: Option<Color32>,
    shadow_additional: Option<Color32>,
    shadow_settings: Option<Color32>,
    arc: Option<u8>,
    arc_main: Option<u8>,
    arc_additional: Option<u8>,
    arc_settings: Option<u8>,
    icon_default_color: Option<Color32>,
    icon_colors: HashMap<String, Color32>,
}

impl LegacyThemePalette {
    fn shadow_for(
        &self,
        specific: Option<Color32>,
        border_color: Option<Color32>,
    ) -> Option<Color32> {
        specific.or_else(|| {
            if self.shadow_uses_border {
                border_color
            } else {
                self.shadow
            }
        })
    }

    fn icon_color(&self, icon_name: &str) -> Option<Color32> {
        self.icon_colors
            .get(icon_name)
            .copied()
            .or(self.icon_default_color)
    }
}

#[derive(Default)]
struct LauncherApp {
    settings: SavedSettings,
    assets: Option<Assets>,
    background_theme_dark: Option<bool>,
    background_custom_loaded: bool,
    command_line_overrides: CommandLineOverrides,
    persisted_settings: SavedSettings,
    external_settings_file: Option<ExternalSettingsFile>,
    extra_version_repositories: Vec<String>,
    version_manifest: Option<MojangVersionManifest>,
    version_catalog_events: Option<Receiver<Result<MojangVersionManifest, String>>>,
    version_catalog_loading: bool,
    version_update_alert: Option<String>,
    version_alert_has_refreshed: bool,
    version_search: String,
    version_kind_filter: String,
    confirm_remove_version: Option<String>,
    auto_launch_pending: bool,
    auto_launch_deadline: Option<Instant>,
    launcher_window_state_seen: bool,
    show_account_dialog: bool,
    show_account_help: bool,
    require_minecraft_account: bool,
    show_require_minecraft_account_notice: bool,
    new_account: String,
    new_account_allow_invalid: bool,
    new_account_ely_skins: bool,
    editing_account: Option<String>,
    show_ely_legacy_dialog: bool,
    ely_legacy_username: String,
    ely_legacy_password: String,
    ely_legacy_signin_events:
        Option<Receiver<Result<(ely_legacy::Profile, ely_legacy::Credentials), String>>>,
    ely_legacy_signin_pending: bool,
    show_ely_dialog: bool,
    ely_signin_events: Option<Receiver<Result<(ely::Profile, ely::Credentials), String>>>,
    ely_signin_pending: bool,
    show_folder_menu: bool,
    folder_menu_pos: Pos2,
    show_support_menu: bool,
    support_menu_pos: Pos2,
    show_support_window: bool,
    show_first_run_notice: bool,
    show_first_run_folder_notice: bool,
    first_run_folder_current_directory: PathBuf,
    first_run_folder_path: String,
    first_run_folder_error: Option<String>,
    show_arm_compat_notice: bool,
    arm_compat_notice_started_at: Option<Instant>,
    show_diagnostic_consent: bool,
    diagnostic_send_enabled: bool,
    diagnostic_events: Option<Receiver<Result<String, String>>>,
    diagnostic_pending: bool,
    diagnostic_payload: Option<String>,
    diagnostic_result_url: Option<String>,
    diagnostic_error: Option<String>,
    show_log_window: bool,
    show_live_launcher_log: bool,
    show_live_game_log: bool,
    selected_log_path: Option<PathBuf>,
    show_settings_menu: bool,
    settings_menu_pos: Pos2,
    legacy_theme_palette_path: String,
    legacy_theme_palette: Option<LegacyThemePalette>,
    update_check_events: Option<Receiver<Result<github_update::CheckResult, String>>>,
    update_checking: bool,
    available_release: Option<github_update::Release>,
    update_prompt_pending: bool,
    show_update_dialog: bool,
    update_install_events: Option<Receiver<Result<(), String>>>,
    update_installing: bool,
    update_install_error: Option<String>,
    show_notices_window: bool,
    show_notice_scene: bool,
    notice_scene_index: usize,
    connectivity_events: Option<Receiver<connectivity::Report>>,
    connectivity_report: Option<connectivity::Report>,
    show_connectivity_window: bool,
    pending_promoted_server: Option<PendingPromotedServerLaunch>,
    new_server_name: String,
    new_server_address: String,
    bootstrap_events: Option<Receiver<Result<bootstrap::Configuration, String>>>,
    bootstrap_loading: bool,
    notices_disable_allowed: bool,
    allow_ely_everywhere: bool,
    bootstrap_error: Option<String>,
    bootstrap_notice_list: Vec<notices::Notice>,
    personal_notice_list: Vec<notices::Notice>,
    bootstrap_promoted_servers: Vec<notices::NoticeAction>,
    bootstrap_notification: Option<bootstrap::Notification>,
    bootstrap_notification_texture: Option<TextureHandle>,
    bootstrap_feedback_url: Option<String>,
    show_feedback_dialog: bool,
    notice_events: Option<Receiver<Result<Vec<notices::Notice>, String>>>,
    notices_loading: bool,
    notice_error: Option<String>,
    notice_list: Vec<notices::Notice>,
    notice_images: HashMap<String, TextureHandle>,
    notice_index: usize,
    server_list: Vec<SavedServer>,
    server_list_root: Option<PathBuf>,
    settings_page: Option<&'static str>,
    status: String,
    launcher_log_initialized: bool,
    launching: bool,
    game_running: bool,
    launch_progress: Option<f32>,
    launch_events: Option<Receiver<LaunchUiEvent>>,
    microsoft_signin_events: Option<Receiver<Result<auth::SignInOutcome, String>>>,
    microsoft_signin_pending: bool,
    profile_creation_events: Option<Receiver<Result<auth::AuthenticatedMicrosoftAccount, String>>>,
    profile_creation_pending: bool,
    pending_minecraft_profile: Option<auth::PendingMinecraftProfile>,
    show_minecraft_profile_dialog: bool,
    new_minecraft_profile_name: String,
    launcher_log: Arc<Mutex<VecDeque<LogRecord>>>,
    game_log: Arc<Mutex<VecDeque<String>>>,
    pending_crash_report: Arc<Mutex<Option<CrashReport>>>,
    crash_report: Option<CrashReport>,
    malware_scan_confirmation: Option<tokio::sync::oneshot::Sender<bool>>,
    malware_detection: Option<malware::MalwareDetection>,
    cancel_requested: Arc<AtomicBool>,
    hidden_for_game: bool,
}

impl LauncherApp {
    fn new(
        cc: &eframe::CreationContext<'_>,
        require_minecraft_account: bool,
        command_line_overrides: CommandLineOverrides,
    ) -> Self {
        let background_light = load_texture(
            &cc.egui_ctx,
            "background-light",
            include_bytes!("../assets/plains.jpg"),
            false,
        );
        let background_dark = load_texture(
            &cc.egui_ctx,
            "background-dark",
            include_bytes!("../assets/dark/plains.jpg"),
            false,
        );
        let mut assets = Assets {
            background: background_light.clone(),
            background_light,
            background_dark,
            support: load_texture(
                &cc.egui_ctx,
                "support",
                include_bytes!("../assets/icons/comments-o.png"),
                true,
            ),
            folder: load_texture(
                &cc.egui_ctx,
                "folder",
                include_bytes!("../assets/icons/folder-open.png"),
                true,
            ),
            refresh: load_texture(
                &cc.egui_ctx,
                "refresh",
                include_bytes!("../assets/icons/refresh.png"),
                true,
            ),
            settings: load_texture(
                &cc.egui_ctx,
                "settings",
                include_bytes!("../assets/icons/bars.png"),
                true,
            ),
        };
        let mut settings = load_settings();
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
            version_catalog_events: Some(spawn_catalog_fetch(Vec::new())),
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

    fn persist(&self) {
        let path = settings_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let mut saved_settings = self.settings.clone();
        self.command_line_overrides
            .restore_persisted_values(&mut saved_settings, &self.persisted_settings);
        http::set_certificate_validation(saved_settings.verify_tls_certificates);
        if let Ok(bytes) = serde_json::to_vec_pretty(&saved_settings) {
            let _ = fs::write(path, bytes);
        }
        if let Some(external_settings_file) = &self.external_settings_file
            && let Err(error) =
                write_external_settings_file(external_settings_file, &saved_settings)
        {
            record_launcher_log_event(
                &self.launcher_log,
                LogLevel::Error,
                LogTarget::Settings,
                &error,
            );
        }
    }

    fn set_logged_status(
        &mut self,
        level: LogLevel,
        target: LogTarget,
        message: impl Into<String>,
    ) {
        let message = message.into();
        record_launcher_log_event(&self.launcher_log, level, target, &message);
        self.status = message;
    }

    fn game_dir_path(&self) -> PathBuf {
        let configured = PathBuf::from(self.settings.game_directory.trim());
        if configured.is_absolute() {
            configured
        } else {
            default_game_directory()
        }
    }

    fn installation_roots(&self) -> Vec<PathBuf> {
        let base = self.game_dir_path();
        let mut roots = vec![base.clone()];
        if let Ok(entries) = fs::read_dir(base.join("home")) {
            roots.extend(entries.flatten().filter_map(|entry| {
                entry.file_type().ok().filter(|kind| kind.is_dir())?;
                Some(entry.path())
            }));
        }
        // Retain installations created by earlier Rust builds that used `instances/`.
        if let Ok(entries) = fs::read_dir(base.join("instances")) {
            roots.extend(entries.flatten().filter_map(|entry| {
                entry.file_type().ok().filter(|kind| kind.is_dir())?;
                Some(entry.path())
            }));
        }
        roots
    }

    fn installed_version_root(&self, version: &str) -> Option<PathBuf> {
        if !valid_version_id(version) {
            return None;
        }
        self.installation_roots().into_iter().find(|root| {
            root.join("versions")
                .join(version)
                .join(format!("{version}.json"))
                .is_file()
        })
    }

    fn game_root_for_version(&self, version: &str) -> PathBuf {
        self.installed_version_root(version).unwrap_or_else(|| {
            let base = self.game_dir_path();
            if !valid_version_id(version) {
                return base;
            }
            match self.settings.separate_game_directories {
                SeparateDirs::None => base,
                SeparateDirs::Version => base.join("home").join(version),
                SeparateDirs::Family => {
                    let family = self.version_family(version);
                    base.join("home").join(family)
                }
            }
        })
    }

    fn game_data_directory_for_version(&self, version: &str) -> PathBuf {
        let root = self.game_root_for_version(version);
        instance_for_version(version, &self.settings)
            .map(|instance| root.join("instances").join(instance))
            .unwrap_or(root)
    }

    fn version_family(&self, version: &str) -> String {
        for root in self.installation_roots() {
            let path = root
                .join("versions")
                .join(version)
                .join(format!("{version}.json"));
            if let Ok(bytes) = fs::read(path) {
                if let Ok(metadata) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                    if let Some(family) = metadata.get("family").and_then(|value| value.as_str()) {
                        if !family.trim().is_empty() {
                            return family.to_owned();
                        }
                    }
                }
            }
        }

        if let Some(kind) = self.version_manifest.as_ref().and_then(|manifest| {
            manifest
                .versions
                .iter()
                .find(|entry| entry.id == version)
                .map(|entry| entry.version_type.as_str())
        }) {
            if matches!(kind, "snapshot" | "old_beta" | "old_alpha") {
                return kind.to_owned();
            }
        }

        let chars: Vec<char> = version.chars().collect();
        let start = chars
            .iter()
            .position(|character| character.is_ascii_digit())
            .unwrap_or(0);
        let mut end = start;
        let mut dots = 0;
        for character in chars.iter().skip(start) {
            if character.is_ascii_digit() || (*character == '.' && dots < 1) {
                if *character == '.' {
                    dots += 1;
                }
                end += 1;
            } else {
                break;
            }
        }
        let family = chars[start..end].iter().collect::<String>();
        if family.matches('.').count() >= 1 {
            let loader_prefix = match self.settings.loader_type.as_str() {
                "forge" | "neoforge" => "Forge-",
                "fabric" | "legacyfabric" => "Fabric-",
                _ => "",
            };
            format!("{loader_prefix}{family}")
        } else {
            "unknown".to_owned()
        }
    }

    fn promoted_version_family(&self, version: &str) -> Option<String> {
        for root in self.installation_roots() {
            let path = root
                .join("versions")
                .join(version)
                .join(format!("{version}.json"));
            if let Ok(bytes) = fs::read(path) {
                if let Ok(metadata) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                    if let Some(family) = metadata
                        .get("family")
                        .and_then(serde_json::Value::as_str)
                        .map(str::trim)
                        .filter(|family| !family.is_empty())
                    {
                        return Some(family.to_owned());
                    }
                }
            }
        }
        legacy_promoted_version_family(version)
    }

    fn promoted_family_versions(&self, families: &[String]) -> Vec<String> {
        let mut result = Vec::new();
        for family in families
            .iter()
            .map(|family| family.trim())
            .filter(|family| !family.is_empty())
        {
            let mut candidates: HashMap<String, String> = HashMap::new();
            if let Some(manifest) = &self.version_manifest {
                for entry in manifest
                    .versions
                    .iter()
                    .filter(|entry| !entry.version_type.eq_ignore_ascii_case("snapshot"))
                {
                    let matches = entry.id == family
                        || self
                            .promoted_version_family(&entry.id)
                            .is_some_and(|candidate_family| candidate_family == family);
                    if matches {
                        candidates.insert(entry.id.clone(), entry.release_time.clone());
                    }
                }
            }
            for installed in self.installed_versions() {
                if let Some(metadata) = self.installed_version_metadata(&installed) {
                    if metadata
                        .get("type")
                        .or_else(|| metadata.get("releaseType"))
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|kind| kind.eq_ignore_ascii_case("snapshot"))
                    {
                        continue;
                    }
                    let candidate_family = metadata
                        .get("family")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                        .or_else(|| legacy_promoted_version_family(&installed));
                    if installed == family || candidate_family.as_deref() == Some(family) {
                        let release_time = metadata
                            .get("releaseTime")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .to_owned();
                        candidates.insert(installed, release_time);
                    }
                }
            }
            let mut family_candidates = candidates.into_iter().collect::<Vec<_>>();
            family_candidates.sort_by(|left, right| right.1.cmp(&left.1));
            result.extend(
                family_candidates
                    .into_iter()
                    .take(2)
                    .map(|(version, _)| version),
            );
        }
        let mut seen = HashSet::new();
        result.retain(|version| seen.insert(version.clone()));
        result
    }

    fn promoted_servers_for_version(&self, version: &str) -> Vec<SavedServer> {
        let family = self.promoted_version_family(version);
        let mut servers = Vec::new();
        for notice in &self.notice_list {
            if !notice.promoted {
                continue;
            }
            let Some(notices::NoticeAction::Server {
                label,
                address,
                families,
                ..
            }) = &notice.action
            else {
                continue;
            };
            append_promoted_server(
                &mut servers,
                version,
                family.as_deref(),
                label,
                address,
                families,
            );
        }
        for action in &self.bootstrap_promoted_servers {
            let notices::NoticeAction::Server {
                label,
                address,
                families,
                ..
            } = action
            else {
                continue;
            };
            append_promoted_server(
                &mut servers,
                version,
                family.as_deref(),
                label,
                address,
                families,
            );
        }
        servers
    }

    fn installed_version_metadata(&self, version: &str) -> Option<serde_json::Value> {
        self.installation_roots().into_iter().find_map(|root| {
            let path = root
                .join("versions")
                .join(version)
                .join(format!("{version}.json"));
            fs::read(path)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        })
    }

    fn installed_versions(&self) -> Vec<String> {
        let mut versions = Vec::new();
        for root in self.installation_roots() {
            if let Ok(entries) = fs::read_dir(root.join("versions")) {
                for entry in entries.flatten() {
                    if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        if entry.path().join(format!("{name}.json")).is_file() {
                            versions.push(name);
                        }
                    }
                }
            }
        }
        versions.sort();
        versions.dedup();
        versions
    }

    fn available_versions(&self) -> Vec<String> {
        let mut versions: Vec<String> = self
            .version_manifest
            .as_ref()
            .map(|manifest| {
                manifest
                    .versions
                    .iter()
                    .map(|entry| entry.id.clone())
                    .collect()
            })
            .unwrap_or_else(|| {
                FALLBACK_VERSIONS
                    .iter()
                    .map(|value| (*value).to_owned())
                    .collect()
            });
        versions.extend(self.installed_versions());
        let mut seen = HashSet::new();
        versions.retain(|version| seen.insert(version.clone()));
        versions
    }

    fn refresh_version_catalog(&mut self) {
        if self.version_catalog_loading {
            return;
        }
        self.start_version_catalog_fetch();
    }

    fn start_version_catalog_fetch(&mut self) {
        self.version_catalog_events =
            Some(spawn_catalog_fetch(self.extra_version_repositories.clone()));
        self.version_catalog_loading = true;
        self.set_logged_status(
            LogLevel::Info,
            LogTarget::VersionCatalog,
            tr("rust.status.catalog.refreshing"),
        );
    }

    fn start_update_check(&mut self) {
        if self.update_checking {
            return;
        }
        self.update_check_events = Some(spawn_update_check(self.settings.beta_updates));
        self.update_checking = true;
        self.set_logged_status(LogLevel::Info, LogTarget::Update, tr("update.checking"));
    }

    fn start_update_install(&mut self, release: github_update::Release) {
        if self.update_installing {
            return;
        }
        self.update_install_events = Some(spawn_update_install(release));
        self.update_installing = true;
        self.update_install_error = None;
        record_launcher_log_event(
            &self.launcher_log,
            LogLevel::Info,
            LogTarget::Update,
            &tr("rust.log.update.download"),
        );
    }

    fn refresh_notices(&mut self) {
        if self.notices_loading {
            return;
        }
        if !self.settings.notices_enabled {
            self.personal_notice_list.clear();
            self.notice_list.clone_from(&self.bootstrap_notice_list);
            self.notice_error = None;
            self.set_logged_status(
                LogLevel::Info,
                LogTarget::Notices,
                tr("rust.status.notices.disabled"),
            );
            return;
        }
        self.notice_events = Some(spawn_notice_fetch(self.settings.client_id.clone()));
        self.notices_loading = true;
        self.notice_error = None;
        record_launcher_log_event(
            &self.launcher_log,
            LogLevel::Info,
            LogTarget::Notices,
            &tr("rust.log.notices.refresh"),
        );
    }

    fn rebuild_notice_list(&mut self, ctx: &Context) {
        self.notice_list.clone_from(&self.bootstrap_notice_list);
        self.notice_list
            .extend(self.personal_notice_list.iter().cloned());
        self.notice_list
            .sort_by_key(|notice| (notice.order, notice.id.clone()));
        self.notice_images.clear();
        for notice in &self.notice_list {
            let Some(bytes) = &notice.image else {
                continue;
            };
            if let Some(texture) =
                load_optional_texture(ctx, &format!("notice_image_{}", notice.id), bytes)
            {
                self.notice_images.insert(notice.id.clone(), texture);
            }
        }
    }

    fn diagnostic_report(&self) -> String {
        let mut report = format!(
            "Legacy Launcher Rust diagnostic report\nLauncher version: {}\nPlatform: {} {}\nMinecraft version: {}\nMod loader: {} {}\n\n{}\n\nRecent game output:\n",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            self.settings.selected_version,
            self.settings.loader_type,
            self.settings.loader_build,
            diagnostics::system_info(),
        );
        let lines = self
            .game_log
            .lock()
            .map(|lines| lines.iter().rev().take(1200).cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        for line in lines.into_iter().rev() {
            let lower = line.to_ascii_lowercase();
            if [
                "token",
                "secret",
                "authorization",
                "bearer ",
                "xsts",
                "xuid",
            ]
            .iter()
            .any(|sensitive| lower.contains(sensitive))
            {
                report.push_str("[sensitive log line omitted]\n");
            } else {
                report.push_str(&line);
                report.push('\n');
            }
            if report.len() >= 700_000 {
                break;
            }
        }
        report.push_str("\nRecent launcher output:\n");
        let launcher_lines = self
            .launcher_log
            .lock()
            .map(|lines| {
                lines
                    .iter()
                    .rev()
                    .take(1200)
                    .map(LogRecord::render)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for line in launcher_lines.into_iter().rev() {
            let lower = line.to_ascii_lowercase();
            if [
                "token",
                "secret",
                "authorization",
                "bearer ",
                "xsts",
                "xuid",
            ]
            .iter()
            .any(|sensitive| lower.contains(sensitive))
            {
                report.push_str("[sensitive log line omitted]\n");
            } else {
                report.push_str(&redact_sensitive_log_values(&line));
                report.push('\n');
            }
            if report.len() >= 700_000 {
                break;
            }
        }
        report.chars().take(700_000).collect()
    }

    fn start_diagnostic_upload(&mut self) {
        let report = self.diagnostic_report();
        self.diagnostic_payload = Some(report.clone());
        self.diagnostic_events = Some(spawn_diagnostic_upload(report));
        self.diagnostic_pending = true;
        self.diagnostic_result_url = None;
        self.diagnostic_error = None;
        record_launcher_log_event(
            &self.launcher_log,
            LogLevel::Info,
            LogTarget::Diagnostics,
            &tr("rust.log.diagnostics.uploading"),
        );
    }

    fn save_diagnostic_report(&mut self) {
        let Some(report) = self.diagnostic_payload.as_ref() else {
            return;
        };
        let path = settings_path()
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("diagnostic.log");
        let result = path
            .parent()
            .map(fs::create_dir_all)
            .transpose()
            .and_then(|_| fs::write(&path, report));
        match result {
            Ok(()) => {
                self.set_logged_status(
                    LogLevel::Info,
                    LogTarget::Diagnostics,
                    tr("rust.status.diagnostics.saved").replace("%0", &path.display().to_string()),
                );
                self.show_diagnostic_consent = false;
                self.diagnostic_error = None;
            }
            Err(error) => {
                self.diagnostic_error = Some(
                    tr("rust.status.diagnostics.save-error").replace("%0", &error.to_string()),
                );
            }
        }
    }

    fn visible_notice_indices(&self) -> Vec<usize> {
        let now = unix_time_millis();
        self.notice_list
            .iter()
            .enumerate()
            .filter(|(_, notice)| {
                (!notice.promoted || self.settings.promoted_notices_enabled)
                    && self
                        .settings
                        .hidden_notices_until
                        .get(&notice.id)
                        .is_none_or(|expiry| *expiry <= now)
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn hide_notice_for_week(&mut self, id: &str) {
        const WEEK_MS: u64 = 7 * 24 * 60 * 60 * 1000;
        self.settings
            .hidden_notices_until
            .insert(id.to_owned(), unix_time_millis().saturating_add(WEEK_MS));
        self.notice_index = self.notice_index.saturating_add(1);
        self.persist();
    }

    fn toggle_notice_visibility_for_week(&mut self, id: &str) {
        const WEEK_MS: u64 = 7 * 24 * 60 * 60 * 1000;
        let now = unix_time_millis();
        let is_hidden = self
            .settings
            .hidden_notices_until
            .get(id)
            .is_some_and(|expiry| *expiry > now);
        if is_hidden {
            self.settings.hidden_notices_until.remove(id);
        } else {
            self.settings
                .hidden_notices_until
                .insert(id.to_owned(), now.saturating_add(WEEK_MS));
        }
        self.persist();
    }

    fn load_server_list(&mut self, game_root: PathBuf) {
        let path = game_root.join("servers.dat");
        let root_key = game_root.to_string_lossy().into_owned();
        let cache_belongs_to_root = self.settings.servers_cache_root.is_empty()
            || self
                .settings
                .servers_cache_root
                .eq_ignore_ascii_case(&root_key);
        match read_minecraft_server_list(&path) {
            Ok(mut servers) => {
                if !path.exists() && cache_belongs_to_root && !self.settings.servers.is_empty() {
                    servers = self.settings.servers.clone();
                    if let Err(error) = write_minecraft_server_list(&path, &servers) {
                        self.set_logged_status(
                            LogLevel::Error,
                            LogTarget::Servers,
                            tr("rust.status.servers.migrate-error")
                                .replace("%0", &error.to_string()),
                        );
                    }
                }
                self.server_list = servers;
                self.settings.servers = self.server_list.clone();
                self.settings.servers_cache_root = root_key;
                self.persist();
            }
            Err(error) => {
                self.server_list = self.settings.servers.clone();
                self.set_logged_status(
                    LogLevel::Error,
                    LogTarget::Servers,
                    tr("rust.status.servers.read-error").replace("%0", &error.to_string()),
                );
            }
        }
        self.server_list_root = Some(game_root);
    }

    fn persist_server_list(&mut self) {
        let Some(root) = self.server_list_root.clone() else {
            return;
        };
        match write_minecraft_server_list(&root.join("servers.dat"), &self.server_list) {
            Ok(()) => {
                self.settings.servers = self.server_list.clone();
                self.settings.servers_cache_root = root.to_string_lossy().into_owned();
                self.persist();
                self.set_logged_status(
                    LogLevel::Info,
                    LogTarget::Servers,
                    tr("rust.status.servers.saved"),
                );
            }
            Err(error) => {
                self.set_logged_status(
                    LogLevel::Error,
                    LogTarget::Servers,
                    tr("rust.status.servers.save-error").replace("%0", &error.to_string()),
                );
            }
        }
    }

    fn perform_notice_action(&mut self, action: notices::NoticeAction, _ctx: &Context) {
        match action {
            notices::NoticeAction::Url { url, .. } => open_url(&url),
            notices::NoticeAction::Server {
                label,
                address,
                accounts,
                families,
            } => {
                self.show_notice_scene = false;
                let name = label
                    .strip_prefix("Join ")
                    .unwrap_or(tr("rust.ui.servers.promoted"))
                    .to_owned();
                if let Some(allowed_accounts) = accounts {
                    let selected_type = self.settings.selected_account.as_ref().map(|selected| {
                        if self.settings.accounts.iter().any(|name| name == selected) {
                            "plain"
                        } else if self
                            .settings
                            .microsoft_accounts
                            .iter()
                            .any(|account| &account.id == selected)
                        {
                            "minecraft"
                        } else if self
                            .settings
                            .ely_accounts
                            .iter()
                            .any(|account| &account.id == selected)
                        {
                            "ely"
                        } else if self
                            .settings
                            .ely_legacy_accounts
                            .iter()
                            .any(|account| &account.id == selected)
                        {
                            "ely_legacy"
                        } else {
                            "unknown"
                        }
                    });
                    if selected_type
                        .is_some_and(|kind| !allowed_accounts.iter().any(|allowed| allowed == kind))
                    {
                        self.set_logged_status(
                            LogLevel::Warn,
                            LogTarget::Servers,
                            tr("rust.status.servers.account-required")
                                .replace("%0", &allowed_accounts.join(", ")),
                        );
                        self.settings_page = Some("Manage accounts");
                        return;
                    }
                }
                let server = SavedServer {
                    name,
                    address,
                    ..Default::default()
                };
                let current_version = self.settings.selected_version.clone();
                let family_versions = self
                    .promoted_family_versions(&families)
                    .into_iter()
                    .filter(|version| version != &current_version)
                    .collect();
                self.pending_promoted_server = Some(PendingPromotedServerLaunch {
                    server,
                    current_version,
                    family_versions,
                });
            }
            notices::NoticeAction::Scene { scene, .. } => match scene.as_str() {
                "settings" | "launcher-settings" => {
                    self.show_notice_scene = false;
                    self.settings_page = Some("Launcher & game preferences")
                }
                "versions" | "version-manager" => {
                    self.show_notice_scene = false;
                    self.settings_page = Some("Manage versions");
                }
                "accounts" | "account-manager" => {
                    self.show_notice_scene = false;
                    self.settings_page = Some("Manage accounts");
                }
                "notices" => {
                    self.show_notices_window = false;
                    self.notice_scene_index = 0;
                    self.show_notice_scene = true;
                }
                "main" | "login" => self.show_notice_scene = false,
                _ => self.set_logged_status(
                    LogLevel::Warn,
                    LogTarget::Launcher,
                    tr("rust.status.scene.unavailable").replace("%0", &scene),
                ),
            },
            notices::NoticeAction::Launcher {
                launcher,
                download_url,
                ..
            } => {
                if let Some(url) = download_url {
                    open_url(&url);
                } else {
                    self.set_logged_status(
                        LogLevel::Warn,
                        LogTarget::Update,
                        tr("rust.status.launcher.version-missing").replace("%0", &launcher),
                    );
                }
            }
        }
    }

    fn visible_catalog_entries(&self) -> Vec<(String, String)> {
        let query = self.version_search.trim().to_ascii_lowercase();
        let installed_versions = self.installed_versions();
        let installed: HashSet<String> = installed_versions.iter().cloned().collect();
        let mut entries: Vec<(String, String)> = self
            .version_manifest
            .as_ref()
            .map(|manifest| {
                manifest
                    .versions
                    .iter()
                    .map(|entry| (entry.id.clone(), entry.version_type.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let known: HashSet<String> = entries.iter().map(|(id, _)| id.clone()).collect();
        entries.extend(
            installed_versions
                .into_iter()
                .filter(|version| !known.contains(version))
                .map(|version| {
                    let version_type = self
                        .installed_version_type(&version)
                        .unwrap_or_else(|| "unknown".to_owned());
                    (version, version_type)
                }),
        );
        entries
            .into_iter()
            .filter(|(id, kind)| {
                let category = normalize_catalog_type(kind);
                let enabled = match category.as_str() {
                    "release" | "unknown" | "installed" => true,
                    "snapshot" | "modified" | "old-beta" | "old-alpha" | "launcher" | "pending" => {
                        self.settings
                            .version_catalog_types
                            .get(&category)
                            .copied()
                            .unwrap_or_else(|| default_version_type_enabled(&category))
                    }
                    _ => false,
                };
                let type_matches = match self.version_kind_filter.as_str() {
                    "release" => kind == "release",
                    "snapshot" => kind == "snapshot",
                    "modified" => category == "modified",
                    "old_beta" => category == "old-beta",
                    "old_alpha" => category == "old-alpha",
                    "launcher" => category == "launcher",
                    "pending" => category == "pending",
                    "old" => matches!(category.as_str(), "old-beta" | "old-alpha"),
                    _ => true,
                };
                let only_installed_matches =
                    !self.settings.version_only_installed || installed.contains(id);
                let remote_matches = self.settings.version_include_remote || installed.contains(id);
                let old_release_matches =
                    self.settings.version_show_old_releases || !self.is_old_release(id);
                enabled
                    && type_matches
                    && only_installed_matches
                    && remote_matches
                    && old_release_matches
                    && (query.is_empty() || id.to_ascii_lowercase().contains(&query))
            })
            .collect()
    }

    fn installed_version_type(&self, version: &str) -> Option<String> {
        for root in self.installation_roots() {
            let path = root
                .join("versions")
                .join(version)
                .join(format!("{version}.json"));
            let Ok(bytes) = fs::read(path) else {
                continue;
            };
            let Ok(metadata) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                continue;
            };
            if let Some(kind) = metadata
                .get("type")
                .or_else(|| metadata.get("releaseType"))
                .and_then(serde_json::Value::as_str)
            {
                return Some(kind.to_ascii_lowercase().replace('_', "-"));
            }
        }
        None
    }

    fn is_old_release(&self, version: &str) -> bool {
        let release_time = self
            .version_manifest
            .as_ref()
            .and_then(|manifest| manifest.versions.iter().find(|entry| entry.id == version))
            .map(|entry| entry.release_time.clone())
            .or_else(|| self.installed_version_release_time(version));
        release_time.is_some_and(|time| time.as_str() < "2014-05-14T00:00:00")
    }

    fn installed_version_release_time(&self, version: &str) -> Option<String> {
        for root in self.installation_roots() {
            let path = root
                .join("versions")
                .join(version)
                .join(format!("{version}.json"));
            let Ok(bytes) = fs::read(path) else {
                continue;
            };
            let Ok(metadata) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                continue;
            };
            if let Some(time) = metadata
                .get("releaseTime")
                .or_else(|| metadata.get("time"))
                .and_then(serde_json::Value::as_str)
            {
                return Some(time.to_owned());
            }
        }
        None
    }

    fn version_release_time(&self, version: &str) -> Option<String> {
        self.version_manifest
            .as_ref()
            .and_then(|manifest| manifest.versions.iter().find(|entry| entry.id == version))
            .map(|entry| entry.release_time.clone())
            .or_else(|| self.installed_version_release_time(version))
    }

    fn version_is_installed(&self) -> bool {
        self.installed_version_root(&self.settings.selected_version)
            .is_some()
    }

    fn open_account_dialog(&mut self) {
        if !self.allow_non_minecraft_account() {
            return;
        }
        self.new_account.clear();
        self.new_account_allow_invalid = false;
        self.new_account_ely_skins = true;
        self.editing_account = None;
        self.show_account_dialog = true;
    }

    fn open_edit_account_dialog(&mut self, account: String) {
        self.new_account = account.clone();
        self.new_account_allow_invalid = false;
        self.new_account_ely_skins = offline_ely_skins_for(&self.settings, &account);
        self.editing_account = Some(account);
        self.show_account_dialog = true;
    }

    fn start_microsoft_sign_in(&mut self) {
        if self.microsoft_signin_pending || self.launching {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.microsoft_signin_events = Some(receiver);
        self.microsoft_signin_pending = true;
        self.cancel_requested.store(false, Ordering::Release);
        let cancel_requested = self.cancel_requested.clone();
        self.set_logged_status(
            LogLevel::Info,
            LogTarget::Authentication,
            tr("rust.status.account.microsoft-browser"),
        );
        thread::spawn(move || {
            let result = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime.block_on(async {
                    tokio::select! {
                        result = auth::sign_in(cancel_requested.clone()) => result,
                        _ = wait_for_cancel(cancel_requested) => {
                            Err(tr("rust.status.account.microsoft.cancelled").to_owned())
                        }
                    }
                }),
                Err(error) => Err(tr("rust.status.account.microsoft.runtime-error")
                    .replace("%0", &error.to_string())),
            };
            let _ = sender.send(result);
        });
    }

    fn open_ely_legacy_dialog(&mut self) {
        if !self.allow_non_minecraft_account() {
            return;
        }
        self.ely_legacy_username.clear();
        self.ely_legacy_password.clear();
        self.show_ely_legacy_dialog = true;
    }

    fn open_ely_dialog(&mut self) {
        if !self.allow_non_minecraft_account() {
            return;
        }
        self.show_ely_dialog = true;
    }

    fn allow_non_minecraft_account(&mut self) -> bool {
        if !self.require_minecraft_account || has_licensed_minecraft_account(&self.settings) {
            return true;
        }
        self.show_require_minecraft_account_notice = true;
        false
    }

    fn start_ely_legacy_sign_in(&mut self) {
        if self.ely_legacy_signin_pending || self.launching {
            return;
        }
        let username = self.ely_legacy_username.trim().to_owned();
        let password = self.ely_legacy_password.clone();
        if username.is_empty() || password.is_empty() {
            self.set_logged_status(
                LogLevel::Warn,
                LogTarget::Authentication,
                tr("rust.status.account.ely.enter-credentials"),
            );
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.ely_legacy_signin_events = Some(receiver);
        self.ely_legacy_signin_pending = true;
        self.cancel_requested.store(false, Ordering::Release);
        let cancel_requested = self.cancel_requested.clone();
        self.set_logged_status(
            LogLevel::Info,
            LogTarget::Authentication,
            tr("rust.status.account.ely.signing-in"),
        );
        thread::spawn(move || {
            let result = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime.block_on(async {
                    tokio::select! {
                        result = ely_legacy::authenticate(&username, &password) => result,
                        _ = wait_for_cancel(cancel_requested) => Err(tr("rust.status.account.ely.oauth.cancelled").to_owned()),
                    }
                }),
                Err(error) => Err(tr("rust.status.account.ely.runtime-error")
                    .replace("%0", &error.to_string())),
            };
            let _ = sender.send(result);
        });
    }

    fn save_ely_legacy_account(
        &mut self,
        result: Result<(ely_legacy::Profile, ely_legacy::Credentials), String>,
    ) {
        self.ely_legacy_signin_events = None;
        self.ely_legacy_signin_pending = false;
        match result {
            Ok((profile, credentials)) => {
                let account = ElyLegacyAccountProfile {
                    id: format!("ely_legacy:{}", profile.uuid),
                    username: profile.username,
                    display_name: profile.display_name,
                    uuid: profile.uuid,
                };
                let secret = match serde_json::to_string(&credentials) {
                    Ok(secret) => secret,
                    Err(error) => {
                        self.set_logged_status(
                            LogLevel::Error,
                            LogTarget::Authentication,
                            tr("rust.status.account.ely.save-error")
                                .replace("%0", &error.to_string()),
                        );
                        return;
                    }
                };
                if let Err(error) = vault::store_refresh_token(&account.storage_key(), &secret) {
                    self.set_logged_status(
                        LogLevel::Error,
                        LogTarget::Authentication,
                        tr("rust.status.account.ely.protect-error")
                            .replace("%0", &error.to_string()),
                    );
                    return;
                }
                self.settings
                    .ely_legacy_accounts
                    .retain(|saved| saved.uuid != account.uuid);
                self.settings.ely_legacy_accounts.push(account.clone());
                self.settings.selected_account = Some(account.id);
                self.show_ely_legacy_dialog = false;
                self.ely_legacy_password.clear();
                self.set_logged_status(
                    LogLevel::Info,
                    LogTarget::Authentication,
                    tr("rust.status.account.ely.signed-in").replace("%0", &account.display_name),
                );
                self.persist();
            }
            Err(error) => self.set_logged_status(LogLevel::Error, LogTarget::Authentication, error),
        }
    }

    fn start_ely_sign_in(&mut self) {
        if self.ely_signin_pending || self.launching {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.ely_signin_events = Some(receiver);
        self.ely_signin_pending = true;
        self.cancel_requested.store(false, Ordering::Release);
        let cancel_requested = self.cancel_requested.clone();
        self.set_logged_status(
            LogLevel::Info,
            LogTarget::Authentication,
            tr("rust.status.account.ely.browser"),
        );
        thread::spawn(move || {
            let result =
                match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime.block_on(ely::sign_in(cancel_requested.clone())),
                    Err(error) => Err(tr("rust.status.account.ely.runtime-error")
                        .replace("%0", &error.to_string())),
                };
            let _ = sender.send(result);
        });
    }

    fn save_ely_account(&mut self, result: Result<(ely::Profile, ely::Credentials), String>) {
        self.ely_signin_events = None;
        self.ely_signin_pending = false;
        match result {
            Ok((profile, credentials)) => {
                let account = ElyAccountProfile {
                    id: format!("ely:{}", profile.uuid),
                    account_id: profile.id,
                    username: profile.username,
                    display_name: profile.display_name,
                    uuid: profile.uuid,
                };
                let secret = match serde_json::to_string(&credentials) {
                    Ok(secret) => secret,
                    Err(error) => {
                        self.set_logged_status(
                            LogLevel::Error,
                            LogTarget::Authentication,
                            tr("rust.status.account.ely.save-error")
                                .replace("%0", &error.to_string()),
                        );
                        return;
                    }
                };
                if let Err(error) = vault::store_refresh_token(&account.storage_key(), &secret) {
                    self.set_logged_status(
                        LogLevel::Error,
                        LogTarget::Authentication,
                        tr("rust.status.account.ely.protect-error")
                            .replace("%0", &error.to_string()),
                    );
                    return;
                }
                self.settings
                    .ely_accounts
                    .retain(|saved| saved.uuid != account.uuid);
                self.settings.ely_accounts.push(account.clone());
                self.settings.selected_account = Some(account.id);
                self.show_ely_dialog = false;
                self.set_logged_status(
                    LogLevel::Info,
                    LogTarget::Authentication,
                    tr("rust.status.account.ely.signed-in").replace("%0", &account.display_name),
                );
                self.persist();
            }
            Err(error) => self.set_logged_status(LogLevel::Error, LogTarget::Authentication, error),
        }
    }

    fn apply_microsoft_sign_in(&mut self, result: Result<auth::SignInOutcome, String>) {
        self.microsoft_signin_pending = false;
        self.microsoft_signin_events = None;
        match result {
            Ok(auth::SignInOutcome::Authenticated(account)) => {
                self.save_microsoft_account(account);
            }
            Ok(auth::SignInOutcome::ProfileCreationRequired(pending)) => {
                self.pending_minecraft_profile = Some(pending);
                self.new_minecraft_profile_name.clear();
                self.show_minecraft_profile_dialog = true;
                self.set_logged_status(
                    LogLevel::Info,
                    LogTarget::Authentication,
                    tr("rust.status.account.microsoft.profile.choose"),
                );
            }
            Err(error) => self.set_logged_status(LogLevel::Error, LogTarget::Authentication, error),
        }
    }

    fn save_microsoft_account(&mut self, account: auth::AuthenticatedMicrosoftAccount) {
        let profile = MicrosoftAccountProfile {
            id: format!("microsoft:{}", account.uuid),
            username: account.username.clone(),
            uuid: account.uuid,
            xuid: account.xuid,
        };
        if let Err(error) =
            vault::store_refresh_token(&profile.storage_key(), &account.refresh_token)
        {
            self.set_logged_status(
                LogLevel::Error,
                LogTarget::Authentication,
                tr("rust.status.account.microsoft.save-error").replace("%0", &error.to_string()),
            );
            return;
        }
        self.settings
            .microsoft_accounts
            .retain(|saved| saved.id != profile.id);
        self.settings.microsoft_accounts.push(profile.clone());
        self.settings.selected_account = Some(profile.id);
        self.set_logged_status(
            LogLevel::Info,
            LogTarget::Authentication,
            tr("rust.status.account.microsoft.signed-in").replace("%0", &account.username),
        );
        self.pending_minecraft_profile = None;
        self.show_minecraft_profile_dialog = false;
        self.persist();
    }

    fn start_minecraft_profile_creation(&mut self) {
        if self.profile_creation_pending {
            return;
        }
        let Some(pending) = self.pending_minecraft_profile.clone() else {
            self.show_minecraft_profile_dialog = false;
            self.set_logged_status(
                LogLevel::Warn,
                LogTarget::Authentication,
                tr("rust.status.account.microsoft.profile.restart"),
            );
            return;
        };
        let name = self.new_minecraft_profile_name.trim().to_owned();
        if !(3..=16).contains(&name.len())
            || !name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            self.set_logged_status(
                LogLevel::Warn,
                LogTarget::Authentication,
                tr("rust.status.account.microsoft.profile.invalid-name"),
            );
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.profile_creation_events = Some(receiver);
        self.profile_creation_pending = true;
        self.set_logged_status(
            LogLevel::Info,
            LogTarget::Authentication,
            tr("rust.status.account.microsoft.profile.creating").replace("%0", &name),
        );
        thread::spawn(move || {
            let result = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime.block_on(auth::create_minecraft_profile(pending, &name)),
                Err(error) => Err(tr("rust.status.account.microsoft.profile-runtime-error")
                    .replace("%0", &error.to_string())),
            };
            let _ = sender.send(result);
        });
    }

    fn selected_account_label(&self) -> String {
        self.settings
            .selected_account
            .as_ref()
            .and_then(|id| {
                self.settings
                    .microsoft_accounts
                    .iter()
                    .find(|account| &account.id == id)
                    .map(|account| account.username.clone())
            })
            .or_else(|| {
                self.settings
                    .ely_accounts
                    .iter()
                    .find(|account| {
                        Some(account.id.as_str()) == self.settings.selected_account.as_deref()
                    })
                    .map(|account| account.display_name.clone())
            })
            .or_else(|| {
                self.settings
                    .ely_legacy_accounts
                    .iter()
                    .find(|account| {
                        Some(account.id.as_str()) == self.settings.selected_account.as_deref()
                    })
                    .map(|account| account.display_name.clone())
            })
            .or_else(|| self.settings.selected_account.clone())
            .unwrap_or_else(|| tr("account.empty").to_owned())
    }

    fn first_account_key(&self) -> Option<String> {
        self.settings
            .accounts
            .first()
            .cloned()
            .or_else(|| {
                self.settings
                    .microsoft_accounts
                    .first()
                    .map(|account| account.id.clone())
            })
            .or_else(|| {
                self.settings
                    .ely_accounts
                    .first()
                    .map(|account| account.id.clone())
            })
            .or_else(|| {
                self.settings
                    .ely_legacy_accounts
                    .first()
                    .map(|account| account.id.clone())
            })
    }

    fn start_selected_game(&mut self) {
        self.start_selected_game_on_server(None);
    }

    fn start_selected_game_on_server(&mut self, server: Option<SavedServer>) {
        if self.launching {
            return;
        }
        if server
            .as_ref()
            .is_some_and(|server| parse_server_address(&server.address).is_none())
        {
            self.set_logged_status(
                LogLevel::Warn,
                LogTarget::Servers,
                tr("rust.status.servers.invalid-address"),
            );
            return;
        }
        let Some(selected_id) = self.settings.selected_account.clone() else {
            self.open_account_dialog();
            return;
        };
        let microsoft_account = self
            .settings
            .microsoft_accounts
            .iter()
            .find(|account| account.id == selected_id)
            .cloned();
        let ely_legacy_account = self
            .settings
            .ely_legacy_accounts
            .iter()
            .find(|account| account.id == selected_id)
            .cloned();
        let ely_account = self
            .settings
            .ely_accounts
            .iter()
            .find(|account| account.id == selected_id)
            .cloned();
        let offline_username = if microsoft_account.is_none()
            && ely_account.is_none()
            && ely_legacy_account.is_none()
            && self
                .settings
                .accounts
                .iter()
                .any(|name| name == &selected_id)
        {
            Some(selected_id)
        } else {
            None
        };
        let offline_uuid = offline_username
            .as_deref()
            .and_then(|username| offline_account_uuid_for(&self.settings, username));
        if microsoft_account.is_none()
            && ely_account.is_none()
            && ely_legacy_account.is_none()
            && offline_username.is_none()
        {
            self.settings.selected_account = None;
            self.open_account_dialog();
            return;
        }

        let library_replace_type = if microsoft_account.is_some() {
            Some("minecraft".to_owned())
        } else if ely_account.is_some() || ely_legacy_account.is_some() {
            Some("ely".to_owned())
        } else {
            offline_username.as_deref().map(|username| {
                if self.allow_ely_everywhere && offline_ely_skins_for(&self.settings, username) {
                    "ely".to_owned()
                } else {
                    "plain".to_owned()
                }
            })
        };
        let mut library_replace_types = Vec::new();
        if self.settings.delete_patchy {
            library_replace_types.push("patchy".to_owned());
        }
        if let Some(replacement_type) = library_replace_type {
            library_replace_types.push(replacement_type);
        }
        if self.settings.legacy_experiments.contains("updated_lwjgl") {
            library_replace_types.push("experiment-lwjgl-update".to_owned());
        }
        if self.settings.legacy_experiments.contains("updated_jna") {
            library_replace_types.push("experiment-jna-update".to_owned());
        }

        let version = self.settings.selected_version.clone();
        if !valid_version_id(&version) {
            self.set_logged_status(
                LogLevel::Warn,
                LogTarget::VersionManager,
                tr("rust.status.versions.invalid-id"),
            );
            return;
        }
        let force_update = self.settings.force_update;
        let game_dir = self.game_root_for_version(&version);
        let promoted_servers = if self.settings.promoted_servers_in_game {
            self.promoted_servers_for_version(&version)
        } else {
            Vec::new()
        };
        let server_save_warning = match prepare_minecraft_server_list(
            &game_dir.join("servers.dat"),
            server.as_ref(),
            &promoted_servers,
            self.settings.promoted_servers_in_game,
        ) {
            Ok(()) => {
                if let Ok(servers) = read_minecraft_server_list(&game_dir.join("servers.dat")) {
                    self.settings.servers = servers.clone();
                    self.settings.servers_cache_root = game_dir.to_string_lossy().into_owned();
                    if self.server_list_root.as_deref() == Some(game_dir.as_path()) {
                        self.server_list = servers;
                    }
                    self.persist();
                }
                None
            }
            Err(error) => {
                Some(tr("rust.status.game.server-save-warning").replace("%0", &error.to_string()))
            }
        };
        let quick_play_supported = server
            .as_ref()
            .map(|_| selected_version_supports_quick_play(&version, &game_dir));
        let quick_play_server = server;
        let mods_directory = game_dir.join("mods");
        let should_scan_malware = self.settings.malware_scan_version < 2 && mods_directory.is_dir();
        if should_scan_malware {
            // Record the scan version first, matching Java's fail-open behavior.
            self.settings.malware_scan_version = 2;
            self.persist();
        }
        let crash_game_dir = self.game_data_directory_for_version(&version);
        let preferences = self.settings.clone();
        let show_full_game_command = preferences.show_full_game_command;
        let crash_detection_enabled = preferences.crash_detection_enabled;
        let crash_version = version.clone();
        let crash_loader = format!("{}-{}", preferences.loader_type, preferences.loader_build);
        let version_manifest = self.version_manifest.clone();
        let extra_version_repositories = self.extra_version_repositories.clone();
        let memory_release_time = self.version_release_time(&version);
        let game_log = self.game_log.clone();
        let launcher_log = self.launcher_log.clone();
        if let Ok(mut lines) = game_log.lock() {
            lines.clear();
        }
        let pending_crash_report = self.pending_crash_report.clone();
        if let Ok(mut report) = pending_crash_report.lock() {
            *report = None;
        }
        let launch_started = SystemTime::now();
        let (update_tx, update_rx) = mpsc::channel();
        self.launch_events = Some(update_rx);
        self.launching = true;
        if self.settings.logger_mode == "global" {
            self.show_log_window = true;
            self.show_live_launcher_log = true;
            self.show_live_game_log = false;
        }
        self.launch_progress = Some(0.0);
        self.cancel_requested.store(false, Ordering::Release);
        let cancel_requested = self.cancel_requested.clone();
        self.set_logged_status(
            LogLevel::Info,
            LogTarget::Minecraft,
            tr("rust.status.game.preparing").replace("%0", &version),
        );

        thread::spawn(move || {
            if let Some(warning) = server_save_warning {
                send_launch_message(&update_tx, LogLevel::Warn, LogTarget::Servers, warning);
            }
            let done_tx = update_tx.clone();
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    send_launch_message(
                        &update_tx,
                        LogLevel::Error,
                        LogTarget::Launcher,
                        tr("rust.status.runtime.start-error").replace("%0", &error.to_string()),
                    );
                    let _ = done_tx.send(LaunchUiEvent::Finished);
                    return;
                }
            };

            runtime.block_on(async move {
                let authenticator = if let Some(account) = microsoft_account {
                    tokio::select! {
                        result = authenticate_microsoft_account(&account) => match result {
                            Ok(authenticator) => authenticator,
                            Err(error) => {
                                send_launch_message(
                                    &update_tx,
                                    LogLevel::Error,
                                    LogTarget::Authentication,
                                    error,
                                );
                                return;
                            }
                        },
                        _ = wait_for_cancel(cancel_requested.clone()) => {
                            send_launch_message(
                                &update_tx,
                                LogLevel::Warn,
                                LogTarget::Authentication,
                                tr("rust.status.account.signin-cancelled"),
                            );
                            return;
                        }
                    }
                } else if let Some(account) = ely_legacy_account {
                    tokio::select! {
                        result = authenticate_ely_legacy_account(&account) => match result {
                            Ok(authenticator) => authenticator,
                            Err(error) => {
                                send_launch_message(
                                    &update_tx,
                                    LogLevel::Error,
                                    LogTarget::Authentication,
                                    error,
                                );
                                return;
                            }
                        },
                        _ = wait_for_cancel(cancel_requested.clone()) => {
                            send_launch_message(
                                &update_tx,
                                LogLevel::Warn,
                                LogTarget::Authentication,
                                tr("rust.status.account.ely.signin-cancelled"),
                            );
                            return;
                        }
                    }
                } else if let Some(account) = ely_account {
                    tokio::select! {
                        result = authenticate_ely_account(&account) => match result {
                            Ok(authenticator) => authenticator,
                            Err(error) => {
                                send_launch_message(
                                    &update_tx,
                                    LogLevel::Error,
                                    LogTarget::Authentication,
                                    error,
                                );
                                return;
                            }
                        },
                        _ = wait_for_cancel(cancel_requested.clone()) => {
                            send_launch_message(
                                &update_tx,
                                LogLevel::Warn,
                                LogTarget::Authentication,
                                tr("rust.status.account.ely.signin-cancelled"),
                            );
                            return;
                        }
                    }
                } else {
                    offline_authenticator_with_uuid(
                        offline_username.unwrap_or_default(),
                        offline_uuid.as_deref(),
                    )
                };
                let mut preferences = preferences;
                let java_mode = preferences.java_mode.clone();
                match java_mode.as_str() {
                    "recommended" => preferences.java_path.clear(),
                    "current" => match tokio::task::spawn_blocking(detect_java).await {
                        Ok(Ok((path, version))) => {
                            preferences.java_path = path.to_string_lossy().into_owned();
                            send_launch_message(
                                &update_tx,
                                LogLevel::Info,
                                LogTarget::JavaRuntime,
                                tr("rust.status.java.current").replace("%0", &version),
                            );
                        }
                        Ok(Err(error)) => {
                            send_launch_message(
                                &update_tx,
                                LogLevel::Error,
                                LogTarget::JavaRuntime,
                                error,
                            );
                            return;
                        }
                        Err(error) => {
                            send_launch_message(
                                &update_tx,
                                LogLevel::Error,
                                LogTarget::JavaRuntime,
                                tr("rust.status.java.detection-error")
                                    .replace("%0", &error.to_string()),
                            );
                            return;
                        }
                    },
                    "custom" if preferences.java_path.trim().is_empty() => {
                        send_launch_message(
                            &update_tx,
                            LogLevel::Warn,
                            LogTarget::JavaRuntime,
                            tr("rust.status.java.custom-required"),
                        );
                        return;
                    }
                    _ => {}
                }
                if java_mode != "recommended" {
                    let java_path = PathBuf::from(preferences.java_path.trim());
                    let check_result = tokio::select! {
                        result = verify_java_compatibility(
                            &java_path,
                            &game_dir,
                            &version,
                            version_manifest.as_ref(),
                            &extra_version_repositories,
                        ) => {
                            Some(result)
                        }
                        _ = wait_for_cancel(cancel_requested.clone()) => None,
                    };
                    let Some(check_result) = check_result else {
                        send_launch_message(
                            &update_tx,
                            LogLevel::Warn,
                            LogTarget::JavaRuntime,
                            tr("rust.status.java.check-cancelled"),
                        );
                        return;
                    };
                    match check_result {
                        Ok(message) => {
                            send_launch_message(
                                &update_tx,
                                LogLevel::Info,
                                LogTarget::JavaRuntime,
                                message,
                            );
                        }
                        Err(error) => {
                            send_launch_message(
                                &update_tx,
                                LogLevel::Error,
                                LogTarget::JavaRuntime,
                                error,
                            );
                            return;
                        }
                    }
                }

                let experimental_java_major = if preferences
                    .legacy_experiments
                    .contains("zgc_generational")
                    && preferences.optimized_java_args == "zgc"
                {
                    selected_java_major_for_experiments(
                        &game_dir,
                        &version,
                        &preferences,
                        version_manifest.as_ref(),
                        &extra_version_repositories,
                    )
                    .await
                } else {
                    None
                };

                let gpu_preference = preferences.gpu_preference.clone();
                let gpu_preference_enabled = preferences.windows_gpu_performance;
                let remove_undesirable_mods = preferences.remove_undesirable_mods;
                let replacement_instance_version = version.clone();
                let legacy_experiments = preferences.legacy_experiments.clone();
                let mut options = make_launch_options_with_authenticator(
                    version,
                    authenticator,
                    game_dir,
                    force_update,
                    preferences,
                    memory_release_time.as_deref(),
                );
                if let Some(warning) = append_legacy_experiment_arguments(
                    &mut options,
                    &legacy_experiments,
                    experimental_java_major,
                ) {
                    send_launch_message(
                        &update_tx,
                        LogLevel::Warn,
                        LogTarget::JavaRuntime,
                        warning,
                    );
                }
                if !library_replace_types.is_empty() {
                    options.library_replace_types = library_replace_types.clone();
                    options.skip_bundle_check = false;
                    let replacement_profile = library_replace_types.join("-");
                    options.instance = Some(match options.instance.take() {
                        Some(instance) => {
                            format!("{instance}-replace-{replacement_profile}-{replacement_instance_version}")
                        }
                        None => format!("legacylauncher-replace-{replacement_profile}-{replacement_instance_version}"),
                    });
                }
                if let (Some(server), Some(supported)) = (quick_play_server, quick_play_supported) {
                    append_server_launch_arguments(&mut options, &server.address, supported);
                    send_launch_message(
                        &update_tx,
                        LogLevel::Info,
                        LogTarget::Minecraft,
                        tr("rust.status.game.connecting").replace("%0", &server.name),
                    );
                }

                if should_scan_malware {
                    match scan_mods_before_launch(
                        &mods_directory,
                        cancel_requested.clone(),
                        &update_tx,
                    )
                    .await
                    {
                        MalwareScanOutcome::Cancelled => {
                            send_launch_message(
                                &update_tx,
                                LogLevel::Warn,
                                LogTarget::Mods,
                                tr("rust.status.game.launch-cancelled"),
                            );
                            return;
                        }
                        MalwareScanOutcome::Infected(detection) => {
                            send_launch_message(
                                &update_tx,
                                LogLevel::Error,
                                LogTarget::Mods,
                                format!(
                                    "Fractureiser signature detected in {}: {}",
                                    detection.jar.display(),
                                    detection.entry
                                ),
                            );
                            let _ = update_tx.send(LaunchUiEvent::MalwareDetected(detection));
                            return;
                        }
                        MalwareScanOutcome::Clean | MalwareScanOutcome::Skipped => {}
                    }
                }

                let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(256);
                let progress_tx = update_tx.clone();
                let listener_game_log = game_log.clone();
                let listener_launcher_log = launcher_log.clone();
                let listener = tokio::spawn(async move {
                    while let Some(event) = event_rx.recv().await {
                        let message = launch_event_message_or_record(
                            &listener_game_log,
                            &listener_launcher_log,
                            event,
                            show_full_game_command,
                        );
                        if let Some(message) = message {
                            let _ = progress_tx.send(message);
                        }
                    }
                });

                let mut launcher = Launcher::new(options);
                let download_result = tokio::select! {
                    result = launcher.download_game(event_tx.clone()) => Some(result),
                    _ = wait_for_cancel(cancel_requested.clone()) => None,
                };
                match download_result {
                    None => {
                        send_launch_message(
                            &update_tx,
                            LogLevel::Warn,
                            LogTarget::Minecraft,
                            tr("rust.status.game.launch-cancelled"),
                        );
                        drop(event_tx);
                        let _ = listener.await;
                        return;
                    }
                    Some(Err(error)) => {
                        send_launch_message(
                            &update_tx,
                            LogLevel::Error,
                            LogTarget::Minecraft,
                            tr("rust.status.game.launch-error").replace("%0", &error.to_string()),
                        );
                        drop(event_tx);
                        let _ = listener.await;
                        return;
                    }
                    Some(Ok(())) => {}
                }
                if gpu_preference_enabled {
                    if let Some(game_data) = launcher.game_data() {
                        if let Err(error) =
                            apply_gpu_preference(&game_data.minecraft_java.path, &gpu_preference)
                        {
                            send_launch_message(
                                &update_tx,
                                LogLevel::Warn,
                                LogTarget::JavaRuntime,
                                tr("rust.status.game.gpu-error").replace("%0", &error.to_string()),
                            );
                        }
                    }
                }
                if cancel_requested.load(Ordering::Acquire) {
                    send_launch_message(
                        &update_tx,
                        LogLevel::Warn,
                        LogTarget::Minecraft,
                        tr("rust.status.game.launch-cancelled"),
                    );
                    drop(event_tx);
                    let _ = listener.await;
                    return;
                }
                if remove_undesirable_mods {
                    match remove_undesirable_mod_files(&mods_directory) {
                        Ok(summary) => {
                            if !summary.removed.is_empty() {
                                send_launch_message(
                                    &update_tx,
                                    LogLevel::Info,
                                    LogTarget::Mods,
                                    tr("rust.status.game.removed-mods")
                                        .replace("%0", &summary.removed.join(", ")),
                                );
                            }
                            for (name, error) in summary.failed {
                                send_launch_message(
                                    &update_tx,
                                    LogLevel::Warn,
                                    LogTarget::Mods,
                                    tr("rust.status.game.remove-mod-error")
                                        .replace("%0", &name)
                                        .replace("%1", &error),
                                );
                            }
                        }
                        Err(error) => {
                            send_launch_message(
                                &update_tx,
                                LogLevel::Warn,
                                LogTarget::Mods,
                                tr("rust.status.game.scan-mods-error")
                                    .replace("%0", &error.to_string()),
                            );
                        }
                    }
                }
                match launcher.launch(event_tx.clone()).await {
                    Ok(mut child) => {
                        let _ = update_tx.send(LaunchUiEvent::GameStarted);
                        let wait_result = tokio::select! {
                            result = child.wait() => Some(result),
                            _ = wait_for_cancel(cancel_requested.clone()) => None,
                        };
                        match wait_result {
                            Some(Ok(status)) => {
                                let code = process_exit_code(&status);
                                if crash_detection_enabled && !status.success() {
                                    let crash_runtime = match launcher.game_data() {
                                        Some(game_data) => {
                                            let java_path = game_data.minecraft_java.path.clone();
                                            tokio::task::spawn_blocking(move || {
                                                crash::collect_runtime_info(&java_path)
                                            })
                                            .await
                                            .unwrap_or_default()
                                        }
                                        None => crash::CrashRuntimeInfo::default(),
                                    };
                                    if let Some(report) = collect_crash_report(
                                        &crash_game_dir,
                                        &game_log,
                                        launch_started,
                                        &crash_version,
                                        &crash_loader,
                                        code,
                                        &crash_runtime,
                                    ) {
                                        if let Ok(mut pending) = pending_crash_report.lock() {
                                            *pending = Some(report);
                                        }
                                    }
                                }
                                let _ = event_tx.send(LaunchEvent::Close(code)).await;
                            }
                            Some(Err(error)) => {
                                send_launch_message(
                                    &update_tx,
                                    LogLevel::Error,
                                    LogTarget::Minecraft,
                                    tr("rust.status.game.monitor-error")
                                        .replace("%0", &error.to_string()),
                                );
                            }
                            None => {
                                let _ = child.kill().await;
                                let _ = child.wait().await;
                                send_launch_message(
                                    &update_tx,
                                    LogLevel::Warn,
                                    LogTarget::Minecraft,
                                    tr("rust.status.game.stopped"),
                                );
                            }
                        }
                    }
                    Err(error) => {
                        send_launch_message(
                            &update_tx,
                            LogLevel::Error,
                            LogTarget::Minecraft,
                            tr("rust.status.game.launch-error").replace("%0", &error.to_string()),
                        );
                    }
                }
                drop(event_tx);
                let _ = listener.await;
            });

            let _ = done_tx.send(LaunchUiEvent::Finished);
        });
    }

    fn start_version_install(&mut self, version: String) {
        if self.launching {
            return;
        }
        self.settings.selected_version = version.clone();
        self.persist();

        if !valid_version_id(&version) {
            self.set_logged_status(
                LogLevel::Warn,
                LogTarget::VersionManager,
                tr("rust.status.versions.invalid-id"),
            );
            return;
        }
        let game_dir = self.game_root_for_version(&version);
        let preferences = self.settings.clone();
        let (update_tx, update_rx) = mpsc::channel();
        self.launch_events = Some(update_rx);
        self.launching = true;
        self.launch_progress = Some(0.0);
        self.cancel_requested.store(false, Ordering::Release);
        let cancel_requested = self.cancel_requested.clone();
        self.set_logged_status(
            LogLevel::Info,
            LogTarget::VersionManager,
            tr("rust.status.game.installing").replace("%0", &version),
        );

        thread::spawn(move || {
            let done_tx = update_tx.clone();
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    send_launch_message(
                        &update_tx,
                        LogLevel::Error,
                        LogTarget::Launcher,
                        tr("rust.status.runtime.start-error").replace("%0", &error.to_string()),
                    );
                    let _ = done_tx.send(LaunchUiEvent::Finished);
                    return;
                }
            };

            runtime.block_on(async move {
                let options = make_launch_options(
                    version.clone(),
                    "VersionManager".to_owned(),
                    game_dir,
                    true,
                    preferences,
                );
                let mut launcher = Launcher::new(options);
                let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(256);
                let progress_tx = update_tx.clone();
                let listener = tokio::spawn(async move {
                    while let Some(event) = event_rx.recv().await {
                        if let Some(message) = launch_event_ui_message(event) {
                            let _ = progress_tx.send(LaunchUiEvent::Message(message));
                        }
                    }
                });

                let install_result = tokio::select! {
                    result = launcher.download_game(event_tx.clone()) => Some(result),
                    _ = wait_for_cancel(cancel_requested) => None,
                };
                match install_result {
                    Some(Ok(())) => {
                        send_launch_message(
                            &update_tx,
                            LogLevel::Info,
                            LogTarget::VersionManager,
                            tr("rust.status.versions.installed").replace("%0", &version),
                        );
                    }
                    Some(Err(error)) => {
                        send_launch_message(
                            &update_tx,
                            LogLevel::Error,
                            LogTarget::VersionManager,
                            tr("rust.status.versions.install-failed")
                                .replace("%0", &error.to_string()),
                        );
                    }
                    None => {
                        send_launch_message(
                            &update_tx,
                            LogLevel::Warn,
                            LogTarget::VersionManager,
                            tr("rust.status.versions.install-cancelled"),
                        );
                    }
                }
                drop(event_tx);
                let _ = listener.await;
            });

            let _ = done_tx.send(LaunchUiEvent::Finished);
        });
    }

    fn show_version_manager(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(tr("rust.ui.versions.heading"));
            if ui
                .add_enabled(
                    !self.version_catalog_loading,
                    egui::Button::new(tr("rust.ui.versions.refresh-catalog")),
                )
                .clicked()
            {
                self.refresh_version_catalog();
            }
            if self.version_catalog_loading {
                ui.spinner();
            }
        });

        ui.horizontal(|ui| {
            ui.label(tr("rust.ui.search"));
            ui.text_edit_singleline(&mut self.version_search);
        });

        let mut filter_settings_changed = false;
        ui.horizontal_wrapped(|ui| {
            for (kind, label_key) in [
                ("snapshot", "settings.versions.snapshot"),
                ("modified", "settings.versions.modified"),
                ("old-beta", "settings.versions.old_beta"),
                ("old-alpha", "settings.versions.old_alpha"),
                ("launcher", "settings.versions.launcher"),
                ("pending", "settings.versions.pending"),
            ] {
                let enabled = self
                    .settings
                    .version_catalog_types
                    .entry(kind.to_owned())
                    .or_insert_with(|| default_version_type_enabled(kind));
                filter_settings_changed |= ui.checkbox(enabled, tr(label_key)).changed();
            }
        });
        ui.horizontal(|ui| {
            filter_settings_changed |= ui
                .checkbox(
                    &mut self.settings.version_only_installed,
                    tr("settings.versions.only-installed"),
                )
                .changed();
            filter_settings_changed |= ui
                .checkbox(
                    &mut self.settings.version_include_remote,
                    tr("rust.ui.versions.filter.remote"),
                )
                .changed();
            filter_settings_changed |= ui
                .checkbox(
                    &mut self.settings.version_show_old_releases,
                    tr("settings.versions.sub.old_release"),
                )
                .changed();
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("settings.alert-on.label"));
            for (kind, label_key) in [
                ("release", "settings.alert-on.release"),
                ("snapshot", "settings.alert-on.snapshot"),
                ("modified", "settings.alert-on.modified"),
            ] {
                let enabled = self
                    .settings
                    .version_alert_types
                    .entry(kind.to_owned())
                    .or_insert(false);
                filter_settings_changed |= ui.checkbox(enabled, tr(label_key)).changed();
            }
        });
        if filter_settings_changed {
            self.persist();
        }

        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.version_kind_filter,
                "all".to_owned(),
                tr("rust.ui.versions.filter.all"),
            );
            ui.selectable_value(
                &mut self.version_kind_filter,
                "release".to_owned(),
                tr("version.release"),
            );
            ui.selectable_value(
                &mut self.version_kind_filter,
                "snapshot".to_owned(),
                tr("settings.versions.snapshot"),
            );
            ui.selectable_value(
                &mut self.version_kind_filter,
                "modified".to_owned(),
                tr("settings.versions.modified"),
            );
            ui.selectable_value(
                &mut self.version_kind_filter,
                "old".to_owned(),
                tr("rust.ui.versions.filter.old"),
            );
            ui.selectable_value(
                &mut self.version_kind_filter,
                "launcher".to_owned(),
                tr("settings.versions.launcher"),
            );
            ui.selectable_value(
                &mut self.version_kind_filter,
                "pending".to_owned(),
                tr("settings.versions.pending"),
            );
        });

        if self.version_manifest.is_none() && !self.version_catalog_loading {
            ui.colored_label(
                Color32::from_rgb(238, 183, 85),
                tr("rust.ui.versions.catalog-fallback"),
            );
        }

        let entries = self.visible_catalog_entries();
        let installed: HashSet<String> = self.installed_versions().into_iter().collect();
        let busy = self.launching;
        egui::ScrollArea::vertical()
            .max_height(330.0)
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                if entries.is_empty() {
                    ui.label(tr("rust.ui.versions.empty"));
                }
                for (version, kind) in entries {
                    let is_installed = installed.contains(&version);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&version).strong());
                        ui.label(RichText::new(&kind).weak());
                        if is_installed {
                            ui.label(
                                RichText::new(tr("rust.ui.versions.installed"))
                                    .color(Color32::LIGHT_GREEN),
                            );
                        }
                        if ui.small_button(tr("rust.ui.select")).clicked() {
                            self.settings.selected_version = version.clone();
                            self.persist();
                        }
                        let install_label = tr(if is_installed {
                            "rust.ui.versions.repair"
                        } else {
                            "rust.ui.versions.install"
                        });
                        if ui
                            .add_enabled(!busy, egui::Button::new(install_label).small())
                            .clicked()
                        {
                            self.start_version_install(version.clone());
                        }
                        if is_installed
                            && ui
                                .add_enabled(!busy, egui::Button::new(tr("rust.ui.remove")).small())
                                .clicked()
                        {
                            self.confirm_remove_version = Some(version.clone());
                        }
                    });
                }
            });
    }
}

impl eframe::App for LauncherApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx();
        if !self.launcher_log_initialized {
            self.launcher_log_initialized = true;
            record_launcher_log_line(&self.launcher_log, tr("rust.log.launcher-started"));
        }
        let connectivity_report = self
            .connectivity_events
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok());
        if let Some(report) = connectivity_report {
            self.connectivity_events = None;
            for service in &report.services {
                record_launcher_log_event(
                    &self.launcher_log,
                    if service.reachable {
                        LogLevel::Info
                    } else {
                        LogLevel::Warn
                    },
                    LogTarget::Connectivity,
                    &format!(
                        "Connectivity check {}: {}{}",
                        service.id,
                        if service.reachable {
                            "reachable"
                        } else {
                            "unreachable"
                        },
                        if service.hosts.is_empty() {
                            String::new()
                        } else {
                            format!(" ({})", service.hosts.join(", "))
                        }
                    ),
                );
            }
            self.connectivity_report = Some(report);
        }
        if let Some(maximized) = ctx.input(|input| input.viewport().maximized) {
            if self.launcher_window_state_seen && maximized != self.settings.launcher_maximized {
                self.settings.launcher_maximized = maximized;
                self.persist();
            }
            self.launcher_window_state_seen = true;
        }
        if ctx.input(|input| input.viewport().close_requested())
            && !self.settings.feedback_acknowledged
            && self.bootstrap_feedback_url.is_some()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.settings.feedback_acknowledged = true;
            self.persist();
            self.show_feedback_dialog = true;
        }
        if self.auto_launch_pending
            && !self.show_first_run_notice
            && !self.show_first_run_folder_notice
            && !self.show_arm_compat_notice
        {
            self.auto_launch_pending = false;
            let timeout = self.settings.auto_login_timeout_secs.clamp(2, 10);
            self.auto_launch_deadline =
                Some(Instant::now() + Duration::from_secs(u64::from(timeout)));
        }
        if let Some(deadline) = self.auto_launch_deadline {
            if !self.settings.auto_login || self.settings.selected_account.is_none() {
                self.auto_launch_deadline = None;
            } else if Instant::now() >= deadline {
                self.auto_launch_deadline = None;
                self.start_selected_game();
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
        if self.legacy_theme_palette_path != self.settings.legacy_theme_path {
            self.legacy_theme_palette = load_legacy_theme_palette(&self.settings.legacy_theme_path);
            self.legacy_theme_palette_path = self.settings.legacy_theme_path.clone();
        }
        let theme_preference = match self.settings.look_and_feel.as_str() {
            "off" | "system" | "auto" => egui::ThemePreference::System,
            "light" => egui::ThemePreference::Light,
            _ => egui::ThemePreference::Dark,
        };
        ctx.set_theme(theme_preference);
        let theme = ctx.theme();
        let dark_theme_assets = self
            .legacy_theme_palette
            .as_ref()
            .and_then(|palette| palette.dark_theme_enabled)
            .unwrap_or_else(|| {
                theme == egui::Theme::Dark
                    || self
                        .legacy_theme_palette
                        .as_ref()
                        .and_then(|palette| palette.background)
                        .is_some_and(legacy_background_prefers_dark_assets)
            });
        if self.background_theme_dark != Some(dark_theme_assets) {
            self.background_theme_dark = Some(dark_theme_assets);
            if !self.background_custom_loaded {
                if let Some(assets) = &mut self.assets {
                    assets.background = if dark_theme_assets {
                        assets.background_dark.clone()
                    } else {
                        assets.background_light.clone()
                    };
                }
            }
        }
        let mut style = (*ctx.style_of(theme)).clone();
        style.visuals = theme.default_visuals();
        if theme == egui::Theme::Dark {
            style.visuals.window_fill = Color32::from_rgb(31, 35, 38);
            style.visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(52, 57, 60);
            style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(75, 80, 82);
            style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(94, 101, 104);
            style.visuals.widgets.active.bg_fill = Color32::from_rgb(64, 116, 144);
            style.visuals.selection.bg_fill = Color32::from_rgb(54, 116, 153);
        }
        if let Some(palette) = &self.legacy_theme_palette {
            apply_legacy_theme_palette(&mut style, palette);
        }
        style.spacing.item_spacing = Vec2::new(0.0, 5.0);
        style.spacing.interact_size.y = 22.0;
        ctx.set_style_of(theme, style);

        let mut updates = Vec::new();
        let mut finished = false;
        if let Some(receiver) = &self.launch_events {
            while let Ok(event) = receiver.try_recv() {
                match event {
                    LaunchUiEvent::Finished => finished = true,
                    _ => updates.push(event),
                }
            }
        }
        for event in updates {
            match event {
                LaunchUiEvent::Message(message) => {
                    record_launcher_log_event(
                        &self.launcher_log,
                        message.level,
                        message.target,
                        &message.message,
                    );
                    if let Some(progress) = parse_launch_progress(&message.message) {
                        self.launch_progress = Some(progress);
                    }
                    self.status = message.message;
                }
                LaunchUiEvent::LongMalwareScanConfirmation(reply) => {
                    self.malware_scan_confirmation = Some(reply);
                }
                LaunchUiEvent::MalwareDetected(detection) => {
                    self.settings.malware_scan_version = 0;
                    self.persist();
                    self.status = tr("jarscanner.detected").to_owned();
                    self.malware_detection = Some(detection);
                }
                LaunchUiEvent::GameStarted => {
                    let message = tr("rust.status.game.starting").to_owned();
                    record_launcher_log_event(
                        &self.launcher_log,
                        LogLevel::Info,
                        LogTarget::Minecraft,
                        &message,
                    );
                    self.game_running = true;
                    self.status = message;
                    self.launch_progress = None;
                    match self.settings.launch_action.as_str() {
                        "hide" => {
                            self.hidden_for_game = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                        }
                        "exit" => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                        _ => {}
                    }
                }
                LaunchUiEvent::Finished => {}
            }
        }
        if finished {
            self.launching = false;
            self.game_running = false;
            self.launch_progress = None;
            self.launch_events = None;
            if self.hidden_for_game {
                self.hidden_for_game = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            }
            let pending_report = self
                .pending_crash_report
                .lock()
                .ok()
                .and_then(|mut pending| pending.take());
            if let Some(report) = pending_report {
                self.set_logged_status(LogLevel::Error, LogTarget::Crash, report.summary.clone());
                self.crash_report = Some(report);
            }
        }

        let mut catalog_update = None;
        if let Some(receiver) = &self.version_catalog_events {
            if let Ok(result) = receiver.try_recv() {
                catalog_update = Some(result);
            }
        }
        if let Some(result) = catalog_update {
            self.version_catalog_events = None;
            self.version_catalog_loading = false;
            match result {
                Ok(manifest) => {
                    let version_count = manifest.versions.len();
                    let first_update = !self.version_alert_has_refreshed;
                    self.version_alert_has_refreshed = true;
                    if let Some(alert) =
                        refresh_version_alert_index(&manifest, &self.settings, first_update)
                    {
                        self.version_update_alert = Some(alert);
                    }
                    self.version_manifest = Some(manifest);
                    self.set_logged_status(
                        LogLevel::Info,
                        LogTarget::VersionCatalog,
                        tr("rust.status.catalog.loaded").replace("%0", &version_count.to_string()),
                    );
                }
                Err(error) => {
                    self.set_logged_status(LogLevel::Error, LogTarget::VersionCatalog, error);
                }
            }
        }
        let mut update_check_result = None;
        if let Some(receiver) = &self.update_check_events {
            if let Ok(result) = receiver.try_recv() {
                update_check_result = Some(result);
            }
        }
        if let Some(result) = update_check_result {
            self.update_check_events = None;
            self.update_checking = false;
            match result {
                Ok(github_update::CheckResult::NoPublishedRelease) => {
                    self.set_logged_status(
                        LogLevel::Info,
                        LogTarget::Update,
                        tr("update.check.no-release"),
                    );
                }
                Ok(github_update::CheckResult::UpToDate) => {
                    self.set_logged_status(
                        LogLevel::Info,
                        LogTarget::Update,
                        tr("update.check.current").replace("%0", env!("CARGO_PKG_VERSION")),
                    );
                }
                Ok(github_update::CheckResult::UpdateAvailable(release)) => {
                    self.set_logged_status(
                        LogLevel::Info,
                        LogTarget::Update,
                        format!("{} {}", tr("update.check.available"), release.tag_name),
                    );
                    self.available_release = Some(release);
                    self.update_prompt_pending = true;
                }
                Err(error) => {
                    self.set_logged_status(
                        LogLevel::Error,
                        LogTarget::Update,
                        tr("update.check.failed").replace("%0", &error.to_string()),
                    );
                }
            }
        }
        let mut update_install_result = None;
        if let Some(receiver) = &self.update_install_events {
            if let Ok(result) = receiver.try_recv() {
                update_install_result = Some(result);
            }
        }
        if let Some(result) = update_install_result {
            self.update_install_events = None;
            self.update_installing = false;
            match result {
                Ok(()) => {
                    self.set_logged_status(
                        LogLevel::Info,
                        LogTarget::Update,
                        tr("update.installing"),
                    );
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Err(error) => {
                    self.update_install_error = Some(error.clone());
                    self.set_logged_status(LogLevel::Error, LogTarget::Update, error);
                }
            }
        }
        if self.update_prompt_pending
            && !self.show_first_run_notice
            && !self.show_first_run_folder_notice
            && !self.show_arm_compat_notice
        {
            self.update_prompt_pending = false;
            self.show_update_dialog = true;
        }
        let mut bootstrap_update = None;
        if let Some(receiver) = &self.bootstrap_events {
            if let Ok(result) = receiver.try_recv() {
                bootstrap_update = Some(result);
            }
        }
        if let Some(result) = bootstrap_update {
            self.bootstrap_events = None;
            self.bootstrap_loading = false;
            match result {
                Ok(configuration) => {
                    let notices_were_enabled = self.settings.notices_enabled;
                    minecraft_java_rs_core::net::client::set_repository_overrides(
                        configuration.repositories.clone(),
                    );
                    spawn_crash_signature_fetch(
                        configuration.extra_version_repositories.clone(),
                        Arc::clone(&self.launcher_log),
                    );
                    self.notices_disable_allowed =
                        configuration.allows_notice_disable(&self.settings.client_id);
                    self.allow_ely_everywhere = configuration.allow_ely_everywhere;
                    if !configuration.extra_version_repositories.is_empty()
                        && self.extra_version_repositories
                            != configuration.extra_version_repositories
                    {
                        self.extra_version_repositories =
                            configuration.extra_version_repositories.clone();
                        self.start_version_catalog_fetch();
                    }
                    if !self.notices_disable_allowed {
                        self.settings.notices_enabled = true;
                        if !notices_were_enabled {
                            self.persist();
                        }
                    }
                    self.bootstrap_notice_list = configuration.notices;
                    self.bootstrap_promoted_servers = configuration.promoted_servers;
                    self.bootstrap_feedback_url = configuration.feedback_url;
                    self.bootstrap_notification_texture = configuration
                        .notification
                        .as_ref()
                        .and_then(|notification| notification.image.as_ref())
                        .and_then(|bytes| {
                            load_optional_texture(ctx, "bootstrap_notification", bytes)
                        });
                    self.bootstrap_notification = configuration.notification;
                    self.bootstrap_error = None;
                    self.rebuild_notice_list(ctx);
                    if !notices_were_enabled && self.settings.notices_enabled {
                        self.refresh_notices();
                    }
                    record_launcher_log_event(
                        &self.launcher_log,
                        LogLevel::Info,
                        LogTarget::Bootstrap,
                        &tr("rust.log.bootstrap.loaded"),
                    );
                }
                Err(error) => {
                    let notices_were_enabled = self.settings.notices_enabled;
                    self.notices_disable_allowed = false;
                    self.allow_ely_everywhere = false;
                    self.settings.notices_enabled = true;
                    if !notices_were_enabled {
                        self.persist();
                        self.refresh_notices();
                    }
                    record_launcher_log_event(
                        &self.launcher_log,
                        LogLevel::Warn,
                        LogTarget::Bootstrap,
                        &error,
                    );
                    self.bootstrap_error = Some(error);
                }
            }
        }

        let mut notice_update = None;
        if let Some(receiver) = &self.notice_events {
            if let Ok(result) = receiver.try_recv() {
                notice_update = Some(result);
            }
        }
        if let Some(result) = notice_update {
            self.notice_events = None;
            self.notices_loading = false;
            match result {
                Ok(notices) => {
                    record_launcher_log_event(
                        &self.launcher_log,
                        LogLevel::Info,
                        LogTarget::Notices,
                        &tr("rust.log.notices.loaded").replace("%0", &notices.len().to_string()),
                    );
                    self.personal_notice_list = notices;
                    if !self.settings.notices_enabled {
                        self.personal_notice_list.clear();
                    }
                    self.notice_error = None;
                }
                Err(error) => {
                    record_launcher_log_event(
                        &self.launcher_log,
                        LogLevel::Warn,
                        LogTarget::Notices,
                        &error,
                    );
                    self.notice_error = Some(error);
                    self.personal_notice_list.clear();
                }
            }
            self.rebuild_notice_list(ctx);
        }
        let mut diagnostic_update = None;
        if let Some(receiver) = &self.diagnostic_events {
            if let Ok(result) = receiver.try_recv() {
                diagnostic_update = Some(result);
            }
        }
        if let Some(result) = diagnostic_update {
            self.diagnostic_events = None;
            self.diagnostic_pending = false;
            match result {
                Ok(url) => {
                    record_launcher_log_event(
                        &self.launcher_log,
                        LogLevel::Info,
                        LogTarget::Diagnostics,
                        &tr("rust.log.diagnostics.uploaded"),
                    );
                    self.diagnostic_result_url = Some(url);
                    self.diagnostic_error = None;
                }
                Err(error) => {
                    record_launcher_log_event(
                        &self.launcher_log,
                        LogLevel::Error,
                        LogTarget::Diagnostics,
                        &error,
                    );
                    self.diagnostic_error = Some(error);
                }
            }
        }
        let mut microsoft_signin_update = None;
        if let Some(receiver) = &self.microsoft_signin_events {
            if let Ok(result) = receiver.try_recv() {
                microsoft_signin_update = Some(result);
            }
        }
        if let Some(result) = microsoft_signin_update {
            self.apply_microsoft_sign_in(result);
        }
        let mut ely_legacy_signin_update = None;
        if let Some(receiver) = &self.ely_legacy_signin_events {
            if let Ok(result) = receiver.try_recv() {
                ely_legacy_signin_update = Some(result);
            }
        }
        if let Some(result) = ely_legacy_signin_update {
            self.save_ely_legacy_account(result);
        }
        let mut ely_signin_update = None;
        if let Some(receiver) = &self.ely_signin_events {
            if let Ok(result) = receiver.try_recv() {
                ely_signin_update = Some(result);
            }
        }
        if let Some(result) = ely_signin_update {
            self.save_ely_account(result);
        }
        let mut profile_creation_update = None;
        if let Some(receiver) = &self.profile_creation_events {
            if let Ok(result) = receiver.try_recv() {
                profile_creation_update = Some(result);
            }
        }
        if let Some(result) = profile_creation_update {
            self.profile_creation_events = None;
            self.profile_creation_pending = false;
            match result {
                Ok(account) => self.save_microsoft_account(account),
                Err(error) => {
                    self.set_logged_status(LogLevel::Error, LogTarget::Authentication, error)
                }
            }
        }
        if self.launching {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
        if self.version_catalog_loading
            || self.update_checking
            || self.update_installing
            || self.notices_loading
            || self.microsoft_signin_pending
            || self.ely_signin_pending
            || self.ely_legacy_signin_pending
            || self.profile_creation_pending
        {
            ctx.request_repaint_after(Duration::from_millis(150));
        }

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
            .unwrap_or(Color32::from_rgba_unmultiplied(23, 29, 34, 198));
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

        // Match the original login panel's compact left-side placement.
        let (panel_anchor, panel_offset) = login_form_anchor(&self.settings.login_form_direction);
        egui::Area::new(Id::unique("legacy_login_panel"))
            .order(egui::Order::Foreground)
            .anchor(panel_anchor, panel_offset)
            .show(ctx, |ui| {
                let mut panel_frame = egui::Frame::new()
                    .fill(login_panel_fill)
                    .stroke(Stroke::new(
                        login_panel_border_size,
                        login_panel_border.unwrap_or(Color32::from_white_alpha(16)),
                    ))
                    .inner_margin(Margin::symmetric(24, 28));
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
                    ui.set_width(236.0);
                    let line_rect = ui
                        .allocate_exact_size(Vec2::new(236.0, 1.0), Sense::hover())
                        .0;
                    ui.painter().line_segment(
                        [line_rect.left_top(), line_rect.right_top()],
                        Stroke::new(1.0, Color32::from_white_alpha(110)),
                    );
                    ui.add_space(5.0);

                    ComboBox::from_id_salt("account_selector")
                        .width(236.0)
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
                                open_url("https://minecraft.net/store/minecraft");
                            }
                            if ui.button(tr("loginform.button.settings.account")).clicked() {
                                self.settings_page = Some("Manage accounts");
                            }
                        });

                    ComboBox::from_id_salt("version_selector")
                        .width(236.0)
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
                        });

                    ui.add_space(5.0);
                    if ui
                        .checkbox(
                            &mut self.settings.auto_login,
                            tr("loginform.checkbox.autologin"),
                        )
                        .changed()
                    {
                        if !self.settings.auto_login {
                            self.auto_launch_deadline = None;
                        }
                        self.persist();
                    }
                    if ui
                        .checkbox(
                            &mut self.settings.force_update,
                            tr("loginform.checkbox.forceupdate"),
                        )
                        .changed()
                    {
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
                            play_button.min_size(Vec2::new(236.0, 46.0)),
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
                        let support = icon_button(ui, "support", &support_icon, support_icon_tint);
                        if support.clicked() {
                            self.show_support_menu = !self.show_support_menu;
                            self.support_menu_pos =
                                support.rect.left_bottom() + Vec2::new(-54.0, 1.0);
                        }
                        support.on_hover_text(tr("loginform.button.support"));

                        if !self.settings.settings_blocked {
                            let folder = icon_button(ui, "folder", &folder_icon, folder_icon_tint);
                            if folder.clicked() {
                                self.show_folder_menu = !self.show_folder_menu;
                                self.folder_menu_pos =
                                    folder.rect.left_bottom() + Vec2::new(-70.0, 1.0);
                            }
                            folder.on_hover_text(tr("loginform.button.folder"));
                        }

                        let refresh = icon_button(ui, "refresh", &refresh_icon, refresh_icon_tint);
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

                        if !self.settings.settings_blocked {
                            let settings =
                                icon_button(ui, "settings", &settings_icon, settings_icon_tint);
                            if settings.clicked() {
                                self.show_settings_menu = !self.show_settings_menu;
                                self.settings_menu_pos =
                                    settings.rect.left_bottom() + Vec2::new(-58.0, 1.0);
                            }
                            settings.on_hover_text(tr("loginform.button.settings"));
                        }
                    });
                });
            });

        if let Some(notification) = self.bootstrap_notification.clone() {
            let notification_texture = self.bootstrap_notification_texture.clone();
            let clicked = egui::Area::new(Id::unique((
                "bootstrap_notification",
                notification.id.as_str(),
            )))
            .anchor(Align2::RIGHT_TOP, Vec2::new(-12.0, 12.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
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
                open_url(&notification.url);
                self.bootstrap_notification = None;
                self.bootstrap_notification_texture = None;
            }
        }

        if self.show_folder_menu {
            egui::Area::new(Id::unique("folder_popup"))
                .order(egui::Order::Foreground)
                .fixed_pos(self.folder_menu_pos)
                .show(ctx, |ui| {
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
                                open_path(
                                    &self.game_root_for_version(&self.settings.selected_version),
                                );
                                self.show_folder_menu = false;
                            }
                        }
                        if self.settings.loader_type != "none"
                            && ui.button(tr("loginform.button.folder.mods")).clicked()
                        {
                            let mods = self
                                .game_data_directory_for_version(&self.settings.selected_version)
                                .join("mods");
                            match fs::create_dir_all(&mods) {
                                Ok(()) => open_path(&mods),
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
                        if ui.button(tr("loginform.button.folder.logs")).clicked() {
                            self.selected_log_path = None;
                            self.show_log_window = true;
                            self.show_folder_menu = false;
                        }
                        if ui.button(tr("loginform.button.folder.root")).clicked() {
                            open_path(&self.game_dir_path());
                            self.show_folder_menu = false;
                        }
                        if ui.small_button(tr("support.button.close")).clicked() {
                            self.show_folder_menu = false;
                        }
                    });
                });
        }

        if self.show_support_menu {
            egui::Area::new(Id::unique("support_popup"))
                .order(egui::Order::Foreground)
                .fixed_pos(self.support_menu_pos)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(220.0);
                        if ui.button(tr("loginform.button.support.discord")).clicked() {
                            open_url("https://llaun.ch/discord/intl");
                            self.show_support_menu = false;
                        }
                        ui.separator();
                        if ui.button(tr("loginform.button.support")).clicked() {
                            self.show_support_window = true;
                            self.show_support_menu = false;
                        }
                    });
                });
        }

        if self.show_support_window {
            let mut open = true;
            let mut open_diagnostic_consent = false;
            egui::Window::new(tr("support.contact.title"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(tr("support.contact.body"));
                    ui.horizontal(|ui| {
                        if ui.button(tr("support.contact.buttons.discord")).clicked() {
                            open_url("https://llaun.ch/support/discord/intl");
                        }
                        if ui.button(tr("support.contact.buttons.mail")).clicked() {
                            open_url("mailto:support@llaun.ch");
                        }
                    });
                    if ui.button(tr("support.pre.diag.send")).clicked() {
                        open_diagnostic_consent = true;
                    }
                });
            if open_diagnostic_consent {
                open = false;
                self.show_diagnostic_consent = true;
                self.diagnostic_send_enabled = true;
                self.diagnostic_error = None;
            }
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
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
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
                                open_url(url);
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
                            open_url(tr("support.pre.diag.url"));
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
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(tr("feedback.body"));
                    ui.horizontal(|ui| {
                        if ui.button(tr("feedback.button.okay")).clicked() {
                            if let Some(url) = &feedback_url {
                                open_url(url);
                            }
                            close_launcher = true;
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
            egui::Area::new(Id::unique("settings_popup"))
                .order(egui::Order::Foreground)
                .fixed_pos(self.settings_menu_pos)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(220.0);
                        if ui
                            .button(tr("loginform.button.settings.launcher"))
                            .clicked()
                        {
                            self.settings_page = Some("Launcher & game preferences");
                            self.show_settings_menu = false;
                        }
                        if ui.button(tr("loginform.button.settings.version")).clicked() {
                            self.settings_page = Some("Manage versions");
                            self.show_settings_menu = false;
                        }
                        if ui.button(tr("loginform.button.settings.account")).clicked() {
                            self.settings_page = Some("Manage accounts");
                            self.show_settings_menu = false;
                        }
                        if !self.notice_list.is_empty() {
                            if ui.button(tr("loginform.button.settings.notices")).clicked() {
                                self.show_notices_window = true;
                                self.show_settings_menu = false;
                            }
                        }
                    });
                });
        }

        if self.show_update_dialog {
            if let Some(release) = self.available_release.clone() {
                let mut open = true;
                let mut close = false;
                let mut start_install = false;
                let install_error = self.update_install_error.clone();
                egui::Window::new(tr("update.title"))
                    .open(&mut open)
                    .collapsible(false)
                    .resizable(true)
                    .default_width(460.0)
                    .show(ctx, |ui| {
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
                                open_url(&release.html_url);
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
                    .order(egui::Order::Middle)
                    .show(ctx, |ui| {
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
                                ui.label(&notice.text);
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
                    self.perform_notice_action(action, ctx);
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
                .open(&mut open)
                .default_size([540.0, 430.0])
                .resizable(true)
                .show(ctx, |ui| {
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
                                ui.label(&notice.text);
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
                self.perform_notice_action(action, ctx);
            }
        }

        if let Some(report) = self.connectivity_report.clone()
            && report.has_critical_failures()
        {
            egui::Area::new(Id::unique("connectivity_warning_badge"))
                .anchor(Align2::RIGHT_TOP, Vec2::new(-12.0, 58.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
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
                egui::Window::new(tr("connectivity.warning.title"))
                    .open(&mut open)
                    .default_size([500.0, 460.0])
                    .resizable(true)
                    .show(ctx, |ui| {
                        let no_connection = report.all_services_unreachable();
                        let body_key = if no_connection {
                            "connectivity.warning.body.empty"
                        } else {
                            "connectivity.warning.body.text"
                        };
                        ui.label(localized_html_to_plain_text(&tr(body_key)));
                        if ui.link(tr("connectivity.warning.body.link")).clicked() {
                            open_url(if no_connection {
                                "https://wiki.llaun.ch/en:connectivity:none"
                            } else {
                                "https://wiki.llaun.ch/en:connectivity:some"
                            });
                        }
                        ui.separator();
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for service in report
                                .services
                                .iter()
                                .filter(|service| !service.reachable)
                            {
                                let (name_key, hint_key) = match service.id {
                                    "official_repo" => (
                                        "connectivity.warning.list.name.official_repo",
                                        "rust.connectivity.hint.official_repo",
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
                    });
                self.show_connectivity_window = open;
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
                .order(egui::Order::Foreground)
                .fixed_pos(screen_rect.min)
                .interactable(true)
                .show(ctx, |ui| {
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
                                                                        ui.label(&notice.text);
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
                self.perform_notice_action(action, ctx);
            }
        }

        if let Some(pending) = self.pending_promoted_server.clone() {
            let installed_versions: HashSet<String> =
                self.installed_versions().into_iter().collect();
            let mut open = true;
            let mut cancelled = false;
            let mut selected_version = None;
            egui::Window::new(tr("rust.ui.window.servers.choose-version"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
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
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button(tr("rust.ui.logs.open-folder")).clicked() {
                            let directory = game_directory.join("logs");
                            let _ = fs::create_dir_all(&directory);
                            open_path(&directory);
                        }
                        if ui.button(tr("rust.ui.logs.open-launcher-folder")).clicked() {
                            let directory = launcher_log_path().parent().map(Path::to_path_buf);
                            if let Some(directory) = directory {
                                let _ = fs::create_dir_all(&directory);
                                open_path(&directory);
                            }
                        }
                        if ui.button(tr("rust.ui.logs.open-crash-folder")).clicked() {
                            let directory = game_directory.join("crash-reports");
                            let _ = fs::create_dir_all(&directory);
                            open_path(&directory);
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
                                let age_label = if age.as_secs() < 60 {
                                    format!("{}s ago", age.as_secs())
                                } else if age.as_secs() < 3_600 {
                                    format!("{}m ago", age.as_secs() / 60)
                                } else if age.as_secs() < 86_400 {
                                    format!("{}h ago", age.as_secs() / 3_600)
                                } else {
                                    format!("{}d ago", age.as_secs() / 86_400)
                                };
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
                                        open_path(path);
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
                .open(&mut open)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
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
                .open(&mut open)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(true)
                .default_size([440.0, 360.0])
                .show(ctx, |ui| {
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

        if self.show_account_dialog {
            let editing = self.editing_account.is_some();
            let title = if editing {
                tr("rust.ui.window.account.offline-edit")
            } else {
                tr("rust.ui.window.account.offline-add")
            };
            egui::Window::new(title)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
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
        }

        if self.show_account_help {
            let mut open = true;
            let mut close = false;
            let mut open_minecraft_store = false;
            let mut open_ely_registration = false;
            egui::Window::new(tr("rust.ui.window.account.help"))
                .open(&mut open)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
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
                open_url("https://minecraft.net/store/minecraft");
            }
            if open_ely_registration {
                open_url("https://ely.by");
            }
        }

        if self.show_ely_dialog {
            let mut open = true;
            egui::Window::new(tr("rust.ui.window.account.ely"))
                .open(&mut open)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
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
        }

        if self.show_ely_legacy_dialog {
            let mut open = true;
            egui::Window::new(tr("rust.ui.window.account.ely-legacy"))
                .open(&mut open)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
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
            }
        }

        if self.show_minecraft_profile_dialog {
            let mut open = true;
            egui::Window::new(tr("rust.ui.window.account.minecraft-profile"))
                .open(&mut open)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
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
        }

        if let Some(title) = self.settings_page {
            if title == "Manage servers" {
                let root = self.game_root_for_version(&self.settings.selected_version);
                if self.server_list_root.as_deref() != Some(root.as_path()) {
                    self.load_server_list(root);
                }
            }
            let mut open = true;
            let mut browse_game_directory = false;
            let mut browse_background_file = false;
            let mut browse_theme_file = false;
            let mut logger_mode_changed = false;
            egui::Window::new(settings_page_title(title))
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .default_size([520.0, 500.0])
                .show(ctx, |ui| match title {
                    "About us" => {
                        ui.horizontal(|ui| {
                            ui.heading("Legacy Launcher");
                            ui.label(format!("v{}", env!("CARGO_PKG_VERSION")));
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.label("(by");
                            if ui.link("turikhay").clicked() {
                                open_url("https://turikhay.com");
                            }
                            ui.label(") —");
                            ui.label(localized_html_to_plain_text(&tr("about.text")));
                        });
                        ui.add_space(10.0);
                        ui.separator();
                        ui.horizontal_wrapped(|ui| {
                            if ui.link("Ely.by").clicked() {
                                open_url("https://ely.by");
                            }
                            ui.label("—");
                            ui.label(localized_html_to_plain_text(&tr("about.partners.ely")));
                        });
                        ui.separator();
                        ui.label(localized_html_to_plain_text(&tr("about.license")));
                    }
                    "Launcher & game preferences" => {
                        ui.label(tr("rust.settings.locale.english-us"));
                        ui.horizontal_wrapped(|ui| {
                            if ui.small_button(tr("settings.tab.about")).clicked() {
                                self.settings_page = Some("About us");
                            }
                            if ui.small_button(tr("rust.settings.page.servers")).clicked() {
                                self.settings_page = Some("Manage servers");
                                self.server_list_root = None;
                            }
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
                                open_path(&self.game_dir_path());
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label(tr("launcher.window.size"));
                            let mut changed = false;
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut self.settings.launcher_window_width)
                                        .range(540..=7680)
                                        .suffix(" px"),
                                )
                                .changed();
                            ui.label("×");
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut self.settings.launcher_window_height)
                                        .range(360..=4320)
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
                                    open_path(&runtime_root);
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
                                match import_legacy_settings(&path, &mut self.settings) {
                                    Ok(summary) => {
                                        self.persisted_settings = self.settings.clone();
                                        self.command_line_overrides.apply(&mut self.settings);
                                        self.set_logged_status(
                                            LogLevel::Info,
                                            LogTarget::Settings,
                                            tr("rust.status.import.settings.success")
                                                .replace("%0", &summary.applied.to_string())
                                                .replace("%1", &summary.skipped.to_string()),
                                        );
                                        ui.ctx().set_zoom_factor(
                                            self.settings.launcher_font_size / 12.0,
                                        );
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
                    "Manage versions" => {
                        self.show_version_manager(ui);
                    }
                    "Manage accounts" => {
                        ui.heading(tr("rust.settings.import.accounts.heading"));
                        ui.horizontal(|ui| {
                            ui.text_edit_singleline(&mut self.settings.legacy_profiles_path);
                            if ui
                                .small_button(tr("rust.settings.import.accounts.button"))
                                .clicked()
                            {
                                let path = if self.settings.legacy_profiles_path.trim().is_empty() {
                                    self.game_dir_path().join("tlauncher_profiles.json")
                                } else {
                                    PathBuf::from(self.settings.legacy_profiles_path.trim())
                                };
                                match import_legacy_accounts(&path, &mut self.settings) {
                                    Ok(summary) => {
                                        self.set_logged_status(
                                            LogLevel::Info,
                                            LogTarget::Authentication,
                                            tr("rust.status.import.accounts.success")
                                                .replace("%0", &summary.offline.to_string())
                                                .replace("%1", &summary.microsoft.to_string())
                                                .replace("%2", &summary.ely.to_string())
                                                .replace("%3", &summary.ely_legacy.to_string())
                                                .replace("%4", &summary.mojang_migrated.to_string())
                                                .replace("%5", &summary.skipped.to_string()),
                                        );
                                        self.persist();
                                    }
                                    Err(error) => {
                                        self.set_logged_status(
                                            LogLevel::Error,
                                            LogTarget::Authentication,
                                            tr("rust.status.import.accounts.error")
                                                .replace("%0", &error.to_string()),
                                        );
                                    }
                                }
                            }
                        });
                        if self.settings.legacy_profiles_path.trim().is_empty() {
                            ui.label(
                                RichText::new(tr("rust.ui.account.legacy-path-default")).small(),
                            );
                        }
                        ui.separator();
                        if self.settings.accounts.is_empty()
                            && self.settings.microsoft_accounts.is_empty()
                                            && self.settings.ely_accounts.is_empty()
                            && self.settings.ely_legacy_accounts.is_empty()
                        {
                            ui.label(tr("account.empty"));
                        }
                        let accounts = self.settings.accounts.clone();
                        let mut remove_account = None;
                        for account in accounts {
                            ui.horizontal(|ui| {
                                ui.label(&account);
                                if ui
                                    .small_button(tr("rust.settings.account.select"))
                                    .clicked()
                                {
                                    self.settings.selected_account = Some(account.clone());
                                    self.persist();
                                }
                                if ui
                                    .small_button(tr("rust.settings.account.edit"))
                                    .clicked()
                                {
                                    self.open_edit_account_dialog(account.clone());
                                }
                                if ui
                                    .small_button(tr("rust.settings.account.remove"))
                                    .clicked()
                                {
                                    remove_account = Some(account.clone());
                                }
                            });
                        }
                        if let Some(account) = remove_account {
                            self.settings.accounts.retain(|saved| saved != &account);
                            self.settings
                                .migrated_mojang_accounts
                                .retain(|saved| !saved.eq_ignore_ascii_case(&account));
                            self.settings
                                .offline_account_uuids
                                .retain(|saved, _| !saved.eq_ignore_ascii_case(&account));
                            self.settings
                                .offline_ely_skins
                                .retain(|saved, _| !saved.eq_ignore_ascii_case(&account));
                            if self.settings.selected_account.as_deref() == Some(&account) {
                                self.settings.selected_account = self.first_account_key();
                            }
                            self.persist();
                        }
                        let microsoft_accounts = self.settings.microsoft_accounts.clone();
                        let mut remove_microsoft_account = None;
                        for account in microsoft_accounts {
                            ui.horizontal(|ui| {
                                ui.label(tr("rust.ui.account.microsoft-label").replace("%0", &account.username));
                                if ui.small_button(tr("rust.ui.select")).clicked() {
                                    self.settings.selected_account = Some(account.id.clone());
                                    self.persist();
                                }
                                if ui.small_button(tr("rust.ui.remove")).clicked() {
                                    remove_microsoft_account = Some(account);
                                }
                            });
                        }
                        if let Some(account) = remove_microsoft_account {
                            match vault::remove_account(&account.storage_key()) {
                                Ok(()) => {
                                    self.settings
                                        .microsoft_accounts
                                        .retain(|saved| saved.id != account.id);
                                    if self.settings.selected_account.as_deref()
                                        == Some(&account.id)
                                    {
                                        self.settings.selected_account = self.first_account_key();
                                    }
                                    self.persist();
                                }
                                Err(error) => {
                                    self.set_logged_status(
                                        LogLevel::Error,
                                        LogTarget::Authentication,
                                        tr("rust.status.account.microsoft.remove-error")
                                            .replace("%0", &error.to_string()),
                                    );
                                }
                            }
                        }
                        let ely_accounts = self.settings.ely_accounts.clone();
                        let mut remove_ely_account = None;
                        for account in ely_accounts {
                            ui.horizontal(|ui| {
                                ui.label(tr("rust.ui.account.ely-label").replace("%0", &account.display_name));
                                if ui.small_button(tr("rust.ui.select")).clicked() {
                                    self.settings.selected_account = Some(account.id.clone());
                                    self.persist();
                                }
                                if ui.small_button(tr("rust.ui.remove")).clicked() {
                                    remove_ely_account = Some(account);
                                }
                            });
                        }
                        if let Some(account) = remove_ely_account {
                            match vault::remove_account(&account.storage_key()) {
                                Ok(()) => {
                                    self.settings
                                        .ely_accounts
                                        .retain(|saved| saved.id != account.id);
                                    if self.settings.selected_account.as_deref()
                                        == Some(&account.id)
                                    {
                                        self.settings.selected_account = self.first_account_key();
                                    }
                                    self.persist();
                                }
                                Err(error) => {
                                    self.set_logged_status(
                                        LogLevel::Error,
                                        LogTarget::Authentication,
                                        tr("rust.status.account.ely.remove-error")
                                            .replace("%0", &error.to_string()),
                                    );
                                }
                            }
                        }
                        let ely_legacy_accounts = self.settings.ely_legacy_accounts.clone();
                        let mut remove_ely_legacy_account = None;
                        for account in ely_legacy_accounts {
                            ui.horizontal(|ui| {
                                ui.label(tr("rust.ui.account.ely-legacy-label").replace("%0", &account.display_name));
                                if ui.small_button(tr("rust.ui.select")).clicked() {
                                    self.settings.selected_account = Some(account.id.clone());
                                    self.persist();
                                }
                                if ui.small_button(tr("rust.ui.remove")).clicked() {
                                    remove_ely_legacy_account = Some(account);
                                }
                            });
                        }
                        if let Some(account) = remove_ely_legacy_account {
                            match vault::remove_account(&account.storage_key()) {
                                Ok(()) => {
                                    self.settings
                                        .ely_legacy_accounts
                                        .retain(|saved| saved.id != account.id);
                                    if self.settings.selected_account.as_deref()
                                        == Some(&account.id)
                                    {
                                        self.settings.selected_account = self.first_account_key();
                                    }
                                    self.persist();
                                }
                                Err(error) => {
                                    self.set_logged_status(
                                        LogLevel::Error,
                                        LogTarget::Authentication,
                                        tr("rust.status.account.ely.remove-error")
                                            .replace("%0", &error.to_string()),
                                    );
                                }
                            }
                        }
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
                        if self.microsoft_signin_pending {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(tr("rust.ui.account.browser-pending"));
                            });
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
                        if self.ely_signin_pending {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(tr("rust.ui.account.ely-browser-pending"));
                            });
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
                        if self.ely_legacy_signin_pending {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(tr("rust.ui.account.ely-signing-in"));
                            });
                        }
                    }
                    "Manage servers" => {
                        ui.heading(tr("rust.ui.servers.heading"));
                        ui.label(tr("rust.ui.servers.hint"));
                        ui.horizontal(|ui| {
                            ui.label(tr("rust.ui.servers.name"));
                            ui.text_edit_singleline(&mut self.new_server_name);
                            ui.label(tr("rust.ui.servers.address"));
                            ui.text_edit_singleline(&mut self.new_server_address);
                            if ui.button(tr("rust.ui.servers.add")).clicked() {
                                let name = self.new_server_name.trim().to_owned();
                                let address = parse_server_address(&self.new_server_address)
                                    .map(|(_, _, full_address)| full_address);
                                if valid_server_name(&name) && address.is_some() {
                                    let address = address.unwrap_or_default();
                                    if !self.server_list.iter().any(|saved| {
                                        parse_server_address(&saved.address)
                                            .is_some_and(|(_, _, existing)| existing.eq_ignore_ascii_case(&address))
                                    }) {
                                        self.server_list.push(SavedServer {
                                            name,
                                            address,
                                            ..Default::default()
                                        });
                                        self.new_server_name.clear();
                                        self.new_server_address.clear();
                                        self.persist_server_list();
                                    } else {
                                        self.set_logged_status(
                                            LogLevel::Warn,
                                            LogTarget::Servers,
                                            tr("rust.status.servers.duplicate"),
                                        );
                                    }
                                } else {
                                    self.set_logged_status(
                                        LogLevel::Warn,
                                        LogTarget::Servers,
                                        tr("rust.status.servers.invalid-entry"),
                                    );
                                }
                            }
                        });
                        ui.separator();
                        if self.server_list.is_empty() {
                            ui.label(tr("rust.ui.servers.empty"));
                        }
                        let mut remove_server = None;
                        let mut quick_play_server = None;
                        let mut server_list_changed = false;
                        for (index, server) in self.server_list.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                server_list_changed |= ui.text_edit_singleline(&mut server.name).changed();
                                server_list_changed |= ui.text_edit_singleline(&mut server.address).changed();
                                if ui.button(tr("rust.ui.servers.quick-play")).clicked() {
                                    quick_play_server = Some(server.clone());
                                }
                                if ui.small_button(tr("rust.ui.remove")).clicked() {
                                    remove_server = Some(index);
                                }
                            });
                        }
                        if server_list_changed {
                            self.persist_server_list();
                        }
                        if let Some(index) = remove_server {
                            self.server_list.remove(index);
                            self.persist_server_list();
                        }
                        if let Some(server) = quick_play_server {
                            self.start_selected_game_on_server(Some(server));
                        }
                    }
                    _ => {}
                });
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
                    match load_texture_from_path(ctx, "background", &path) {
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
            }
        }

        if let Some(version) = self.confirm_remove_version.clone() {
            let mut close_dialog = false;
            egui::Window::new(tr("rust.ui.window.version.remove"))
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(tr("rust.ui.versions.remove-confirm").replace("%0", &version));
                    ui.horizontal(|ui| {
                        if ui.button(tr("support.button.cancel")).clicked() {
                            close_dialog = true;
                        }
                        if ui.button(tr("rust.ui.versions.remove")).clicked() {
                            let version_root = self
                                .installed_version_root(&version)
                                .unwrap_or_else(|| self.game_root_for_version(&version));
                            match remove_installed_version(&version_root, &version) {
                                Ok(()) => {
                                    let invalidated =
                                        invalidate_game_data_cache(&version_root, &version)
                                            .unwrap_or(0);
                                    let message = if invalidated == 0 {
                                        tr("rust.status.versions.removed").replace("%0", &version)
                                    } else {
                                        tr("rust.status.versions.removed-caches")
                                            .replace("%0", &version)
                                            .replace("%1", &invalidated.to_string())
                                    };
                                    self.set_logged_status(
                                        LogLevel::Info,
                                        LogTarget::VersionManager,
                                        message,
                                    );
                                    if self.settings.selected_version == version {
                                        self.settings.selected_version = self
                                            .available_versions()
                                            .into_iter()
                                            .find(|candidate| candidate != &version)
                                            .unwrap_or_else(|| "latest_release".to_owned());
                                        self.persist();
                                    }
                                }
                                Err(error) => {
                                    self.set_logged_status(
                                        LogLevel::Error,
                                        LogTarget::VersionManager,
                                        tr("rust.status.versions.remove-error")
                                            .replace("%0", &error.to_string()),
                                    );
                                }
                            }
                            close_dialog = true;
                        }
                    });
                });
            if close_dialog {
                self.confirm_remove_version = None;
            }
        }

        if let Some(detection) = self.malware_detection.clone() {
            let mut open = true;
            let mut close = false;
            egui::Window::new(tr("rust.malware.detected.title"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
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
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
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
            let permit_help = report
                .suggestions
                .first()
                .is_none_or(|suggestion| suggestion.permit_help);
            egui::Window::new(tr("rust.ui.window.crash"))
                .open(&mut open)
                .default_size([660.0, 520.0])
                .resizable(true)
                .show(ctx, |ui| {
                    ui.colored_label(Color32::LIGHT_RED, &report.summary);
                    if let Some(path) = &report.path {
                        ui.horizontal(|ui| {
                            ui.label(
                                tr("rust.ui.logs.path").replace("%0", &path.display().to_string()),
                            );
                            if ui.small_button(tr("rust.ui.logs.open-file")).clicked() {
                                open_path(path);
                            }
                        });
                    }
                    ui.separator();
                    if !report.suggestions.is_empty() {
                        ui.heading(tr("rust.ui.crash.matches"));
                        for suggestion in &report.suggestions {
                            ui.group(|ui| {
                                ui.strong(&suggestion.title);
                                if !suggestion.body.is_empty() {
                                    ui.label(&suggestion.body);
                                }
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
            }
            if request_crash_help {
                self.show_diagnostic_consent = true;
                self.diagnostic_send_enabled = true;
                self.diagnostic_error = None;
            }
            if open_crash_logs {
                if let Some(path) = report.path.as_ref().filter(|path| path.is_file()) {
                    open_path(path);
                } else {
                    self.show_log_window = true;
                }
            }
            if let Some(action) = selected_action {
                match action {
                    crash::CrashActionKind::OpenUrl(url) => open_url(&url),
                    crash::CrashActionKind::OpenFolder(folder) => {
                        open_crash_folder(&report.game_directory, &folder, &mut self.status);
                    }
                    crash::CrashActionKind::OpenLogs => {
                        self.selected_log_path = report.path.clone();
                        self.show_log_window = true;
                    }
                    crash::CrashActionKind::OpenSettings => {
                        self.settings_page = Some("Launcher & game preferences");
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

        let legacy_font_revert = self
            .settings
            .legacy_font_size_revert
            .filter(|previous| (previous - self.settings.launcher_font_size).abs() > 0.01);
        if let Some(previous_size) = legacy_font_revert {
            let mut revert = false;
            let mut dismiss = false;
            egui::Area::new(Id::unique("legacy_font_revert"))
                .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 8.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
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
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
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

        if self.show_first_run_notice {
            let acknowledged = egui::Modal::new(Id::unique("first_run_notice")).show(ctx, |ui| {
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
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_size([540.0, 270.0])
                .show(ctx, |ui| {
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
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
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
                open_url("https://apps.microsoft.com/detail/9nqpsl29bfff");
                open = false;
            }
            if open_support {
                self.show_support_window = true;
                open = false;
            }
            self.show_arm_compat_notice = open;
        }
    }
}

fn spawn_catalog_fetch(
    repositories: Vec<String>,
) -> Receiver<Result<MojangVersionManifest, String>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| tr("rust.error.runtime.catalog").replace("%0", &error.to_string()))
            .and_then(|runtime| {
                runtime
                    .block_on(catalog::fetch_manifest(&repositories))
                    .map_err(|error| error)
            });
        let _ = sender.send(result);
    });
    receiver
}

fn spawn_connectivity_check() -> Receiver<connectivity::Report> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let report = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime.block_on(connectivity::check()),
            Err(_) => connectivity::unavailable_report(),
        };
        let _ = sender.send(report);
    });
    receiver
}

fn spawn_update_check(
    include_prereleases: bool,
) -> Receiver<Result<github_update::CheckResult, String>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| {
                tr("rust.error.runtime.update-check").replace("%0", &error.to_string())
            })
            .and_then(|runtime| {
                runtime.block_on(github_update::check_for_update(include_prereleases))
            });
        let _ = sender.send(result);
    });
    receiver
}

fn spawn_update_install(release: github_update::Release) -> Receiver<Result<(), String>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| {
                tr("rust.error.runtime.update-install").replace("%0", &error.to_string())
            })
            .and_then(|runtime| runtime.block_on(github_update::download_and_handoff(release)));
        let _ = sender.send(result);
    });
    receiver
}

fn spawn_bootstrap_fetch() -> Receiver<Result<bootstrap::Configuration, String>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| tr("rust.error.runtime.bootstrap").replace("%0", &error.to_string()))
            .and_then(|runtime| runtime.block_on(bootstrap::fetch()));
        let _ = sender.send(result);
    });
    receiver
}

fn spawn_crash_signature_fetch(
    repositories: Vec<String>,
    launcher_log: Arc<Mutex<VecDeque<LogRecord>>>,
) {
    thread::spawn(move || {
        let result = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| {
                tr("rust.error.runtime.crash-signatures").replace("%0", &error.to_string())
            })
            .and_then(|runtime| {
                runtime.block_on(crash::refresh_external_signatures(&repositories))
            });
        match result {
            Ok(Some(revision)) => record_launcher_log_event(
                &launcher_log,
                LogLevel::Info,
                LogTarget::Crash,
                &tr("rust.log.crash.signatures.updated").replace("%0", &revision.to_string()),
            ),
            Ok(None) => record_launcher_log_event(
                &launcher_log,
                LogLevel::Debug,
                LogTarget::Crash,
                &tr("rust.log.crash.signatures.current"),
            ),
            Err(error) => record_launcher_log_event(
                &launcher_log,
                LogLevel::Debug,
                LogTarget::Crash,
                &tr("rust.log.crash.signatures.failed").replace("%0", &error),
            ),
        }
    });
}

fn spawn_diagnostic_upload(report: String) -> Receiver<Result<String, String>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| tr("rust.error.runtime.diagnostics").replace("%0", &error.to_string()))
            .and_then(|runtime| runtime.block_on(support::upload_diagnostic_report(report)));
        let _ = sender.send(result);
    });
    receiver
}

fn spawn_notice_fetch(client_id: String) -> Receiver<Result<Vec<notices::Notice>, String>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| tr("rust.error.runtime.notices").replace("%0", &error.to_string()))
            .and_then(|runtime| runtime.block_on(notices::fetch(&client_id)));
        let _ = sender.send(result);
    });
    receiver
}

fn unix_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

async fn wait_for_cancel(cancel_requested: Arc<AtomicBool>) {
    while !cancel_requested.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn make_launch_options(
    version: String,
    username: String,
    path: PathBuf,
    force_update: bool,
    preferences: SavedSettings,
) -> LaunchOptions {
    make_launch_options_with_authenticator(
        version,
        offline_authenticator(username),
        path,
        force_update,
        preferences,
        None,
    )
}

fn make_launch_options_with_authenticator(
    version: String,
    authenticator: Authenticator,
    path: PathBuf,
    force_update: bool,
    preferences: SavedSettings,
    release_time: Option<&str>,
) -> LaunchOptions {
    let instance = instance_for_version(&version, &preferences);
    let memory = memory_config(&preferences, &version, release_time);
    LaunchOptions {
        path,
        version,
        authenticator,
        timeout_secs: 30,
        download_concurrency: 8,
        verify_concurrency: 4,
        memory,
        java: JavaOptions {
            path: if preferences.java_path.trim().is_empty() {
                None
            } else {
                Some(PathBuf::from(preferences.java_path.trim()))
            },
            runtime_dir: if preferences.java_runtime_directory.trim().is_empty() {
                None
            } else {
                Some(PathBuf::from(
                    preferences.java_runtime_directory.trim().to_owned(),
                ))
            },
            optimized_args_type: preferences.optimized_java_args.clone(),
            use_current_trust_store: preferences.use_current_trust_store,
            wrapper_command: if preferences.wrapper_command.trim().is_empty() {
                None
            } else {
                Some(preferences.wrapper_command.clone())
            },
            ..JavaOptions::default()
        },
        loader: loader_config(&preferences),
        screen: ScreenConfig {
            width: Some(preferences.screen_width.clamp(320, 7680)),
            height: Some(preferences.screen_height.clamp(240, 4320)),
            fullscreen: preferences.fullscreen,
        },
        verify: force_update,
        game_args: split_arguments(&preferences.game_args),
        jvm_args: split_arguments(&preferences.jvm_args),
        instance,
        url: None,
        mcp: None,
        library_replace_type: None,
        library_replace_types: vec![],
        intel_enabled_mac: false,
        bypass_offline: false,
        skip_bundle_check: !force_update,
        force_ipv4: false,
        dns: None,
    }
}

fn append_legacy_experiment_arguments(
    options: &mut LaunchOptions,
    experiments: &HashSet<String>,
    java_major: Option<u32>,
) -> Option<String> {
    if options.java.optimized_args_type != "none" {
        if experiments.contains("max_xms") {
            options.jvm_args.push(format!("-Xms{}", options.memory.max));
        }
        if experiments.contains("more_gc_threads") {
            let threads = std::thread::available_parallelism()
                .map(|count| count.get() / 2)
                .unwrap_or(1)
                .max(1);
            options
                .jvm_args
                .push(format!("-XX:ConcGCThreads={threads}"));
        }
        if experiments.contains("tenuring") {
            options
                .jvm_args
                .push("-XX:MaxTenuringThreshold=1".to_owned());
            options.jvm_args.push("-XX:SurvivorRatio=32".to_owned());
        }
    }
    if experiments.contains("zgc_generational") && options.java.optimized_args_type == "zgc" {
        match java_major {
            Some(21..=22) => {
                options.jvm_args.push("-XX:+ZGenerational".to_owned());
            }
            Some(major) => {
                return Some(
                    tr("rust.status.experiment.zgc-generational-unavailable")
                        .replace("%0", &major.to_string()),
                );
            }
            None => {
                return Some(
                    tr("rust.status.experiment.zgc-generational-unavailable")
                        .replace("%0", "unknown"),
                );
            }
        }
    }
    None
}

fn append_promoted_server(
    servers: &mut Vec<SavedServer>,
    version: &str,
    family: Option<&str>,
    label: &str,
    address: &str,
    families: &[String],
) {
    if !families.is_empty()
        && !families
            .iter()
            .any(|candidate| candidate == version || Some(candidate.as_str()) == family)
    {
        return;
    }
    let name = label.strip_prefix("Join ").unwrap_or(label).trim();
    if !valid_server_name(name) || parse_server_address(address).is_none() {
        return;
    }
    if servers.iter().any(|server| {
        parse_server_address(&server.address)
            .zip(parse_server_address(address))
            .is_some_and(|((_, _, left), (_, _, right))| left.eq_ignore_ascii_case(&right))
    }) {
        return;
    }
    servers.push(SavedServer {
        name: if name.starts_with("§r") {
            name.to_owned()
        } else {
            format!("§r{name}")
        },
        address: address.to_owned(),
        ..Default::default()
    });
}

fn legacy_promoted_version_family(version: &str) -> Option<String> {
    let bytes = version.as_bytes();
    let mut cursor = 0;
    while bytes.get(cursor).is_some_and(u8::is_ascii_lowercase) {
        cursor += 1;
    }
    let major_start = cursor;
    while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    if cursor == major_start || bytes.get(cursor) != Some(&b'.') {
        return None;
    }
    cursor += 1;
    let minor_start = cursor;
    while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    if cursor == minor_start {
        return None;
    }
    let family = &version[..cursor];
    (family != "26.1").then(|| family.to_owned())
}

fn valid_server_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty() && name.chars().count() <= 64 && !name.chars().any(char::is_control)
}

fn parse_server_address(address: &str) -> Option<(String, u16, String)> {
    let address = address.trim();
    if address.is_empty()
        || address.len() > 255
        || address.chars().any(|character| {
            character.is_whitespace() || character.is_control() || matches!(character, '/' | '\\')
        })
    {
        return None;
    }

    let (host, port) = if let Some(ipv6) = address.strip_prefix('[') {
        let (host, suffix) = ipv6.split_once(']')?;
        if host.is_empty() || !host.contains(':') {
            return None;
        }
        let port = if suffix.is_empty() {
            25565
        } else {
            suffix.strip_prefix(':')?.parse::<u16>().ok()?
        };
        if port == 0 {
            return None;
        }
        (format!("[{host}]"), port)
    } else if address.matches(':').count() == 1 {
        let (host, port) = address.rsplit_once(':')?;
        let port = port.parse::<u16>().ok()?;
        if host.is_empty() || port == 0 {
            return None;
        }
        (host.to_owned(), port)
    } else if address.contains(':') {
        (format!("[{address}]"), 25565)
    } else {
        (address.to_owned(), 25565)
    };

    let raw_host = host.trim_matches(['[', ']']);
    if raw_host.is_empty()
        || !raw_host.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_' | ':')
        })
    {
        return None;
    }
    let full_address = if port == 25565 {
        host.clone()
    } else {
        format!("{host}:{port}")
    };
    Some((host, port, full_address))
}

fn selected_version_supports_quick_play(version: &str, game_root: &std::path::Path) -> bool {
    let version_json = game_root
        .join("versions")
        .join(version)
        .join(format!("{version}.json"));
    if let Ok(bytes) = fs::read(version_json)
        && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
    {
        let supported = value
            .get("arguments")
            .and_then(|arguments| arguments.get("game"))
            .and_then(serde_json::Value::as_array)
            .is_some_and(|arguments| {
                arguments.iter().any(|argument| {
                    argument
                        .get("rules")
                        .and_then(serde_json::Value::as_array)
                        .is_some_and(|rules| {
                            rules.iter().any(|rule| {
                                rule.get("features")
                                    .and_then(|features| features.get("is_quick_play_multiplayer"))
                                    .and_then(serde_json::Value::as_bool)
                                    == Some(true)
                            })
                        })
                })
            });
        return supported;
    }

    let mut parts = version.split('.');
    let major = parts.next().and_then(|value| value.parse::<u32>().ok());
    let minor = parts.next().and_then(|value| value.parse::<u32>().ok());
    let patch = parts
        .next()
        .and_then(|value| {
            value
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse::<u32>()
                .ok()
        })
        .unwrap_or_default();
    match (major, minor) {
        (Some(major), _) if major > 1 => true,
        (Some(1), Some(minor)) if minor > 20 => true,
        (Some(1), Some(20)) => patch >= 5,
        _ => false,
    }
}

fn append_server_launch_arguments(
    options: &mut LaunchOptions,
    address: &str,
    supports_quick_play: bool,
) {
    let Some((host, port, full_address)) = parse_server_address(address) else {
        return;
    };
    if supports_quick_play {
        options
            .game_args
            .extend(["--quickPlayMultiplayer".to_owned(), full_address]);
    } else {
        options.game_args.extend(["--server".to_owned(), host]);
        if port != 25565 {
            options
                .game_args
                .extend(["--port".to_owned(), port.to_string()]);
        }
    }
}

fn read_minecraft_server_list(path: &std::path::Path) -> Result<Vec<SavedServer>, String> {
    const MAX_COMPRESSED_BYTES: u64 = 8 * 1024 * 1024;
    const MAX_NBT_BYTES: u64 = 16 * 1024 * 1024;
    let compressed = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(tr("rust.error.servers.read")
                .replace("%0", &path.display().to_string())
                .replace("%1", &error.to_string()));
        }
    };
    if compressed.len() as u64 > MAX_COMPRESSED_BYTES {
        return Err(tr("rust.error.servers.compressed-large").to_owned());
    }
    let decoder = flate2::read::GzDecoder::new(compressed.as_slice());
    let mut nbt_bytes = Vec::new();
    decoder
        .take(MAX_NBT_BYTES + 1)
        .read_to_end(&mut nbt_bytes)
        .map_err(|error| {
            tr("rust.error.servers.decompress")
                .replace("%0", &path.display().to_string())
                .replace("%1", &error.to_string())
        })?;
    if nbt_bytes.len() as u64 > MAX_NBT_BYTES {
        return Err(tr("rust.error.servers.decompressed-large").to_owned());
    }
    let file: MinecraftServerList = fastnbt::from_bytes(&nbt_bytes).map_err(|error| {
        tr("rust.error.servers.parse")
            .replace("%0", &path.display().to_string())
            .replace("%1", &error.to_string())
    })?;
    Ok(file
        .servers
        .into_iter()
        .filter(|server| {
            valid_server_name(&server.name) && parse_server_address(&server.address).is_some()
        })
        .map(|server| SavedServer {
            name: server.name,
            address: server.address,
            hide_address: server.hide_address,
            accept_textures: server.accept_textures,
        })
        .collect())
}

fn write_minecraft_server_list(
    path: &std::path::Path,
    servers: &[SavedServer],
) -> Result<(), String> {
    let file = MinecraftServerList {
        servers: servers
            .iter()
            .map(|server| MinecraftServerRecord {
                name: server.name.clone(),
                address: server.address.clone(),
                hide_address: server.hide_address,
                accept_textures: server.accept_textures,
            })
            .collect(),
    };
    let nbt_bytes = fastnbt::to_bytes(&file)
        .map_err(|error| tr("rust.error.servers.encode").replace("%0", &error.to_string()))?;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(&nbt_bytes)
        .map_err(|error| tr("rust.error.servers.compress").replace("%0", &error.to_string()))?;
    let compressed = encoder.finish().map_err(|error| {
        tr("rust.error.servers.compress-finish").replace("%0", &error.to_string())
    })?;

    let parent = path
        .parent()
        .ok_or_else(|| tr("rust.error.servers.parent-missing").to_owned())?;
    fs::create_dir_all(parent).map_err(|error| {
        tr("rust.error.servers.directory")
            .replace("%0", &parent.display().to_string())
            .replace("%1", &error.to_string())
    })?;
    let temporary_path = path_with_suffix(path, ".tmp");
    let backup_path = path_with_suffix(path, ".bak");
    fs::write(&temporary_path, compressed).map_err(|error| {
        tr("rust.error.servers.write-temp")
            .replace("%0", &temporary_path.display().to_string())
            .replace("%1", &error.to_string())
    })?;

    let had_original = path.is_file();
    if had_original {
        if let Err(error) = fs::copy(path, &backup_path) {
            let _ = fs::remove_file(&temporary_path);
            return Err(tr("rust.error.servers.backup")
                .replace("%0", &path.display().to_string())
                .replace("%1", &error.to_string()));
        }
        if let Err(error) = fs::remove_file(path) {
            let _ = fs::remove_file(&temporary_path);
            return Err(tr("rust.error.servers.replace")
                .replace("%0", &path.display().to_string())
                .replace("%1", &error.to_string()));
        }
    }
    if let Err(error) = fs::rename(&temporary_path, path) {
        if had_original {
            let _ = fs::copy(&backup_path, path);
        }
        let _ = fs::remove_file(&temporary_path);
        return Err(tr("rust.error.servers.install")
            .replace("%0", &path.display().to_string())
            .replace("%1", &error.to_string()));
    }
    Ok(())
}

fn prepare_minecraft_server_list(
    path: &std::path::Path,
    quick_play_server: Option<&SavedServer>,
    promoted_servers: &[SavedServer],
    add_promoted_servers: bool,
) -> Result<(), String> {
    let mut existing = read_minecraft_server_list(path).unwrap_or_default();
    // The Java launcher tags its generated entries with the reset formatting
    // code so stale promoted records can be removed before rebuilding the list.
    existing.retain(|server| !server.name.starts_with("§r"));

    let mut merged = Vec::new();
    let mut seen_addresses = HashSet::new();
    let mut add_once = |server: SavedServer| {
        if let Some((_, _, normalized)) = parse_server_address(&server.address) {
            if seen_addresses.insert(normalized.to_ascii_lowercase()) {
                merged.push(server);
            }
        }
    };

    if let Some(server) = quick_play_server {
        add_once(server.clone());
    }
    if add_promoted_servers {
        for promoted in promoted_servers {
            let Some((_, _, address)) = parse_server_address(&promoted.address) else {
                continue;
            };
            if let Some(index) = existing.iter().position(|saved| {
                parse_server_address(&saved.address).is_some_and(|(_, _, saved_address)| {
                    saved_address.eq_ignore_ascii_case(&address)
                })
            }) {
                add_once(existing.remove(index));
            } else {
                add_once(promoted.clone());
            }
        }
    }
    for server in existing {
        add_once(server);
    }
    write_minecraft_server_list(path, &merged)
}

fn path_with_suffix(path: &std::path::Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn instance_for_version(version: &str, preferences: &SavedSettings) -> Option<String> {
    let loader = match preferences.loader_type.as_str() {
        "forge" | "neoforge" | "fabric" | "legacyfabric" | "quilt" => {
            preferences.loader_type.as_str()
        }
        _ => return None,
    };
    let version = safe_path_component(version, 48);
    let build = safe_path_component(&preferences.loader_build, 48);
    let build = if build.is_empty() {
        "latest".to_owned()
    } else {
        build
    };
    Some(format!("{version}-{loader}-{build}"))
}

fn safe_path_component(value: &str, max_chars: usize) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || "._+-".contains(*character))
        .take(max_chars)
        .collect()
}

fn offline_authenticator(username: String) -> Authenticator {
    offline_authenticator_with_uuid(username, None)
}

fn offline_authenticator_with_uuid(
    username: String,
    preserved_uuid: Option<&str>,
) -> Authenticator {
    Authenticator {
        access_token: "offline".to_owned(),
        name: username.clone(),
        uuid: preserved_uuid
            .and_then(|value| uuid::Uuid::parse_str(value).ok())
            .map(|value| value.to_string())
            .unwrap_or_else(|| offline_uuid(&username)),
        xbox_account: None,
        user_properties: None,
        client_id: None,
        client_token: None,
    }
}

fn offline_account_uuid_for(settings: &SavedSettings, username: &str) -> Option<String> {
    settings
        .offline_account_uuids
        .iter()
        .find(|(saved, _)| saved.eq_ignore_ascii_case(username))
        .map(|(_, uuid)| uuid.clone())
}

fn has_licensed_minecraft_account(settings: &SavedSettings) -> bool {
    !settings.microsoft_accounts.is_empty()
        || settings.migrated_mojang_accounts.iter().any(|legacy_name| {
            settings
                .accounts
                .iter()
                .any(|saved| saved.eq_ignore_ascii_case(legacy_name))
        })
}

fn offline_ely_skins_for(settings: &SavedSettings, username: &str) -> bool {
    settings
        .offline_ely_skins
        .iter()
        .find(|(saved, _)| saved.eq_ignore_ascii_case(username))
        .map(|(_, enabled)| *enabled)
        .unwrap_or(true)
}

async fn authenticate_microsoft_account(
    account: &MicrosoftAccountProfile,
) -> Result<Authenticator, String> {
    let refresh_token = vault::load_refresh_token(&account.storage_key())
        .map_err(|error| {
            tr("rust.status.account.microsoft.credentials-read").replace("%0", &error.to_string())
        })?
        .ok_or_else(|| tr("rust.status.account.microsoft.credentials-missing").to_owned())?;
    let authenticated = auth::refresh_sign_in(&refresh_token).await?;
    vault::store_refresh_token(&account.storage_key(), &authenticated.refresh_token).map_err(
        |error| {
            tr("rust.status.account.microsoft.credentials-save").replace("%0", &error.to_string())
        },
    )?;
    Ok(authenticated.authenticator)
}

async fn authenticate_ely_legacy_account(
    account: &ElyLegacyAccountProfile,
) -> Result<Authenticator, String> {
    let stored = vault::load_refresh_token(&account.storage_key())
        .map_err(|error| {
            tr("rust.status.account.ely.credentials-read").replace("%0", &error.to_string())
        })?
        .ok_or_else(|| tr("rust.status.account.ely.credentials-missing").to_owned())?;
    let credentials: ely_legacy::Credentials = serde_json::from_str(&stored).map_err(|error| {
        tr("rust.status.account.ely.credentials-invalid").replace("%0", &error.to_string())
    })?;
    let (profile, refreshed) = ely_legacy::refresh(&account.username, &credentials).await?;
    let serialized = serde_json::to_string(&refreshed).map_err(|error| {
        tr("rust.status.account.ely.credentials-encode").replace("%0", &error.to_string())
    })?;
    vault::store_refresh_token(&account.storage_key(), &serialized).map_err(|error| {
        tr("rust.status.account.ely.credentials-save").replace("%0", &error.to_string())
    })?;

    // LegacyLauncher's ElyLegacyUser exposes Ely's display name as its auth XUID and
    // reports auth type "msa". The core derives that type from xbox_account presence.
    Ok(Authenticator {
        access_token: refreshed.access_token,
        name: profile.display_name.clone(),
        uuid: profile.uuid,
        xbox_account: Some(XboxAccount {
            xuid: profile.display_name,
        }),
        user_properties: None,
        client_id: None,
        client_token: Some(refreshed.client_token),
    })
}

async fn authenticate_ely_account(account: &ElyAccountProfile) -> Result<Authenticator, String> {
    let stored = vault::load_refresh_token(&account.storage_key())
        .map_err(|error| {
            tr("rust.status.account.ely.credentials-read").replace("%0", &error.to_string())
        })?
        .ok_or_else(|| tr("rust.status.account.ely.credentials-missing").to_owned())?;
    let credentials: ely::Credentials = serde_json::from_str(&stored).map_err(|error| {
        tr("rust.status.account.ely.credentials-invalid").replace("%0", &error.to_string())
    })?;
    let (profile, refreshed) = ely::refresh(&credentials).await?;
    let serialized = serde_json::to_string(&refreshed).map_err(|error| {
        tr("rust.status.account.ely.credentials-encode").replace("%0", &error.to_string())
    })?;
    vault::store_refresh_token(&account.storage_key(), &serialized).map_err(|error| {
        tr("rust.status.account.ely.credentials-save").replace("%0", &error.to_string())
    })?;
    Ok(ely::authenticator(profile, refreshed))
}

fn launch_event_message(event: LaunchEvent) -> Option<String> {
    match event {
        LaunchEvent::Progress {
            downloaded,
            total,
            kind,
        } => {
            let percent = if total == 0 {
                0
            } else {
                downloaded.saturating_mul(100) / total
            };
            Some(
                tr("rust.status.download.progress")
                    .replace("%0", &kind)
                    .replace("%1", &percent.to_string()),
            )
        }
        LaunchEvent::Check {
            current,
            total,
            kind,
        } => Some(
            tr("rust.status.download.check")
                .replace("%0", &kind)
                .replace("%1", &current.to_string())
                .replace("%2", &total.to_string()),
        ),
        LaunchEvent::GameDownloadFinished => Some(tr("rust.status.download.ready").to_owned()),
        LaunchEvent::Extract(name) => Some(tr("rust.status.download.extract").replace("%0", &name)),
        LaunchEvent::Patch(message) => Some(message),
        LaunchEvent::Close(code) => {
            Some(tr("rust.status.download.game-closed").replace("%0", &code.to_string()))
        }
        LaunchEvent::Error(message) => {
            Some(tr("rust.status.download.warning").replace("%0", &message))
        }
        LaunchEvent::Speed(bytes_per_second) => Some(
            tr("rust.status.download.speed").replace("%0", &format_data_rate(bytes_per_second)),
        ),
        LaunchEvent::Estimated(seconds) => {
            Some(tr("rust.status.download.eta").replace("%0", &format_duration(seconds)))
        }
        LaunchEvent::Command(_) => None,
        LaunchEvent::Data(_) => None,
    }
}

fn launch_event_message_or_record(
    buffer: &Arc<Mutex<VecDeque<String>>>,
    launcher_log: &Arc<Mutex<VecDeque<LogRecord>>>,
    event: LaunchEvent,
    show_full_game_command: bool,
) -> Option<LaunchUiEvent> {
    match event {
        LaunchEvent::Command(line) if show_full_game_command => {
            record_launcher_log_event(
                launcher_log,
                LogLevel::Debug,
                LogTarget::MinecraftCommand,
                &line,
            );
            record_game_log_line(buffer, line);
            None
        }
        LaunchEvent::Command(_) => None,
        LaunchEvent::Data(line) => {
            record_launcher_log_event(launcher_log, LogLevel::Trace, LogTarget::Minecraft, &line);
            record_game_log_line(buffer, line);
            None
        }
        event => launch_event_ui_message(event).map(LaunchUiEvent::Message),
    }
}

enum MalwareScanOutcome {
    Clean,
    Skipped,
    Cancelled,
    Infected(malware::MalwareDetection),
}

async fn scan_mods_before_launch(
    mods_directory: &Path,
    cancel_requested: Arc<AtomicBool>,
    sender: &mpsc::Sender<LaunchUiEvent>,
) -> MalwareScanOutcome {
    send_launch_message(
        sender,
        LogLevel::Info,
        LogTarget::Mods,
        tr("launcher.step.jarscanner"),
    );

    let scanner_cancel = Arc::new(AtomicBool::new(false));
    let scan_directory = mods_directory.to_path_buf();
    let worker_cancel = Arc::clone(&scanner_cancel);
    let mut scan_task =
        tokio::task::spawn_blocking(move || malware::scan_mods(&scan_directory, &worker_cancel));
    let prompt_delay = tokio::time::sleep(Duration::from_secs(60));
    tokio::pin!(prompt_delay);

    loop {
        tokio::select! {
            result = &mut scan_task => {
                return finish_malware_scan(result, sender);
            }
            _ = wait_for_cancel(Arc::clone(&cancel_requested)) => {
                scanner_cancel.store(true, Ordering::Release);
                let _ = scan_task.await;
                return MalwareScanOutcome::Cancelled;
            }
            _ = &mut prompt_delay => {
                let (reply, answer) = tokio::sync::oneshot::channel();
                if sender.send(LaunchUiEvent::LongMalwareScanConfirmation(reply)).is_err() {
                    scanner_cancel.store(true, Ordering::Release);
                    let _ = scan_task.await;
                    return MalwareScanOutcome::Cancelled;
                }
                let continue_scanning = tokio::select! {
                    answer = answer => answer.unwrap_or(false),
                    _ = wait_for_cancel(Arc::clone(&cancel_requested)) => {
                        scanner_cancel.store(true, Ordering::Release);
                        let _ = scan_task.await;
                        return MalwareScanOutcome::Cancelled;
                    }
                };
                if !continue_scanning {
                    scanner_cancel.store(true, Ordering::Release);
                    let _ = scan_task.await;
                    send_launch_message(
                        sender,
                        LogLevel::Info,
                        LogTarget::Mods,
                        tr("rust.status.malware.skipped"),
                    );
                    return MalwareScanOutcome::Skipped;
                }
                prompt_delay.as_mut().reset(
                    tokio::time::Instant::now() + Duration::from_secs(10 * 60),
                );
            }
        }
    }
}

fn finish_malware_scan(
    result: Result<std::io::Result<malware::ScanResult>, tokio::task::JoinError>,
    sender: &mpsc::Sender<LaunchUiEvent>,
) -> MalwareScanOutcome {
    let report = match result {
        Ok(Ok(report)) => report,
        Ok(Err(error)) => {
            send_launch_message(
                sender,
                LogLevel::Warn,
                LogTarget::Mods,
                tr("rust.status.malware.scan-error").replace("%0", &error.to_string()),
            );
            return MalwareScanOutcome::Skipped;
        }
        Err(error) => {
            send_launch_message(
                sender,
                LogLevel::Warn,
                LogTarget::Mods,
                tr("rust.status.malware.scan-error").replace("%0", &error.to_string()),
            );
            return MalwareScanOutcome::Skipped;
        }
    };

    if report.cancelled {
        return MalwareScanOutcome::Cancelled;
    }
    if report.unreadable_archives > 0 {
        send_launch_message(
            sender,
            LogLevel::Warn,
            LogTarget::Mods,
            tr("rust.status.malware.archive-errors")
                .replace("%0", &report.unreadable_archives.to_string()),
        );
    }
    if report.malformed_classes > 0 {
        send_launch_message(
            sender,
            LogLevel::Warn,
            LogTarget::Mods,
            tr("rust.status.malware.class-errors")
                .replace("%0", &report.malformed_classes.to_string()),
        );
    }
    if report.oversized_classes > 0 {
        send_launch_message(
            sender,
            LogLevel::Warn,
            LogTarget::Mods,
            tr("rust.status.malware.large-classes")
                .replace("%0", &report.oversized_classes.to_string()),
        );
    }
    if let Some(detection) = report.detection {
        return MalwareScanOutcome::Infected(detection);
    }

    send_launch_message(
        sender,
        LogLevel::Info,
        LogTarget::Mods,
        tr("rust.status.malware.clean"),
    );
    MalwareScanOutcome::Clean
}

fn send_launch_message(
    sender: &mpsc::Sender<LaunchUiEvent>,
    level: LogLevel,
    target: LogTarget,
    message: impl Into<String>,
) {
    let _ = sender.send(LaunchUiEvent::Message(LaunchUiMessage {
        message: message.into(),
        level,
        target,
    }));
}

fn launch_event_ui_message(event: LaunchEvent) -> Option<LaunchUiMessage> {
    let (level, target) = match &event {
        LaunchEvent::Progress { .. }
        | LaunchEvent::Check { .. }
        | LaunchEvent::Speed(_)
        | LaunchEvent::Estimated(_) => (LogLevel::Debug, LogTarget::MinecraftDownload),
        LaunchEvent::GameDownloadFinished | LaunchEvent::Extract(_) => {
            (LogLevel::Info, LogTarget::MinecraftDownload)
        }
        LaunchEvent::Patch(_) => (LogLevel::Info, LogTarget::Mods),
        LaunchEvent::Close(code) if *code == 0 => (LogLevel::Info, LogTarget::Minecraft),
        LaunchEvent::Close(_) => (LogLevel::Error, LogTarget::Minecraft),
        LaunchEvent::Error(_) => (LogLevel::Error, LogTarget::MinecraftDownload),
        LaunchEvent::Command(_) | LaunchEvent::Data(_) => return None,
    };
    let message = launch_event_message(event)?;
    Some(LaunchUiMessage {
        message,
        level,
        target,
    })
}

fn launch_action_label_key(action: &str) -> &'static str {
    match action {
        "hide" => "settings.launch-action.hide",
        "exit" => "settings.launch-action.exit",
        _ => "settings.launch-action.nothing",
    }
}

fn localized_html_to_plain_text(value: &str) -> String {
    let mut text = String::with_capacity(value.len());
    let mut inside_tag = false;
    let mut tag = String::new();
    for character in value.chars() {
        if inside_tag {
            if character == '>' {
                if tag.trim_start_matches('/').starts_with("br") {
                    text.push('\n');
                }
                inside_tag = false;
                tag.clear();
            } else {
                tag.push(character);
            }
        } else if character == '<' {
            inside_tag = true;
        } else {
            text.push(character);
        }
    }
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn settings_page_title(title: &str) -> &'static str {
    tr(match title {
        "About us" => "settings.tab.about",
        "Launcher" | "Launcher & game preferences" => "rust.settings.page.preferences",
        "Manage versions" => "rust.settings.page.versions",
        "Manage accounts" => "rust.settings.page.accounts",
        "Manage servers" => "rust.settings.page.servers",
        _ => "rust.settings.page.preferences",
    })
}

fn look_and_feel_label_key(state: &str) -> &'static str {
    match state {
        "off" => "settings.laf.state.off",
        "system" => "settings.laf.state.system",
        "dark" => "settings.laf.state.dark",
        "light" => "settings.laf.state.light",
        _ => "settings.laf.state.auto",
    }
}

fn login_form_anchor(direction: &str) -> (Align2, Vec2) {
    match direction {
        "top" => (Align2::CENTER_TOP, Vec2::new(0.0, 22.0)),
        "top_right" => (Align2::RIGHT_TOP, Vec2::new(-55.0, 22.0)),
        "center_left" => (Align2::LEFT_CENTER, Vec2::new(55.0, 0.0)),
        "center" => (Align2::CENTER_CENTER, Vec2::ZERO),
        "center_right" => (Align2::RIGHT_CENTER, Vec2::new(-55.0, 0.0)),
        "bottom_left" => (Align2::LEFT_BOTTOM, Vec2::new(55.0, -22.0)),
        "bottom" => (Align2::CENTER_BOTTOM, Vec2::new(0.0, -22.0)),
        "bottom_right" => (Align2::RIGHT_BOTTOM, Vec2::new(-55.0, -22.0)),
        _ => (Align2::LEFT_TOP, Vec2::new(55.0, 22.0)),
    }
}

fn login_form_direction_label_key(direction: &str) -> &'static str {
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

fn parse_launch_progress(message: &str) -> Option<f32> {
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

fn format_data_rate(bytes_per_second: f64) -> String {
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

fn format_duration(seconds: f64) -> String {
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

fn record_game_log_line(buffer: &Arc<Mutex<VecDeque<String>>>, line: String) {
    if let Ok(mut lines) = buffer.lock() {
        if lines.len() >= 2500 {
            lines.pop_front();
        }
        lines.push_back(line);
    }
}

fn record_launcher_log_line(buffer: &Arc<Mutex<VecDeque<LogRecord>>>, message: &str) {
    record_launcher_log_event(buffer, LogLevel::Info, LogTarget::Launcher, message);
}

fn record_launcher_log_event(
    buffer: &Arc<Mutex<VecDeque<LogRecord>>>,
    level: LogLevel,
    target: LogTarget,
    message: &str,
) {
    const BUFFER_CAPACITY: usize = 1024 * 1024;
    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = now.as_secs() % 86_400;
    let timestamp = format!(
        "{:02}:{:02}:{:02}.{:03}",
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60,
        now.subsec_millis()
    );
    let redacted = redact_sensitive_log_values(message);
    let mut records_to_write = Vec::new();
    for message_line in redacted.lines().take(256) {
        let mut record = LogRecord {
            timestamp: timestamp.clone(),
            level,
            target,
            message: message_line.to_owned(),
        };
        if record.rendered_len() > 32 * 1024 {
            let fixed_length = record.rendered_len() - record.message.len();
            record.message = truncate_utf8_to_bytes(&record.message, 32 * 1024 - fixed_length);
        }
        let line = record.render();
        if let Ok(mut lines) = buffer.lock() {
            let mut current_size = lines.iter().map(LogRecord::rendered_len).sum::<usize>();
            while current_size + line.len() > BUFFER_CAPACITY {
                let Some(removed) = lines.pop_front() else {
                    break;
                };
                current_size = current_size.saturating_sub(removed.rendered_len());
            }
            lines.push_back(record.clone());
        }
        records_to_write.push(format!("{line}\n"));
    }
    for line in records_to_write {
        append_launcher_log_file(line.as_bytes());
    }
}

fn truncate_utf8_to_bytes(value: &str, limit: usize) -> String {
    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn redact_sensitive_log_values(message: &str) -> String {
    use std::sync::OnceLock;

    static KEY_VALUE: OnceLock<regex::Regex> = OnceLock::new();
    static BEARER: OnceLock<regex::Regex> = OnceLock::new();
    let key_value = KEY_VALUE.get_or_init(|| {
        regex::Regex::new(
            r#"(?i)((?:access|refresh|id|client)[_-]?(?:token|secret)|authorization|password|secret|token)(["']?\s*[:=]\s*["']?)[^&\s,;"']+"#,
        )
        .expect("static sensitive-value expression is valid")
    });
    let bearer = BEARER.get_or_init(|| {
        regex::Regex::new(r"(?i)(bearer\s+)[A-Za-z0-9._~+/-]+=*")
            .expect("static bearer-token expression is valid")
    });
    let without_bearer = bearer.replace_all(message, "$1[REDACTED]");
    key_value
        .replace_all(&without_bearer, "$1$2[REDACTED]")
        .into_owned()
}

fn append_launcher_log_file(line: &[u8]) {
    const FILE_CAPACITY: u64 = 12 * 1024 * 1024;
    let path = launcher_log_path();
    let Some(directory) = path.parent() else {
        return;
    };
    if fs::create_dir_all(directory).is_err() {
        return;
    }
    let current_size = fs::metadata(&path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if current_size.saturating_add(line.len() as u64) > FILE_CAPACITY {
        for index in (1..10).rev() {
            let source = path.with_file_name(format!("launcher.log.{index}"));
            let target = path.with_file_name(format!("launcher.log.{}", index + 1));
            if source.exists() {
                let _ = fs::remove_file(&target);
                let _ = fs::rename(source, target);
            }
        }
        let first_backup = path.with_file_name("launcher.log.1");
        let _ = fs::remove_file(&first_backup);
        let _ = fs::rename(&path, first_backup);
    }
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line);
    }
}

fn append_launcher_diagnostic(level: LogLevel, target: LogTarget, message: &str) {
    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = now.as_secs() % 86_400;
    let timestamp = format!(
        "{:02}:{:02}:{:02}.{:03}",
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60,
        now.subsec_millis()
    );
    let redacted = redact_sensitive_log_values(message);
    for line in redacted.lines().take(256) {
        let mut record = LogRecord {
            timestamp: timestamp.clone(),
            level,
            target,
            message: line.to_owned(),
        };
        if record.rendered_len() > 32 * 1024 {
            let fixed_length = record.rendered_len() - record.message.len();
            record.message = truncate_utf8_to_bytes(&record.message, 32 * 1024 - fixed_length);
        }
        append_launcher_log_file(format!("{}\n", record.render()).as_bytes());
    }
}

fn install_process_panic_logger() {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        previous_hook(panic_info);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let thread_name = std::thread::current()
                .name()
                .unwrap_or("<unnamed>")
                .to_owned();
            let payload = panic_info
                .payload()
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| {
                    panic_info
                        .payload()
                        .downcast_ref::<String>()
                        .map(String::as_str)
                })
                .unwrap_or("non-string panic payload");
            let location = panic_info
                .location()
                .map(|location| {
                    format!(
                        "{}:{}:{}",
                        location.file(),
                        location.line(),
                        location.column()
                    )
                })
                .unwrap_or_else(|| "unknown location".to_owned());
            append_launcher_diagnostic(
                LogLevel::Error,
                LogTarget::Launcher,
                &format!("Unhandled panic in thread {thread_name} at {location}: {payload}"),
            );
            append_launcher_diagnostic(
                LogLevel::Error,
                LogTarget::Launcher,
                &format!(
                    "Panic backtrace:\n{}",
                    std::backtrace::Backtrace::force_capture()
                ),
            );
        }));
    }));
}

fn collect_crash_report(
    game_dir: &std::path::Path,
    game_log: &Arc<Mutex<VecDeque<String>>>,
    launch_started: SystemTime,
    version: &str,
    loader: &str,
    exit_code: i32,
    runtime: &crash::CrashRuntimeInfo,
) -> Option<CrashReport> {
    let cutoff = launch_started
        .checked_sub(Duration::from_secs(3))
        .unwrap_or(std::time::UNIX_EPOCH);
    let report_path = newest_recent_log(&game_dir.join("crash-reports"), cutoff, "txt")
        .or_else(|| newest_recent_log(&game_dir.join("logs"), cutoff, "log"));
    let disk_log = report_path.as_ref().and_then(|path| read_log_tail(path));
    let log = disk_log.unwrap_or_else(|| {
        game_log
            .lock()
            .map(|lines| lines.iter().cloned().collect::<Vec<_>>().join("\n"))
            .unwrap_or_default()
    });
    if log.trim().is_empty() {
        return None;
    }
    let mut suggestions = crash::diagnose(&log, version, loader, exit_code, runtime);
    if suggestions.is_empty() {
        suggestions = diagnose_crash_fallback(&log)
            .into_iter()
            .map(|body| crash::CrashSuggestion {
                title: tr("rust.ui.crash.fallback-title").to_owned(),
                body,
                permit_help: true,
                actions: Vec::new(),
            })
            .collect();
    }
    let summary = if let Some(first) = suggestions.first() {
        tr("rust.crash.summary-with-suggestion").replace("%0", &first.title)
    } else {
        tr("rust.crash.summary-no-suggestion").to_owned()
    };
    Some(CrashReport {
        path: report_path,
        game_directory: game_dir.to_path_buf(),
        summary,
        suggestions,
        log,
    })
}

fn newest_recent_log(
    directory: &std::path::Path,
    cutoff: SystemTime,
    extension: &str,
) -> Option<PathBuf> {
    fs::read_dir(directory)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file()
                || !path
                    .extension()
                    .is_some_and(|value| value.to_string_lossy().eq_ignore_ascii_case(extension))
            {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            (modified >= cutoff).then_some((path, modified))
        })
        .max_by_key(|(_, modified)| *modified)
        .map(|(path, _)| path)
}

fn process_exit_code(status: &std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        return status.signal().map(|signal| 128 + signal).unwrap_or(-1);
    }
    #[cfg(not(unix))]
    {
        -1
    }
}

fn list_managed_java_runtimes(root: &std::path::Path) -> Vec<PathBuf> {
    let mut found = HashSet::new();
    let mut pending = vec![(root.to_path_buf(), 0_u8)];
    while let Some((directory, depth)) = pending.pop() {
        if depth > 6 {
            continue;
        }
        if directory
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("bin"))
        {
            for executable in ["javaw.exe", "java.exe", "java"] {
                let candidate = directory.join(executable);
                if candidate.is_file() {
                    found.insert(candidate);
                    break;
                }
            }
        }
        if let Ok(entries) = fs::read_dir(&directory) {
            for entry in entries.flatten() {
                if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                    pending.push((entry.path(), depth.saturating_add(1)));
                }
            }
        }
    }
    let mut runtimes = found.into_iter().collect::<Vec<_>>();
    runtimes.sort();
    runtimes
}

fn list_recent_logs(game_dir: &std::path::Path) -> Vec<(PathBuf, SystemTime)> {
    let mut files = [game_dir.join("logs"), game_dir.join("crash-reports")]
        .into_iter()
        .filter_map(|directory| fs::read_dir(directory).ok())
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let extension = path.extension()?.to_string_lossy();
            if !path.is_file()
                || !(extension.eq_ignore_ascii_case("log") || extension.eq_ignore_ascii_case("txt"))
            {
                return None;
            }
            Some((path, entry.metadata().ok()?.modified().ok()?))
        })
        .collect::<Vec<_>>();
    let launcher_log = launcher_log_path();
    for path in std::iter::once(launcher_log.clone())
        .chain((1..=10).map(|index| launcher_log.with_file_name(format!("launcher.log.{index}"))))
    {
        if let Ok(metadata) = fs::metadata(&path) {
            if metadata.is_file() {
                if let Ok(modified) = metadata.modified() {
                    files.push((path, modified));
                }
            }
        }
    }
    files.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
    files.truncate(100);
    files
}

fn read_log_tail(path: &std::path::Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let start = file.metadata().ok()?.len().saturating_sub(64 * 1024);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::with_capacity(64 * 1024);
    file.read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn diagnose_crash_fallback(log: &str) -> Vec<String> {
    let lower = log.to_ascii_lowercase();
    let mut suggestions = Vec::new();
    if lower.contains("outofmemoryerror")
        || lower.contains("could not reserve enough space for object heap")
    {
        suggestions.push(tr("rust.crash.fallback.out-of-memory").to_owned());
    }
    if lower.contains("unsupportedclassversionerror")
        || lower.contains("unsupported major.minor version")
    {
        suggestions.push(tr("rust.crash.fallback.java-incompatible").to_owned());
    }
    if lower.contains("could not find or load main class")
        || lower.contains("noclassdeffounderror")
        || lower.contains("classnotfoundexception")
    {
        suggestions.push(tr("rust.crash.fallback.game-file-missing").to_owned());
    }
    if lower.contains("glfw error") || lower.contains("failed to create window") {
        suggestions.push(tr("rust.crash.fallback.graphics-window").to_owned());
    }
    if lower.contains("duplicate mods") || lower.contains("duplicate mod") {
        suggestions.push(tr("rust.crash.fallback.mod-conflict").to_owned());
    }
    suggestions
}

fn icon_button(
    ui: &mut egui::Ui,
    name: &'static str,
    texture: &TextureHandle,
    tint: Option<Color32>,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(58.0, 31.0), Sense::click());
    let fill = if response.hovered() {
        Color32::from_rgb(102, 108, 110)
    } else {
        Color32::from_rgb(73, 78, 80)
    };
    ui.painter().rect_filled(rect, 2.0, fill);
    ui.painter().rect_stroke(
        rect,
        2.0,
        Stroke::new(1.0, Color32::from_white_alpha(24)),
        egui::StrokeKind::Inside,
    );
    let size = Vec2::new(22.0, 22.0);
    let icon_rect = Rect::from_center_size(rect.center(), size);
    ui.painter().image(
        texture.id(),
        icon_rect,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        tint.unwrap_or(Color32::WHITE),
    );
    response.on_hover_text(name)
}

fn load_texture(ctx: &Context, name: &str, bytes: &[u8], recolor: bool) -> TextureHandle {
    load_texture_from_bytes(ctx, name, bytes, recolor).expect("embedded launcher image is valid")
}

fn load_texture_from_path(ctx: &Context, name: &str, path: &Path) -> Result<TextureHandle, String> {
    let metadata = fs::metadata(path).map_err(|error| {
        tr("rust.status.background.path-error")
            .replace("%0", &path.display().to_string())
            .replace("%1", &error.to_string())
    })?;
    if metadata.len() > 64 * 1024 * 1024 {
        return Err(tr("rust.status.background.file-large").to_owned());
    }
    let mut reader = image::ImageReader::open(path)
        .map_err(|error| {
            tr("rust.status.background.path-error")
                .replace("%0", &path.display().to_string())
                .replace("%1", &error.to_string())
        })?
        .with_guessed_format()
        .map_err(|error| {
            tr("rust.status.background.path-error")
                .replace("%0", &path.display().to_string())
                .replace("%1", &error.to_string())
        })?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|error| {
        tr("rust.status.background.decode-error")
            .replace("%0", &path.display().to_string())
            .replace("%1", &error.to_string())
    })?;
    texture_from_decoded_image(ctx, name, decoded, false)
}

fn load_texture_from_bytes(
    ctx: &Context,
    name: &str,
    bytes: &[u8],
    recolor: bool,
) -> Result<TextureHandle, String> {
    let decoded = image::load_from_memory(bytes).map_err(|error| {
        tr("rust.status.background.decode-bytes-error").replace("%0", &error.to_string())
    })?;
    texture_from_decoded_image(ctx, name, decoded, recolor)
}

fn texture_from_decoded_image(
    ctx: &Context,
    name: &str,
    decoded: image::DynamicImage,
    recolor: bool,
) -> Result<TextureHandle, String> {
    if u64::from(decoded.width()) * u64::from(decoded.height()) > 32_000_000 {
        return Err(tr("rust.status.background.dimensions-large").to_owned());
    }
    let mut rgba = decoded.to_rgba8();
    if recolor {
        for pixel in rgba.pixels_mut() {
            if pixel[3] > 0 {
                pixel[0] = 226;
                pixel[1] = 229;
                pixel[2] = 231;
            }
        }
    }
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    );
    Ok(ctx.load_texture(name, image, TextureOptions::LINEAR))
}

fn load_optional_texture(ctx: &Context, name: &str, bytes: &[u8]) -> Option<TextureHandle> {
    if bytes.len() > 8 * 1024 * 1024 {
        return None;
    }
    let dimensions = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()?;
    if u64::from(dimensions.0).saturating_mul(u64::from(dimensions.1)) > 16_777_216 {
        return None;
    }
    let rgba = image::load_from_memory(bytes).ok()?.to_rgba8();
    let color_image = egui::ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    );
    Some(ctx.load_texture(name, color_image, TextureOptions::LINEAR))
}

fn cover_uv(image_size: Vec2, view_size: Vec2) -> Rect {
    let image_ratio = image_size.x / image_size.y;
    let view_ratio = view_size.x / view_size.y.max(1.0);
    if image_ratio > view_ratio {
        let visible = view_ratio / image_ratio;
        let offset = (1.0 - visible) * 0.5;
        Rect::from_min_max(Pos2::new(offset, 0.0), Pos2::new(1.0 - offset, 1.0))
    } else {
        let visible = image_ratio / view_ratio;
        let offset = (1.0 - visible) * 0.5;
        Rect::from_min_max(Pos2::new(0.0, offset), Pos2::new(1.0, 1.0 - offset))
    }
}

fn tr(key: &'static str) -> &'static str {
    EN_US
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .find_map(|line| {
            let (candidate, value) = line.split_once('=')?;
            (candidate.trim() == key).then_some(value.trim())
        })
        .unwrap_or(key)
}

fn tr_dynamic(key: &str) -> String {
    EN_US
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .find_map(|line| {
            let (candidate, value) = line.split_once('=')?;
            (candidate.trim() == key).then_some(value.trim())
        })
        .unwrap_or(key)
        .to_owned()
}

fn settings_path() -> PathBuf {
    let root = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    root.join("LegacyLauncher-Rust").join("config.json")
}

fn launcher_log_path() -> PathBuf {
    settings_path()
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("logs")
        .join("launcher.log")
}

fn default_legacy_config_path() -> PathBuf {
    let root = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    root.join(".tlauncher").join("legacy.properties")
}

fn default_game_directory() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join(".minecraft")
}

fn first_run_game_directory_suggestion(current_directory: &Path) -> Option<PathBuf> {
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

fn source_directory_needs_migration_warning(path: &Path) -> bool {
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

fn path_is_same_or_nested(candidate: &Path, directory: &Path) -> bool {
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

fn is_windows_arm_host() -> bool {
    if !cfg!(windows) {
        return false;
    }

    ["PROCESSOR_ARCHITEW6432", "PROCESSOR_ARCHITECTURE"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .any(|architecture| architecture.to_ascii_uppercase().starts_with("ARM"))
}

fn load_settings() -> SavedSettings {
    load_settings_from_path(&settings_path())
}

fn load_settings_from_path(path: &Path) -> SavedSettings {
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
    settings.launcher_window_width = settings.launcher_window_width.clamp(540, 7680);
    settings.launcher_window_height = settings.launcher_window_height.clamp(360, 4320);
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

fn load_external_settings_file(
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

fn import_legacy_settings(
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

fn write_external_settings_file(
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
                    format!(
                        "Could not create settings directory {}: {error}",
                        parent.display()
                    )
                })?;
            }
            let bytes = serde_json::to_vec_pretty(settings).map_err(|error| error.to_string())?;
            fs::write(path, bytes).map_err(|error| {
                format!("Could not write settings file {}: {error}", path.display())
            })
        }
        ExternalSettingsFile::LegacyProperties(path) => write_legacy_properties(path, settings),
    }
}

fn legacy_properties_from_settings(settings: &SavedSettings) -> BTreeMap<String, String> {
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

fn escape_java_property_component(value: &str, escape_all_spaces: bool) -> String {
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

fn finish_legacy_property_group(
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

fn write_legacy_properties(path: &Path, settings: &SavedSettings) -> Result<(), String> {
    let original = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(format!(
                "Could not read settings file {}: {error}",
                path.display()
            ));
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
            format!(
                "Could not create settings directory {}: {error}",
                parent.display()
            )
        })?;
    }
    fs::write(path, contents)
        .map_err(|error| format!("Could not write settings file {}: {error}", path.display()))
}

#[derive(Default)]
struct ModCleanupSummary {
    removed: Vec<String>,
    failed: Vec<(String, String)>,
}

fn remove_undesirable_mod_files(
    mods_directory: &std::path::Path,
) -> std::io::Result<ModCleanupSummary> {
    static PATTERNS: std::sync::OnceLock<regex::RegexSet> = std::sync::OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        regex::RegexSet::new([r"tl.?skin.?cape.*\.jar", r"kl.?master.*\.jar"])
            .expect("undesirable mod filename patterns are valid")
    });
    let mut summary = ModCleanupSummary::default();
    if !mods_directory.exists() {
        return Ok(summary);
    }

    for entry in fs::read_dir(mods_directory)? {
        let entry = entry?;
        if !entry.metadata()?.is_file() {
            continue;
        }
        let filename = entry.file_name().to_string_lossy().to_lowercase();
        if !patterns.is_match(&filename) {
            continue;
        }
        match fs::remove_file(entry.path()) {
            Ok(()) => summary.removed.push(filename),
            Err(error) => summary.failed.push((filename, error.to_string())),
        }
    }
    Ok(summary)
}

fn parse_legacy_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" => Some(true),
        "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

fn parse_cli_window_size(value: &str) -> Option<(u32, u32)> {
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

fn parse_command_line_overrides(arguments: &[String]) -> CommandLineOverrides {
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

fn select_command_line_account(
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

fn print_command_line_help() {
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

fn load_legacy_theme_palette(path: &str) -> Option<LegacyThemePalette> {
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

fn parse_legacy_theme_color(value: &str) -> Option<Color32> {
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

fn legacy_background_prefers_dark_assets(background: Color32) -> bool {
    background.r() <= 64 && background.g() <= 64 && background.b() <= 64
}

fn apply_legacy_theme_palette(style: &mut egui::Style, palette: &LegacyThemePalette) {
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

fn default_version_catalog_types() -> HashMap<String, bool> {
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

fn default_version_alert_types() -> HashMap<String, bool> {
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

fn refresh_version_alert_index(
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

fn version_alert_cache_path(settings: &SavedSettings) -> PathBuf {
    let configured = PathBuf::from(settings.game_directory.trim());
    let game_directory = if configured.is_absolute() {
        configured
    } else {
        default_game_directory()
    };
    game_directory.join("versions").join("versions.json")
}

fn format_version_update_alert(versions: &[CachedVersion], first_update: bool) -> String {
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

fn append_alert_version(message: &mut String, version: &CachedVersion) {
    message.push_str("– ");
    message.push_str(&version.id);
    message.push_str(" (");
    message.push_str(&version_time_ago(version.time.as_deref()));
    message.push_str(")\n");
}

fn version_time_ago(value: Option<&str>) -> String {
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

fn default_version_type_enabled(kind: &str) -> bool {
    matches!(kind, "snapshot" | "modified" | "pending")
}

fn normalize_catalog_type(kind: &str) -> String {
    kind.trim().to_ascii_lowercase().replace('_', "-")
}

fn parse_java_properties(contents: &str) -> HashMap<String, String> {
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

fn unescape_java_property(value: &str) -> String {
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

fn legacy_mojang_profile(user: &serde_json::Value) -> Option<(String, String)> {
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

fn import_legacy_accounts(
    path: &std::path::Path,
    settings: &mut SavedSettings,
) -> Result<LegacyImportSummary, String> {
    let bytes = fs::read(path).map_err(|error| {
        tr("rust.error.import.accounts-read")
            .replace("%0", &path.display().to_string())
            .replace("%1", &error.to_string())
    })?;
    let root = serde_json::from_slice::<serde_json::Value>(&bytes)
        .map_err(|error| tr("rust.error.import.accounts-json").replace("%0", &error.to_string()))?;
    let user_set = root.get("userSet").unwrap_or(&root);
    let users = user_set
        .get("list")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| tr("rust.error.import.accounts-list-missing").to_owned())?;
    let selected = user_set.get("selected").and_then(|value| {
        Some((
            value.get("type")?.as_str()?.to_ascii_lowercase(),
            value.get("username")?.as_str()?.to_owned(),
        ))
    });
    let mut summary = LegacyImportSummary::default();
    let mut imported_selected = None;

    for user in users {
        let Some(kind) = user.get("type").and_then(serde_json::Value::as_str) else {
            summary.skipped += 1;
            continue;
        };
        match kind {
            "mojang" => {
                let login_name = user
                    .get("username")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                let Some((username, player_uuid)) = legacy_mojang_profile(user) else {
                    summary.skipped += 1;
                    continue;
                };
                if !valid_player_name(&username) {
                    summary.skipped += 1;
                    continue;
                }
                let already_present = settings
                    .accounts
                    .iter()
                    .any(|saved| saved.eq_ignore_ascii_case(&username));
                if !already_present {
                    settings.accounts.push(username.clone());
                    settings
                        .offline_account_uuids
                        .entry(username.clone())
                        .or_insert(player_uuid);
                    summary.mojang_migrated += 1;
                }
                if !settings
                    .migrated_mojang_accounts
                    .iter()
                    .any(|saved| saved.eq_ignore_ascii_case(&username))
                {
                    settings.migrated_mojang_accounts.push(username.clone());
                }
                if selected
                    .as_ref()
                    .is_some_and(|(selected_kind, selected_name)| {
                        selected_kind == "mojang"
                            && (selected_name.eq_ignore_ascii_case(login_name)
                                || selected_name.eq_ignore_ascii_case(&username))
                    })
                {
                    imported_selected = Some(username);
                }
            }
            "plain" => {
                let Some(username) = user.get("username").and_then(serde_json::Value::as_str)
                else {
                    summary.skipped += 1;
                    continue;
                };
                let username = username.trim();
                if username.is_empty() {
                    summary.skipped += 1;
                    continue;
                }
                let already_present = settings
                    .accounts
                    .iter()
                    .any(|saved| saved.eq_ignore_ascii_case(username));
                if !already_present {
                    settings.accounts.push(username.to_owned());
                    summary.offline += 1;
                    let use_ely_skins = user
                        .get("elySkins")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(true);
                    settings
                        .offline_ely_skins
                        .insert(username.to_owned(), use_ely_skins);
                }
                if let Some(uuid) = user
                    .get("uuid")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|value| uuid::Uuid::parse_str(value).ok())
                {
                    let key = settings
                        .accounts
                        .iter()
                        .find(|saved| saved.eq_ignore_ascii_case(username))
                        .cloned()
                        .unwrap_or_else(|| username.to_owned());
                    settings
                        .offline_account_uuids
                        .entry(key)
                        .or_insert_with(|| uuid.to_string());
                }
                if selected
                    .as_ref()
                    .is_some_and(|(selected_kind, selected_name)| {
                        selected_kind == "plain" && selected_name.eq_ignore_ascii_case(username)
                    })
                {
                    imported_selected = Some(username.to_owned());
                }
            }
            "minecraft" => {
                let Some(username) = user.get("name").and_then(serde_json::Value::as_str) else {
                    summary.skipped += 1;
                    continue;
                };
                let Some(profile_uuid) = user
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|value| uuid::Uuid::parse_str(value).ok())
                else {
                    summary.skipped += 1;
                    continue;
                };
                let Some(refresh_token) = user
                    .pointer("/microsoft/refreshToken")
                    .and_then(serde_json::Value::as_str)
                    .filter(|token| !token.trim().is_empty())
                else {
                    summary.skipped += 1;
                    continue;
                };
                let uuid = profile_uuid.to_string();
                let profile = MicrosoftAccountProfile {
                    id: format!("microsoft:{uuid}"),
                    username: username.to_owned(),
                    uuid,
                    xuid: String::new(),
                };
                vault::store_refresh_token(&profile.storage_key(), refresh_token).map_err(
                    |error| {
                        tr("rust.error.import.microsoft-token").replace("%0", &error.to_string())
                    },
                )?;
                let exists = settings
                    .microsoft_accounts
                    .iter()
                    .any(|saved| saved.id == profile.id);
                settings
                    .microsoft_accounts
                    .retain(|saved| saved.id != profile.id);
                settings.microsoft_accounts.push(profile.clone());
                if !exists {
                    summary.microsoft += 1;
                }
                if selected
                    .as_ref()
                    .is_some_and(|(selected_kind, selected_name)| {
                        selected_kind == "minecraft" && selected_name.eq_ignore_ascii_case(username)
                    })
                {
                    imported_selected = Some(profile.id);
                }
            }
            "ely" => {
                let Some(username) = user
                    .get("username")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    summary.skipped += 1;
                    continue;
                };
                let Some(account_id) = user
                    .get("id")
                    .and_then(serde_json::Value::as_i64)
                    .filter(|value| *value >= 0)
                else {
                    summary.skipped += 1;
                    continue;
                };
                let Some(profile_uuid) = user
                    .get("uuid")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|value| uuid::Uuid::parse_str(value).ok())
                else {
                    summary.skipped += 1;
                    continue;
                };
                let display_name = user
                    .get("displayName")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or(username);
                let Some(access_token) = user
                    .get("accessToken")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                else {
                    summary.skipped += 1;
                    continue;
                };
                let refresh_token = user
                    .get("refreshToken")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .map(str::to_owned);
                let expiry_time = user.get("expiryTime").and_then(serde_json::Value::as_u64);
                let uuid = profile_uuid.to_string();
                let account = ElyAccountProfile {
                    id: format!("ely:{uuid}"),
                    account_id,
                    username: username.to_owned(),
                    display_name: display_name.to_owned(),
                    uuid,
                };
                let credentials = ely::Credentials {
                    access_token: access_token.to_owned(),
                    refresh_token,
                    expiry_time,
                };
                let serialized = serde_json::to_string(&credentials).map_err(|error| {
                    tr("rust.error.import.ely-encode").replace("%0", &error.to_string())
                })?;
                vault::store_refresh_token(&account.storage_key(), &serialized).map_err(
                    |error| tr("rust.error.import.ely-protect").replace("%0", &error.to_string()),
                )?;
                let exists = settings
                    .ely_accounts
                    .iter()
                    .any(|saved| saved.id == account.id);
                settings.ely_accounts.retain(|saved| saved.id != account.id);
                settings.ely_accounts.push(account.clone());
                if !exists {
                    summary.ely += 1;
                }
                if selected
                    .as_ref()
                    .is_some_and(|(selected_kind, selected_name)| {
                        selected_kind == "ely" && selected_name.eq_ignore_ascii_case(username)
                    })
                {
                    imported_selected = Some(account.id);
                }
            }
            "ely_legacy" => {
                let Some(username) = user
                    .get("username")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    summary.skipped += 1;
                    continue;
                };
                let Some(profile_uuid) = user
                    .get("uuid")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|value| uuid::Uuid::parse_str(value).ok())
                else {
                    summary.skipped += 1;
                    continue;
                };
                let Some(display_name) = user
                    .get("displayName")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    summary.skipped += 1;
                    continue;
                };
                let Some(client_token) = user
                    .get("clientToken")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                else {
                    summary.skipped += 1;
                    continue;
                };
                let Some(access_token) = user
                    .get("accessToken")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                else {
                    summary.skipped += 1;
                    continue;
                };
                let uuid = profile_uuid.to_string();
                let profile = ElyLegacyAccountProfile {
                    id: format!("ely_legacy:{uuid}"),
                    username: username.to_owned(),
                    display_name: display_name.to_owned(),
                    uuid,
                };
                let credentials = ely_legacy::Credentials {
                    access_token: access_token.to_owned(),
                    client_token: client_token.to_owned(),
                };
                let serialized = serde_json::to_string(&credentials).map_err(|error| {
                    tr("rust.error.import.ely-encode").replace("%0", &error.to_string())
                })?;
                vault::store_refresh_token(&profile.storage_key(), &serialized).map_err(
                    |error| tr("rust.error.import.ely-protect").replace("%0", &error.to_string()),
                )?;
                let exists = settings
                    .ely_legacy_accounts
                    .iter()
                    .any(|saved| saved.id == profile.id);
                settings
                    .ely_legacy_accounts
                    .retain(|saved| saved.id != profile.id);
                settings.ely_legacy_accounts.push(profile.clone());
                if !exists {
                    summary.ely_legacy += 1;
                }
                if selected
                    .as_ref()
                    .is_some_and(|(selected_kind, selected_name)| {
                        selected_kind == "ely_legacy"
                            && selected_name.eq_ignore_ascii_case(username)
                    })
                {
                    imported_selected = Some(profile.id);
                }
            }
            _ => summary.skipped += 1,
        }
    }

    if let Some(selected) = imported_selected {
        settings.selected_account = Some(selected);
    } else if settings.selected_account.is_none() {
        settings.selected_account = settings.accounts.first().cloned().or_else(|| {
            settings
                .microsoft_accounts
                .first()
                .map(|account| account.id.clone())
                .or_else(|| {
                    settings
                        .ely_legacy_accounts
                        .first()
                        .map(|account| account.id.clone())
                })
                .or_else(|| {
                    settings
                        .ely_accounts
                        .first()
                        .map(|account| account.id.clone())
                })
        });
    }
    Ok(summary)
}

fn valid_player_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 16
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn valid_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= 16
        && name
            .chars()
            .all(|character| (' '..'\u{7f}').contains(&character))
}

fn valid_version_id(version: &str) -> bool {
    !version.is_empty()
        && version != "."
        && version != ".."
        && version
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._+-".contains(character))
}

fn remove_installed_version(game_dir: &std::path::Path, version: &str) -> std::io::Result<()> {
    if !valid_version_id(version) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "version ID contains unsupported path characters",
        ));
    }

    let versions_dir = game_dir.join("versions");
    let target = versions_dir.join(version);
    let canonical_game_dir = game_dir.canonicalize()?;
    let canonical_root = versions_dir.canonicalize()?;
    if canonical_root.parent() != Some(canonical_game_dir.as_path()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "versions folder resolves outside the selected Minecraft folder",
        ));
    }
    let canonical_target = target.canonicalize()?;
    if canonical_target.parent() != Some(canonical_root.as_path()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "version path resolves outside the Minecraft versions folder",
        ));
    }
    fs::remove_dir_all(canonical_target)
}

fn invalidate_game_data_cache(game_dir: &std::path::Path, version: &str) -> std::io::Result<usize> {
    let mut cache_files = vec![game_dir.join("gameData.json")];
    if let Ok(instances) = fs::read_dir(game_dir.join("instances")) {
        cache_files.extend(instances.flatten().filter_map(|entry| {
            entry.file_type().ok().filter(|kind| kind.is_dir())?;
            Some(entry.path().join("gameData.json"))
        }));
    }
    let mut removed = 0;
    for cache_file in cache_files {
        let Ok(bytes) = fs::read(&cache_file) else {
            continue;
        };
        let Ok(cache) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        if cache
            .get("minecraftVersion")
            .and_then(|value| value.as_str())
            == Some(version)
        {
            fs::remove_file(cache_file)?;
            removed += 1;
        }
    }
    Ok(removed)
}

fn parse_memory_size(value: &str) -> Option<u64> {
    let value = value.trim().to_ascii_uppercase();
    let (number, multiplier) = if let Some(number) = value.strip_suffix('G') {
        (number, 1024_u64.pow(3))
    } else if let Some(number) = value.strip_suffix('M') {
        (number, 1024_u64.pow(2))
    } else if let Some(number) = value.strip_suffix('K') {
        (number, 1024_u64)
    } else {
        return None;
    };
    let number = number.trim().parse::<u64>().ok()?;
    (number > 0)
        .then(|| number.checked_mul(multiplier))
        .flatten()
}

fn default_legacy_xmx_mib(total_memory_mb: u32, is_64_bit: bool) -> u32 {
    if !is_64_bit || total_memory_mb <= 2_000 {
        total_memory_mb.min(1_024)
    } else if total_memory_mb <= 2_500 {
        1_536
    } else {
        2_048
    }
}

fn legacy_release_at_or_after(release_time: &str, threshold: &str) -> bool {
    use time::format_description::well_known::Rfc3339;

    match (
        time::OffsetDateTime::parse(release_time, &Rfc3339),
        time::OffsetDateTime::parse(threshold, &Rfc3339),
    ) {
        (Ok(release_time), Ok(threshold)) => release_time >= threshold,
        _ => false,
    }
}

fn legacy_auto_xmx_mib(
    version: &str,
    release_time: Option<&str>,
    total_memory_mb: u32,
    is_64_bit: bool,
    is_forge: bool,
) -> u32 {
    const ONE_GIB: u32 = 1_024;
    const TWO_GIB: u32 = 2_048;
    const THREE_GIB: u32 = 3_072;
    const FOUR_GIB: u32 = 4_096;

    let total_memory_mb = total_memory_mb.max(1);
    let default_requirement = || {
        if total_memory_mb < 3_500 {
            TWO_GIB
        } else if total_memory_mb < 5_500 {
            THREE_GIB
        } else {
            FOUR_GIB
        }
    };
    let requirement = if release_time
        .is_some_and(|time| legacy_release_at_or_after(time, "2021-07-13T12:54:19+00:00"))
    {
        default_requirement()
    } else if release_time
        .is_some_and(|time| legacy_release_at_or_after(time, "2018-02-14T17:34:13+00:00"))
    {
        TWO_GIB
    } else if release_time
        .is_some_and(|time| legacy_release_at_or_after(time, "2009-05-13T20:11:00+00:00"))
    {
        ONE_GIB
    } else {
        // The Java source falls back to the latest requirement when a version
        // has no usable release date or predates the first table entry.
        default_requirement()
    };

    let rounding_base = if total_memory_mb < 4_779 { 256 } else { 1_024 };
    let fraction = if total_memory_mb < 2_048 { 0.5 } else { 0.75 };
    let rounded_safe_memory = ((f64::from(total_memory_mb) * fraction / f64::from(rounding_base))
        .round() as u32)
        .saturating_mul(rounding_base);
    let safe_max = total_memory_mb.min(1_024).max(rounded_safe_memory);
    let is_forge = is_forge || version.to_ascii_lowercase().contains("forge");
    let requested = if total_memory_mb >= 6_000 && is_forge {
        requirement.saturating_mul(2).min(safe_max)
    } else {
        requirement
    };
    let actual = safe_max.min(requested);
    if is_64_bit { actual } else { actual.min(1_024) }
}

fn memory_config(
    preferences: &SavedSettings,
    version: &str,
    release_time: Option<&str>,
) -> MemoryConfig {
    if preferences.automatic_memory {
        let total_memory_mb = diagnostics::total_physical_memory_mb()
            .unwrap_or(8_192)
            .min(i32::MAX as u64) as u32;
        let max_memory_mb = legacy_auto_xmx_mib(
            version,
            release_time,
            total_memory_mb,
            usize::BITS >= 64,
            matches!(preferences.loader_type.as_str(), "forge" | "neoforge"),
        );
        let min_memory_mb = parse_memory_size(&preferences.min_memory)
            .map(|memory| memory.div_ceil(1_048_576).min(u64::from(u32::MAX)) as u32)
            .unwrap_or(1_024)
            .min(max_memory_mb);
        return MemoryConfig {
            min: format!("{min_memory_mb}M"),
            max: format!("{max_memory_mb}M"),
        };
    }

    match (
        parse_memory_size(&preferences.min_memory),
        parse_memory_size(&preferences.max_memory),
    ) {
        (Some(min), Some(max)) if min <= max => MemoryConfig {
            min: preferences.min_memory.trim().to_owned(),
            max: preferences.max_memory.trim().to_owned(),
        },
        _ => MemoryConfig::default(),
    }
}

fn loader_config(preferences: &SavedSettings) -> LoaderConfig {
    let loader_type = match preferences.loader_type.as_str() {
        "forge" => Some(LoaderType::Forge),
        "neoforge" => Some(LoaderType::NeoForge),
        "fabric" => Some(LoaderType::Fabric),
        "legacyfabric" => Some(LoaderType::LegacyFabric),
        "quilt" => Some(LoaderType::Quilt),
        _ => None,
    };
    let enable = loader_type.is_some();
    let build = if preferences.loader_build.trim().is_empty() {
        "latest".to_owned()
    } else {
        preferences.loader_build.trim().to_owned()
    };
    LoaderConfig {
        loader_type,
        build,
        enable,
        ..LoaderConfig::default()
    }
}

fn split_arguments(arguments: &str) -> Vec<String> {
    arguments.split_whitespace().map(str::to_owned).collect()
}

fn detect_java() -> Result<(PathBuf, String), String> {
    let mut candidates = Vec::new();
    if let Some(java_home) = std::env::var_os("JAVA_HOME") {
        let bin = PathBuf::from(java_home).join("bin");
        candidates.push(bin.join("javaw.exe"));
        candidates.push(bin.join("java.exe"));
    }
    if let Ok(output) = Command::new("where.exe").arg("java.exe").output() {
        if output.status.success() {
            candidates.extend(
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .map(PathBuf::from),
            );
        }
    }
    if let Ok(output) = Command::new("where.exe").arg("javaw.exe").output() {
        if output.status.success() {
            candidates.extend(
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .map(PathBuf::from),
            );
        }
    }

    let mut seen = HashSet::new();
    for candidate in candidates {
        if !candidate.is_file() || !seen.insert(candidate.clone()) {
            continue;
        }
        let output = match Command::new(&candidate).arg("-version").output() {
            Ok(output) if output.status.success() => output,
            _ => continue,
        };
        let mut version_output = String::from_utf8_lossy(&output.stderr).into_owned();
        if version_output.trim().is_empty() {
            version_output = String::from_utf8_lossy(&output.stdout).into_owned();
        }
        let version = version_output
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or(tr("rust.status.java.runtime"))
            .trim()
            .to_owned();
        return Ok((candidate, version));
    }
    Err(tr("rust.status.java.no-usable").to_owned())
}

async fn verify_java_compatibility(
    java_path: &std::path::Path,
    game_dir: &std::path::Path,
    version_id: &str,
    version_manifest: Option<&MojangVersionManifest>,
    extra_version_repositories: &[String],
) -> Result<String, String> {
    let path = java_path.to_owned();
    let (major, version_text) = tokio::task::spawn_blocking(move || inspect_java(&path))
        .await
        .map_err(|error| {
            tr("rust.status.java.inspection-error").replace("%0", &error.to_string())
        })??;
    let required_major = local_required_java_major(game_dir, version_id);
    let required_major = match required_major {
        Some(required) => Some(required),
        None => match version_manifest {
            Some(manifest) => {
                catalog::fetch_required_java_major(version_id, manifest, extra_version_repositories)
                    .await
                    .ok()
                    .flatten()
            }
            None => None,
        },
    };
    if let Some(required) = required_major {
        if major < required {
            return Err(tr("rust.status.java.too-old")
                .replace("%0", &major.to_string())
                .replace("%1", version_id)
                .replace("%2", &required.to_string()));
        }
        Ok(tr("rust.status.java.compatible")
            .replace("%0", &major.to_string())
            .replace("%1", version_id)
            .replace("%2", &version_text))
    } else {
        Ok(tr("rust.status.java.using")
            .replace("%0", &major.to_string())
            .replace("%1", &version_text))
    }
}

async fn selected_java_major_for_experiments(
    game_dir: &std::path::Path,
    version_id: &str,
    preferences: &SavedSettings,
    version_manifest: Option<&MojangVersionManifest>,
    extra_version_repositories: &[String],
) -> Option<u32> {
    if preferences.java_mode == "recommended" {
        if let Some(major) = local_required_java_major(game_dir, version_id) {
            return Some(major);
        }
        return match version_manifest {
            Some(manifest) => {
                catalog::fetch_required_java_major(version_id, manifest, extra_version_repositories)
                    .await
                    .ok()
                    .flatten()
            }
            None => None,
        };
    }

    let path = PathBuf::from(preferences.java_path.trim());
    tokio::task::spawn_blocking(move || inspect_java(&path).ok().map(|(major, _)| major))
        .await
        .ok()
        .flatten()
}

fn local_required_java_major(game_dir: &std::path::Path, version_id: &str) -> Option<u32> {
    let mut current = version_id.to_owned();
    for _ in 0..8 {
        if !valid_version_id(&current) {
            return None;
        }
        let path = game_dir
            .join("versions")
            .join(&current)
            .join(format!("{current}.json"));
        let bytes = fs::read(path).ok()?;
        let metadata = serde_json::from_slice::<serde_json::Value>(&bytes).ok()?;
        if let Some(major) = metadata
            .pointer("/javaVersion/majorVersion")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
        {
            return Some(major);
        }
        let Some(parent) = metadata
            .get("inheritsFrom")
            .and_then(serde_json::Value::as_str)
        else {
            return None;
        };
        current = parent.to_owned();
    }
    None
}

fn inspect_java(path: &std::path::Path) -> Result<(u32, String), String> {
    if !path.is_file() {
        return Err(
            tr("rust.status.java.executable-missing").replace("%0", &path.display().to_string())
        );
    }
    let probe_path = if path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("javaw.exe"))
    {
        let console_java = path.with_file_name("java.exe");
        if console_java.is_file() {
            console_java
        } else {
            path.to_owned()
        }
    } else {
        path.to_owned()
    };
    let output = Command::new(&probe_path)
        .arg("-version")
        .output()
        .map_err(|error| {
            tr("rust.status.java.executable-run")
                .replace("%0", &path.display().to_string())
                .replace("%1", &error.to_string())
        })?;
    if !output.status.success() {
        return Err(
            tr("rust.status.java.version-command-error").replace("%0", &path.display().to_string())
        );
    }
    let mut text = String::from_utf8_lossy(&output.stderr).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    let version = text
        .split('"')
        .nth(1)
        .map(str::trim)
        .or_else(|| {
            text.split_whitespace().find(|word| {
                word.chars()
                    .next()
                    .is_some_and(|character| character.is_ascii_digit())
            })
        })
        .ok_or_else(|| tr("rust.status.java.version-unreadable").to_owned())?;
    let major_text = version
        .strip_prefix("1.")
        .unwrap_or(version)
        .split(['.', '-', '+', '_'])
        .next()
        .unwrap_or_default();
    let major = major_text
        .parse::<u32>()
        .map_err(|_| tr("rust.status.java.major-unreadable").replace("%0", version))?;
    Ok((major, version.to_owned()))
}

fn open_path(path: &std::path::Path) {
    #[cfg(target_os = "windows")]
    let result = Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = Command::new("xdg-open").arg(path).spawn();
    let _ = result;
}

#[cfg(target_os = "windows")]
fn apply_gpu_preference(java_path: &str, preference: &str) -> Result<(), String> {
    let value = match preference {
        "integrated" => "GpuPreference=1;",
        "discrete" => "GpuPreference=2;",
        _ => return Ok(()),
    };
    let java_path = PathBuf::from(java_path).canonicalize().map_err(|error| {
        tr("rust.status.java.path-unavailable").replace("%0", &error.to_string())
    })?;
    let current_version = Command::new("reg.exe")
        .args([
            "query",
            r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion",
            "/v",
            "CurrentBuildNumber",
        ])
        .output()
        .map_err(|error| {
            tr("rust.status.game.gpu-windows-build-read").replace("%0", &error.to_string())
        })?;
    if !current_version.status.success() {
        return Err(tr("rust.status.game.gpu-windows-build-read-failed").to_owned());
    }
    let build = String::from_utf8_lossy(&current_version.stdout)
        .split_whitespace()
        .filter_map(|field| field.parse::<u32>().ok())
        .next_back()
        .ok_or_else(|| tr("rust.status.game.gpu-windows-build-parse").to_owned())?;
    if build < 20_190 {
        return Err(tr("rust.status.game.gpu-windows-build-old").to_owned());
    }
    let java_path = java_path.to_string_lossy().into_owned();
    let output = Command::new("reg.exe")
        .args([
            "add",
            r"HKCU\Software\Microsoft\DirectX\UserGpuPreferences",
            "/v",
            &java_path,
            "/t",
            "REG_SZ",
            "/d",
            value,
            "/f",
        ])
        .output()
        .map_err(|error| {
            tr("rust.status.game.gpu-registry-update").replace("%0", &error.to_string())
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

#[cfg(not(target_os = "windows"))]
fn apply_gpu_preference(_java_path: &str, preference: &str) -> Result<(), String> {
    if preference == "system" {
        Ok(())
    } else {
        Err(tr("rust.status.game.gpu-windows-only").to_owned())
    }
}

fn open_crash_folder(game_directory: &std::path::Path, folder: &str, status: &mut String) {
    let relative = folder.strip_prefix("./").unwrap_or(folder);
    let relative_path = std::path::Path::new(relative);
    if relative_path.is_absolute()
        || relative_path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::Prefix(_)
            )
        })
    {
        *status = tr("rust.status.crash.folder-outside-game").to_owned();
        return;
    }
    let base = match game_directory.canonicalize() {
        Ok(base) => base,
        Err(error) => {
            *status = tr("rust.status.crash.game-folder-error").replace("%0", &error.to_string());
            return;
        }
    };
    let target = base.join(relative_path);
    if relative_path == std::path::Path::new("mods") && !target.exists() {
        if let Err(error) = fs::create_dir_all(&target) {
            *status = tr("rust.status.crash.mods-create-error").replace("%0", &error.to_string());
            return;
        }
    }
    let target = match target.canonicalize() {
        Ok(target) if target.starts_with(&base) => target,
        _ => {
            *status = tr("rust.status.crash.folder-missing").to_owned();
            return;
        }
    };
    open_path(&target);
}

pub(crate) fn open_url(url: &str) {
    #[cfg(target_os = "windows")]
    let result = Command::new("explorer").arg(url).spawn();
    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = Command::new("xdg-open").arg(url).spawn();
    let _ = result;
}

fn main() -> eframe::Result {
    install_process_panic_logger();
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments.iter().any(|argument| argument == "--help") {
        print_command_line_help();
        return Ok(());
    }
    let require_minecraft_account = arguments.iter().any(|argument| {
        argument == "--requireMinecraftAccount" || argument == "--require-minecraft-account"
    });
    let command_line_overrides = parse_command_line_overrides(&arguments);
    let saved_settings = load_settings();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([
                saved_settings.launcher_window_width as f32,
                saved_settings.launcher_window_height as f32,
            ])
            .with_maximized(saved_settings.launcher_maximized)
            .with_min_inner_size([540.0, 360.0])
            .with_title("Legacy Launcher — Rust"),
        ..Default::default()
    };
    eframe::run_native(
        "Legacy Launcher — Rust",
        options,
        Box::new(move |cc| {
            Ok(Box::new(LauncherApp::new(
                cc,
                require_minecraft_account,
                command_line_overrides,
            )))
        }),
    )
}
