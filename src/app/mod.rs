use crate::{
    auth, bootstrap, catalog, connectivity, crash, diagnostics, ely, ely_legacy, github_update,
    http, malware, notices, support, vault,
};

mod account_ui;
mod accounts;
mod catalog_view;
mod constructor;
mod crash_ui;
mod first_run_ui;
mod frame_update;
mod game_paths;
mod helpers;
mod launch;
mod login_ui;
mod notices_ui;
mod preferences;
mod settings_ui;
mod ui;
mod version_catalog;
mod version_manager;

use eframe::egui::{
    self, Align2, Color32, ComboBox, Context, Id, Margin, Pos2, Rect, RichText, Sense, Stroke,
    TextureHandle, TextureOptions, Vec2,
};
pub(super) use helpers::*;
pub(crate) use helpers::{open_url, tr, tr_dynamic};
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

// Keep the composited launcher scene below transient UI, matching Java's layered pane.
pub(super) const SCENE_LAYER: egui::Order = egui::Order::Middle;
pub(super) const OVERLAY_LAYER: egui::Order = egui::Order::Foreground;
pub(super) const JAVA_LOGIN_FORM_SIZE: Vec2 = Vec2::new(285.0, 240.0);
pub(super) const JAVA_SETTINGS_PANEL_SIZE: Vec2 = Vec2::new(750.0, 690.0);
pub(super) const JAVA_LOGIN_CONTENT_WIDTH: f32 = JAVA_LOGIN_FORM_SIZE.x - 48.0;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum AccountManagerView {
    #[default]
    Overview,
    Add,
    Edit,
}

const EN_US: &str = include_str!("../../assets/lang/lang_en_US.properties");
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

#[derive(Clone)]
struct PendingVersionUpdateConfirmation {
    version: String,
    server: Option<SavedServer>,
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
    version: String,
    loader: String,
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

struct LaunchErrorDialog {
    title: String,
    message: String,
    details: Option<String>,
}

enum LaunchUiEvent {
    Message(LaunchUiMessage),
    ErrorDialog(LaunchErrorDialog),
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
            login_form_direction: "center".to_owned(),
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
    crash_bug: TextureHandle,
    crash_duke: TextureHandle,
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
    version_catalog_cancel: Option<Arc<AtomicBool>>,
    version_catalog_loading: bool,
    version_update_alert: Option<String>,
    version_alert_has_refreshed: bool,
    version_search: String,
    version_kind_filter: String,
    confirm_remove_version: Option<String>,
    version_install_force_confirmation: Option<String>,
    force_update_confirmation: Option<PendingVersionUpdateConfirmation>,
    force_update_confirmed_version: Option<String>,
    outdated_version_confirmation: Option<PendingVersionUpdateConfirmation>,
    version_update_prompt_bypassed: Option<String>,
    force_update_toggle_count: u8,
    force_update_easter_egg: bool,
    show_autologin_tip: bool,
    auto_launch_pending: bool,
    auto_launch_deadline: Option<Instant>,
    launcher_window_state_seen: bool,
    show_account_dialog: bool,
    account_success_kind: Option<&'static str>,
    show_account_help: bool,
    require_minecraft_account: bool,
    show_require_minecraft_account_notice: bool,
    new_account: String,
    new_account_allow_invalid: bool,
    new_account_ely_skins: bool,
    editing_account: Option<String>,
    account_manager_view: AccountManagerView,
    show_ely_legacy_dialog: bool,
    editing_ely_legacy_account: Option<String>,
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
    show_settings_reset_confirmation: bool,
    connectivity_events: Option<Receiver<connectivity::Report>>,
    connectivity_report: Option<connectivity::Report>,
    show_connectivity_window: bool,
    pending_promoted_server: Option<PendingPromotedServerLaunch>,
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
    launch_error_dialog: Option<LaunchErrorDialog>,
    malware_scan_confirmation: Option<tokio::sync::oneshot::Sender<bool>>,
    malware_detection: Option<malware::MalwareDetection>,
    cancel_requested: Arc<AtomicBool>,
    hidden_for_game: bool,
}

pub(crate) fn run() -> eframe::Result {
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
    let ui_scale = saved_settings.launcher_font_size / 12.0;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([
                saved_settings.launcher_window_width as f32,
                saved_settings.launcher_window_height as f32,
            ])
            .with_maximized(saved_settings.launcher_maximized)
            .with_min_inner_size([700.0 * ui_scale, 600.0 * ui_scale])
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
