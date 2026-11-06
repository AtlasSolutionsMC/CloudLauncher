use bzip2::read::BzDecoder;
use flate2::read::GzDecoder;
use regex::Regex;
use serde_json::Value;
use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant};

const SIGNATURES: &str = include_str!("../assets/crash/signature.json");
const MAX_SIGNATURE_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_SIGNATURE_REPOSITORIES: [&str; 3] = [
    "https://repo.llaun.ch/",
    "https://repo.legacylauncher.ru/",
    "https://repo.legacylauncherservices.com/",
];

static REMOTE_SIGNATURES: OnceLock<RwLock<Option<Value>>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct CrashSuggestion {
    pub title: String,
    pub body: String,
    pub permit_help: bool,
    pub actions: Vec<CrashSuggestionAction>,
}

#[derive(Debug, Clone)]
pub struct CrashSuggestionAction {
    pub label: String,
    pub kind: CrashActionKind,
}

#[derive(Debug, Clone, Default)]
pub struct CrashRuntimeInfo {
    pub java_version: Option<String>,
    pub java_is_32_bit: Option<bool>,
    pub system_is_64_bit: bool,
    pub graphics_cards: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum CrashActionKind {
    OpenUrl(String),
    OpenFolder(String),
    OpenLogs,
    OpenSettings,
    ForceUpdate,
    RecommendedJava,
    SetJvmArgument(String),
}

pub fn diagnose(
    log: &str,
    version_id: &str,
    loader: &str,
    exit_code: i32,
    runtime: &CrashRuntimeInfo,
) -> Vec<CrashSuggestion> {
    let root = current_signature_catalog();
    if !root.is_object() {
        return Vec::new();
    }
    let variables = runtime_variables(&root);
    let buttons = root.get("buttons").and_then(Value::as_array);
    let mut result = Vec::new();
    let version_context = format!("{version_id}-{loader}");

    for signature in root
        .get("signatures")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if signature
            .get("fake")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            continue;
        }

        if let Some(signature_exit_code) = signature.get("exitCode").and_then(Value::as_i64)
            && signature_exit_code != 0
            && signature_exit_code != i64::from(exit_code)
        {
            continue;
        }

        if let Some(operating_systems) = signature.get("os").and_then(Value::as_array)
            && !operating_systems.iter().any(|entry| {
                entry
                    .as_str()
                    .is_some_and(|entry| entry.eq_ignore_ascii_case(current_os_name()))
            })
        {
            continue;
        }

        if let Some(java_pattern) = signature.get("jre").and_then(Value::as_str) {
            let Some(java_version) = runtime.java_version.as_deref() else {
                continue;
            };
            let java_pattern = expand_vars(java_pattern, Some(&variables));
            let Ok(java_pattern) = Regex::new(&format!("^(?:{java_pattern})$")) else {
                continue;
            };
            if !java_pattern.is_match(java_version) {
                continue;
            }
        }

        if let Some(graphics_card_pattern) = signature.get("graphicsCard").and_then(Value::as_str) {
            let graphics_card_pattern = expand_vars(graphics_card_pattern, Some(&variables));
            let Ok(graphics_card_pattern) = Regex::new(&format!("^(?:{graphics_card_pattern})$"))
            else {
                continue;
            };
            if runtime.graphics_cards.is_empty()
                || !runtime
                    .graphics_cards
                    .iter()
                    .any(|card| graphics_card_pattern.is_match(card))
            {
                continue;
            }
        } else if signature
            .get("archIssue")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && !(runtime.java_is_32_bit == Some(true) && runtime.system_is_64_bit)
        {
            continue;
        }

        let Some(pattern) = signature.get("pattern").and_then(Value::as_str) else {
            continue;
        };
        let pattern = expand_vars(pattern, Some(&variables));
        let Ok(pattern) = Regex::new(&format!("^(?:{pattern})$")) else {
            // Java's signature database occasionally uses syntax unsupported by Rust's
            // regex engine. Ignore just that rule and retain the rest of the catalog.
            continue;
        };
        if !log.lines().any(|line| pattern.is_match(line)) {
            continue;
        }

        if let Some(version_pattern) = signature.get("version").and_then(Value::as_str) {
            let version_pattern = expand_vars(version_pattern, Some(&variables));
            let Ok(version_pattern) = Regex::new(&format!("^(?:{version_pattern})$")) else {
                continue;
            };
            if !version_pattern.is_match(version_id) && !version_pattern.is_match(&version_context)
            {
                continue;
            }
        }

        let title = localized(signature.get("title"), Some(&variables)).unwrap_or_else(|| {
            signature
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(crate::tr("rust.crash.issue.known"))
                .replace('-', " ")
        });
        let body = localized(signature.get("body"), Some(&variables)).unwrap_or_default();
        let actions = signature
            .get("buttons")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|button| resolve_button(button, buttons, Some(&variables)))
            .collect();
        result.push(CrashSuggestion {
            title,
            body: clean_html(&body),
            permit_help: signature
                .get("permitHelp")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            actions,
        });

        // The Java launcher presents one most relevant crash explanation at a time.
        if result.len() >= 3 {
            break;
        }
    }
    result
}

/// Probe the Java executable selected by the completed Minecraft installation.
/// The source launcher evaluates these values from inside the game JVM; this
/// bounded subprocess probe supplies equivalent values before showing crash help.
pub fn collect_runtime_info(java_path: &str) -> CrashRuntimeInfo {
    let root = current_signature_catalog();
    let signatures = root.get("signatures").and_then(Value::as_array);
    let needs_java = signatures.into_iter().flatten().any(|signature| {
        signature.get("jre").is_some()
            || signature
                .get("archIssue")
                .and_then(Value::as_bool)
                .unwrap_or(false)
    });
    let needs_graphics = signatures.into_iter().flatten().any(|signature| {
        signature
            .get("graphicsCard")
            .and_then(Value::as_str)
            .is_some()
    });

    let (java_version, java_is_32_bit) = if needs_java {
        probe_java(java_path)
    } else {
        (None, None)
    };
    CrashRuntimeInfo {
        java_version,
        java_is_32_bit,
        system_is_64_bit: system_is_64_bit(),
        graphics_cards: if needs_graphics {
            installed_graphics_cards()
        } else {
            Vec::new()
        },
    }
}

fn current_os_name() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "linux" => "linux",
        "macos" => "osx",
        _ => "unknown",
    }
}

fn system_is_64_bit() -> bool {
    #[cfg(target_os = "windows")]
    {
        return ["PROCESSOR_ARCHITEW6432", "PROCESSOR_ARCHITECTURE"]
            .into_iter()
            .filter_map(|name| std::env::var(name).ok())
            .any(|architecture| {
                matches!(
                    architecture.to_ascii_lowercase().as_str(),
                    "amd64" | "ia64" | "arm64"
                )
            })
            || cfg!(target_pointer_width = "64");
    }
    #[cfg(not(target_os = "windows"))]
    {
        cfg!(target_pointer_width = "64")
    }
}

fn probe_java(java_path: &str) -> (Option<String>, Option<bool>) {
    let path = std::path::Path::new(java_path);
    let mut candidates = Vec::new();
    if path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("javaw.exe"))
    {
        candidates.push(path.with_file_name("java.exe"));
    }
    candidates.push(path.to_path_buf());

    for candidate in candidates.into_iter().filter(|path| path.is_file()) {
        let mut command = Command::new(candidate);
        command
            .arg("-XshowSettings:properties")
            .arg("-version")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let Some(output) = command_output_with_timeout(command, Duration::from_secs(8)) else {
            continue;
        };
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
        let java_version =
            property_value(&text, "java.version").or_else(|| java_version_from_banner(&text));
        let java_is_32_bit = property_value(&text, "sun.arch.data.model")
            .and_then(|value| match value.as_str() {
                "32" => Some(true),
                "64" => Some(false),
                _ => None,
            })
            .or_else(|| {
                let lower = text.to_ascii_lowercase();
                if lower.contains("32-bit") {
                    Some(true)
                } else if lower.contains("64-bit") {
                    Some(false)
                } else {
                    None
                }
            });
        return (java_version, java_is_32_bit);
    }
    (None, None)
}

fn command_output_with_timeout(mut command: Command, timeout: Duration) -> Option<Output> {
    let mut child = command.spawn().ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(40));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

fn property_value(output: &str, property: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let (name, value) = line.trim().split_once('=')?;
        name.trim().eq(property).then(|| value.trim().to_owned())
    })
}

fn java_version_from_banner(output: &str) -> Option<String> {
    let marker = "version \"";
    let start = output.find(marker)? + marker.len();
    let rest = &output[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

fn installed_graphics_cards() -> Vec<String> {
    #[cfg(target_os = "windows")]
    {
        const DISPLAY_CLASS: &str =
            r"HKLM\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
        let output = Command::new("reg.exe")
            .args(["query", DISPLAY_CLASS, "/s", "/v", "DriverDesc"])
            .output();
        let Ok(output) = output else {
            return Vec::new();
        };
        let text = String::from_utf8_lossy(&output.stdout);
        return text
            .lines()
            .filter_map(|line| {
                let (_, value) = line.split_once("REG_SZ")?;
                let value = value.trim();
                (!value.is_empty()).then(|| value.to_owned())
            })
            .collect();
    }
    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}

fn runtime_variables(root: &Value) -> serde_json::Map<String, Value> {
    let mut variables = serde_json::Map::new();
    let os = match std::env::consts::OS {
        "windows" => "WINDOWS",
        "linux" => "LINUX",
        "macos" => "OSX",
        _ => "UNKNOWN",
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "x86" | "i686" => "x86",
        "aarch64" => "arm64",
        _ => std::env::consts::ARCH,
    };
    variables.insert("os".to_owned(), Value::String(os.to_owned()));
    variables.insert("arch".to_owned(), Value::String(arch.to_owned()));
    variables.insert("locale".to_owned(), Value::String("en_US".to_owned()));
    if let Some(source_variables) = root.get("variables").and_then(Value::as_object) {
        for (key, value) in source_variables {
            variables.insert(key.clone(), value.clone());
        }
    }
    variables
}

/// Fetch the source launcher’s external crash rules and use them only when
/// their revision is newer than the bundled catalog.
pub async fn refresh_external_signatures(repositories: &[String]) -> Result<Option<u64>, String> {
    let client = crate::http::client_builder()
        .timeout(std::time::Duration::from_secs(12))
        .user_agent(concat!("LegacyLauncher-Rust/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| error.to_string())?;
    let roots: Vec<String> = if repositories.is_empty() {
        DEFAULT_SIGNATURE_REPOSITORIES
            .iter()
            .map(|root| (*root).to_owned())
            .collect()
    } else {
        repositories.to_vec()
    };

    let mut last_error = None;
    let mut saw_valid_catalog = false;
    for root in roots {
        let url = format!("{}/libraries/signature.json", root.trim_end_matches('/'));
        let response = match client.get(&url).send().await {
            Ok(response) => match response.error_for_status() {
                Ok(response) => response,
                Err(error) => {
                    last_error = Some(format!("GET {url}: {error}"));
                    continue;
                }
            },
            Err(error) => {
                last_error = Some(format!("GET {url}: {error}"));
                continue;
            }
        };
        if response
            .content_length()
            .is_some_and(|length| length > MAX_SIGNATURE_BYTES as u64)
        {
            last_error = Some(format!("GET {url}: signature catalog exceeds size limit"));
            continue;
        }
        let mut bytes = Vec::new();
        let mut response = response;
        loop {
            match response.chunk().await {
                Ok(Some(chunk))
                    if bytes.len().saturating_add(chunk.len()) <= MAX_SIGNATURE_BYTES =>
                {
                    bytes.extend_from_slice(&chunk);
                }
                Ok(Some(_)) => {
                    last_error = Some(format!("GET {url}: signature catalog exceeds size limit"));
                    bytes.clear();
                    break;
                }
                Ok(None) => break,
                Err(error) => {
                    last_error = Some(format!(
                        "GET {url}: failed to read signature catalog: {error}"
                    ));
                    bytes.clear();
                    break;
                }
            }
        }
        if bytes.is_empty() {
            continue;
        }
        let decoded = match uncompress_marked(&bytes) {
            Ok(decoded) => decoded,
            Err(error) => {
                last_error = Some(format!("GET {url}: {error}"));
                continue;
            }
        };
        let catalog: Value = match serde_json::from_slice(&decoded) {
            Ok(catalog) => catalog,
            Err(error) => {
                last_error = Some(format!("GET {url}: invalid signature JSON: {error}"));
                continue;
            }
        };
        if !valid_signature_catalog(&catalog) {
            last_error = Some(format!(
                "GET {url}: signature catalog is missing required fields"
            ));
            continue;
        }
        saw_valid_catalog = true;
        if set_newer_signature_catalog(catalog.clone()) {
            return Ok(catalog.get("revision").and_then(Value::as_u64));
        }
    }

    if saw_valid_catalog {
        Ok(None)
    } else {
        Err(last_error.unwrap_or_else(|| "no crash-signature repository available".to_owned()))
    }
}

fn current_signature_catalog() -> Value {
    let current = REMOTE_SIGNATURES.get_or_init(|| RwLock::new(None));
    if let Ok(catalog) = current.read()
        && let Some(catalog) = catalog.as_ref()
    {
        return catalog.clone();
    }
    serde_json::from_str(SIGNATURES).unwrap_or(Value::Null)
}

fn set_newer_signature_catalog(catalog: Value) -> bool {
    let Some(revision) = catalog.get("revision").and_then(Value::as_u64) else {
        return false;
    };
    let current = REMOTE_SIGNATURES.get_or_init(|| RwLock::new(None));
    let Ok(mut active) = current.write() else {
        return false;
    };
    let bundled_revision = serde_json::from_str::<Value>(SIGNATURES)
        .ok()
        .and_then(|value| value.get("revision").and_then(Value::as_u64))
        .unwrap_or(0);
    let active_revision = active
        .as_ref()
        .and_then(|value| value.get("revision").and_then(Value::as_u64))
        .unwrap_or(bundled_revision);
    if revision <= active_revision {
        return false;
    }
    *active = Some(catalog);
    true
}

fn valid_signature_catalog(catalog: &Value) -> bool {
    catalog.get("revision").and_then(Value::as_u64).is_some()
        && catalog.get("required").and_then(Value::as_str).is_some()
        && catalog
            .get("variables")
            .and_then(Value::as_object)
            .is_some()
        && catalog.get("buttons").and_then(Value::as_array).is_some()
        && catalog
            .get("signatures")
            .and_then(Value::as_array)
            .is_some()
}

fn uncompress_marked(input: &[u8]) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    match input.get(..5) {
        Some(b"gzip:") => {
            GzDecoder::new(&input[5..])
                .take(MAX_SIGNATURE_BYTES as u64 + 1)
                .read_to_end(&mut output)
                .map_err(|error| error.to_string())?;
        }
        Some(b"bzip:") => {
            BzDecoder::new(&input[5..])
                .take(MAX_SIGNATURE_BYTES as u64 + 1)
                .read_to_end(&mut output)
                .map_err(|error| error.to_string())?;
        }
        _ => output.extend_from_slice(input),
    }
    if output.len() > MAX_SIGNATURE_BYTES {
        return Err("decompressed signature catalog exceeds size limit".to_owned());
    }
    Ok(output)
}

fn resolve_button(
    button: &Value,
    shared_buttons: Option<&Vec<Value>>,
    variables: Option<&serde_json::Map<String, Value>>,
) -> Option<CrashSuggestionAction> {
    let resolved;
    let button = if let Some(name) = button.as_str() {
        resolved = shared_buttons?
            .iter()
            .find(|candidate| candidate.get("name").and_then(Value::as_str) == Some(name))?;
        resolved
    } else {
        button
    };
    let raw_label = localized(button.get("text"), variables).or_else(|| {
        button
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned)
    })?;
    let label = friendly_button_label(&raw_label);
    let actions = button.get("actions")?.as_array()?;
    let first_action = actions.first()?.as_str()?;
    let kind = if let Some(url) = first_action.strip_prefix("browse --www ") {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return None;
        }
        CrashActionKind::OpenUrl(expand_vars(url, variables))
    } else if let Some(folder) = first_action.strip_prefix("browse --folder ") {
        CrashActionKind::OpenFolder(expand_vars(folder, variables))
    } else if first_action == "gui args" {
        CrashActionKind::OpenLogs
    } else if first_action.starts_with("gui settings") {
        CrashActionKind::OpenSettings
    } else if first_action == "force-update" {
        CrashActionKind::ForceUpdate
    } else if first_action == "set --minecraft.jre.type recommended" {
        CrashActionKind::RecommendedJava
    } else if let Some(argument) = first_action.strip_prefix("set --minecraft.javaargs ") {
        CrashActionKind::SetJvmArgument(expand_vars(argument, variables))
    } else if let Some(folder) = first_action.strip_prefix("browse --folder ./") {
        CrashActionKind::OpenFolder(format!("./{folder}"))
    } else {
        return None;
    };
    Some(CrashSuggestionAction { label, kind })
}

fn localized(
    value: Option<&Value>,
    variables: Option<&serde_json::Map<String, Value>>,
) -> Option<String> {
    let value = value?;
    let value = if let Some(object) = value.as_object() {
        object.get("en_US")?
    } else {
        value
    };
    value.as_str().map(|text| expand_vars(text, variables))
}

fn expand_vars(input: &str, variables: Option<&serde_json::Map<String, Value>>) -> String {
    let mut output = input.to_owned();
    if let Some(variables) = variables {
        for (name, value) in variables {
            if let Some(value) = value.as_str() {
                output = output.replace(&format!("${{{name}}}"), value);
            }
        }
    }
    output
}

fn clean_html(input: &str) -> String {
    input
        .replace("<b>", "")
        .replace("</b>", "")
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

fn friendly_button_label(label: &str) -> String {
    match label {
        "logs" => crate::tr("rust.crash.action.logs").to_owned(),
        "open-mods" => crate::tr("rust.crash.action.mods").to_owned(),
        "open-config" => crate::tr("rust.crash.action.config").to_owned(),
        "open-game-root" => crate::tr("rust.crash.action.game-folder").to_owned(),
        "jre-recommended" => crate::tr("rust.crash.action.recommended-java").to_owned(),
        "update-java" => crate::tr("rust.crash.action.update-java").to_owned(),
        "install-java64" => crate::tr("rust.crash.action.java-64-bit").to_owned(),
        "open-settings" => crate::tr("rust.crash.action.settings").to_owned(),
        "fix-stack-size" => crate::tr("rust.crash.action.stack-size").to_owned(),
        "force-update" => crate::tr("rust.crash.action.force-update").to_owned(),
        other => other.replace('-', " "),
    }
}
