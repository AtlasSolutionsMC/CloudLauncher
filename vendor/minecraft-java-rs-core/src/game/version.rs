use crate::error::LaunchError;
use crate::launcher::options::LaunchOptions;
use crate::models::minecraft::{MinecraftVersionJson, MojangVersionManifest, VersionEntry};
use crate::net::http::fetch_json;
use regex::Regex;
use serde_json::Value;
use std::{collections::HashSet, time::Duration};

const MANIFEST_URL: &str = "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";
const EXTRA_VERSION_REPOS: &[&str] = &[
    "https://repo.llaun.ch/",
    "https://repo.legacylauncher.ru/",
    "https://repo.legacylauncherservices.com/",
];

#[derive(serde::Deserialize)]
struct ExtraVersionList {
    #[serde(default)]
    versions: Vec<ExtraVersionEntry>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExtraVersionEntry {
    id: String,
    #[serde(rename = "type", default)]
    version_type: String,
    #[serde(default)]
    time: String,
    #[serde(default)]
    release_time: String,
}

/// Fetch and return the `MinecraftVersionJson` for the version requested in
/// `options`.
///
/// Steps:
/// 1. Download the Mojang version manifest.
/// 2. Resolve version aliases (`latest_release` / `r` / `lr`, etc.).
/// 3. Locate the version entry and download its per-version JSON.
/// 4. On Linux ARM, patch LWJGL/JInput libraries via `lwjgl_native::process_json`.
pub async fn get_version_json(
    options: &LaunchOptions,
    client: &reqwest::Client,
) -> Result<MinecraftVersionJson, LaunchError> {
    fetch_version_json(MANIFEST_URL, options, client).await
}

/// Inner implementation — accepts an explicit manifest base URL so that tests
/// can point at a local mock server instead of the real Mojang CDN.
pub(crate) async fn fetch_version_json(
    manifest_base: &str,
    options: &LaunchOptions,
    client: &reqwest::Client,
) -> Result<MinecraftVersionJson, LaunchError> {
    // Cache-buster keeps CDN proxies from returning stale manifests.
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let manifest_url = format!("{manifest_base}?_t={ts}");

    let mut manifest: MojangVersionManifest = fetch_json(client, &manifest_url)
        .await
        .map_err(LaunchError::InvalidData)?;
    if manifest_base.starts_with("https://launchermeta.mojang.com/") {
        merge_extra_versions(&mut manifest, client).await;
    }

    let version = resolve_alias(&options.version, &manifest);

    let mut version_json: MinecraftVersionJson =
        if let Some(entry) = manifest.versions.iter().find(|entry| entry.id == version) {
            if is_extra_version_url(&entry.url) {
                let (mut metadata, repo) = fetch_extra_version_json(client, &version)
                    .await
                    .map_err(LaunchError::InvalidData)?;
                normalize_extra_urls(&mut metadata, repo);
                serde_json::from_value(metadata).map_err(LaunchError::Json)?
            } else {
                fetch_json(client, &entry.url)
                    .await
                    .map_err(LaunchError::InvalidData)?
            }
        } else {
            load_custom_version(&options.path, &version, &manifest, client).await?
        };

    let mut replacement_types = options.library_replace_types.clone();
    if let Some(replacement_type) = options.library_replace_type.as_ref() {
        if !replacement_types.contains(replacement_type) {
            replacement_types.push(replacement_type.clone());
        }
    }
    if !replacement_types.is_empty() {
        persist_base_version_json(options, &version_json).await?;
        if let Some((mut replacements, repo)) = fetch_library_replacements(client).await {
            normalize_extra_urls(&mut replacements, repo);
            for replacement_type in &replacement_types {
                apply_library_replacements(
                    &mut version_json,
                    &replacements,
                    replacement_type,
                    repo,
                );
            }
        }
    }

    if is_linux_arm() {
        crate::game::lwjgl_native::process_json(&mut version_json)?;
    }

    Ok(version_json)
}

async fn persist_base_version_json(
    options: &LaunchOptions,
    version_json: &MinecraftVersionJson,
) -> Result<(), LaunchError> {
    let directory = options.path.join("versions").join(&version_json.id);
    tokio::fs::create_dir_all(&directory).await?;
    let content = serde_json::to_vec_pretty(version_json)?;
    tokio::fs::write(directory.join(format!("{}.json", version_json.id)), content).await?;
    Ok(())
}

async fn fetch_library_replacements(client: &reqwest::Client) -> Option<(Value, &'static str)> {
    for repo in EXTRA_VERSION_REPOS {
        let url = format!("{repo}libraries/replace.json");
        let Ok(response) = client
            .get(&url)
            .timeout(Duration::from_secs(6))
            .send()
            .await
        else {
            continue;
        };
        let Ok(response) = response.error_for_status() else {
            continue;
        };
        let Ok(payload) = response.json::<Value>().await else {
            continue;
        };
        if payload
            .get("libraries")
            .and_then(Value::as_object)
            .is_some()
        {
            return Some((payload, repo));
        }
    }
    None
}

fn apply_library_replacements(
    version_json: &mut MinecraftVersionJson,
    payload: &Value,
    replacement_type: &str,
    repo: &str,
) {
    let Some(replacements) = payload
        .pointer(&format!("/libraries/{replacement_type}"))
        .and_then(Value::as_array)
    else {
        return;
    };
    let Ok(mut version) = serde_json::to_value(&*version_json) else {
        return;
    };
    let version_id = version_json.id.clone();
    let mut appended_args = Vec::new();
    let mut main_class_override = None;

    {
        let Some(libraries) = version.get_mut("libraries").and_then(Value::as_array_mut) else {
            return;
        };

        for replacement in replacements {
            let name = replacement
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if name.is_empty()
                || libraries
                    .iter()
                    .any(|library| library_name(library) == name)
            {
                continue;
            }
            let pattern = replacement
                .get("replaces")
                .and_then(Value::as_str)
                .and_then(|pattern| Regex::new(&format!("^(?:{pattern})$")).ok());
            let pattern_matches = pattern.as_ref().is_some_and(|pattern| {
                libraries
                    .iter()
                    .any(|library| pattern.is_match(library_name(library)))
            });
            let supports_version = replacement
                .get("supports")
                .and_then(Value::as_array)
                .is_some_and(|supported| {
                    supported
                        .iter()
                        .any(|version| version.as_str() == Some(version_id.as_str()))
                });
            if !supports_version && !pattern_matches {
                continue;
            }

            let mut replacement = replacement.clone();
            set_default_library_repo(&mut replacement, repo);
            if let Some(required) = replacement
                .get_mut("requires")
                .and_then(Value::as_array_mut)
            {
                for library in required.iter_mut() {
                    set_default_library_repo(library, repo);
                }
                let mut required = std::mem::take(required);
                for library in libraries.iter_mut() {
                    if let Some(index) = required.iter().position(|candidate| {
                        plain_library_name(candidate) == plain_library_name(library)
                    }) {
                        *library = required.remove(index);
                    }
                }
                libraries.splice(0..0, required);
            }

            if let Some(pattern) = pattern {
                for library in libraries.iter_mut() {
                    if pattern.is_match(library_name(library)) {
                        *library = replacement.clone();
                    }
                }
            } else {
                libraries.insert(0, replacement.clone());
            }

            if let Some(args) = replacement
                .get("args")
                .and_then(Value::as_str)
                .filter(|args| !args.trim().is_empty())
            {
                appended_args.push(args.to_owned());
            }
            if let Some(main_class) = replacement.get("mainClass").and_then(Value::as_str) {
                main_class_override = Some(main_class.to_owned());
            }
        }
    }

    for args in appended_args {
        append_replacement_args(&mut version, &args);
    }
    if let (Some(main_class), Some(object)) = (main_class_override, version.as_object_mut()) {
        object.insert("mainClass".to_owned(), Value::String(main_class));
    }

    if let Ok(updated) = serde_json::from_value(version) {
        *version_json = updated;
    }
}

fn set_default_library_repo(library: &mut Value, repo: &str) {
    let Some(library) = library.as_object_mut() else {
        return;
    };
    library
        .entry("url")
        .or_insert_with(|| Value::String(format!("{}/libraries/", repo.trim_end_matches('/'))));
}

fn library_name(library: &Value) -> &str {
    library
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .split('@')
        .next()
        .unwrap_or_default()
}

fn plain_library_name(library: &Value) -> String {
    let parts = library_name(library).split(':').collect::<Vec<_>>();
    if parts.len() < 2 {
        return library_name(library).to_owned();
    }
    if parts.len() >= 4 {
        format!("{}:{}::{}", parts[0], parts[1], parts[3])
    } else {
        format!("{}:{}", parts[0], parts[1])
    }
}

fn append_replacement_args(version: &mut Value, args: &str) {
    if let Some(raw) = version
        .get("minecraftArguments")
        .and_then(Value::as_str)
        .map(str::to_owned)
    {
        let appended = format!("{} {args}", raw.trim()).trim().to_owned();
        version["minecraftArguments"] = Value::String(appended);
        return;
    }
    let Some(object) = version.as_object_mut() else {
        return;
    };
    let arguments = object
        .entry("arguments")
        .or_insert_with(|| Value::Object(Default::default()));
    let Some(arguments) = arguments.as_object_mut() else {
        return;
    };
    let game = arguments
        .entry("game")
        .or_insert_with(|| Value::Array(Vec::new()));
    if let Some(game) = game.as_array_mut() {
        game.extend(
            args.split_whitespace()
                .map(|arg| Value::String(arg.to_owned())),
        );
    }
}

async fn merge_extra_versions(manifest: &mut MojangVersionManifest, client: &reqwest::Client) {
    let mut extra_entries = Vec::new();
    for base in EXTRA_VERSION_REPOS {
        let url = format!("{base}versions/versions.json");
        let request = fetch_json::<ExtraVersionList>(client, &url);
        let Ok(Ok(extra)) = tokio::time::timeout(Duration::from_secs(5), request).await else {
            continue;
        };
        extra_entries = extra
            .versions
            .into_iter()
            .filter(|entry| safe_version_id(&entry.id) && !entry.version_type.trim().is_empty())
            .map(|entry| VersionEntry {
                id: entry.id.clone(),
                version_type: entry.version_type,
                url: format!("{base}versions/{}.json", entry.id),
                time: entry.time,
                release_time: entry.release_time,
            })
            .collect();
        if !extra_entries.is_empty() {
            break;
        }
    }

    if extra_entries.is_empty() {
        return;
    }
    let mut seen = HashSet::new();
    manifest.versions = extra_entries
        .into_iter()
        .chain(std::mem::take(&mut manifest.versions))
        .filter(|entry| seen.insert(entry.id.clone()))
        .collect();
}

fn is_extra_version_url(url: &str) -> bool {
    EXTRA_VERSION_REPOS.iter().any(|repo| url.starts_with(repo))
}

async fn fetch_extra_version_json(
    client: &reqwest::Client,
    version_id: &str,
) -> Result<(Value, &'static str), String> {
    if !safe_version_id(version_id) {
        return Err(format!("Invalid remote version ID: {version_id}"));
    }
    let mut last_error = String::new();
    for repo in EXTRA_VERSION_REPOS {
        let url = format!("{repo}versions/{version_id}.json");
        let request = fetch_json::<Value>(client, &url);
        match tokio::time::timeout(Duration::from_secs(10), request).await {
            Ok(Ok(value)) => return Ok((value, repo)),
            Ok(Err(error)) => last_error = error,
            Err(_) => last_error = format!("GET {url}: request timed out"),
        }
    }
    Err(last_error)
}

fn normalize_extra_urls(value: &mut Value, repo: &str) {
    match value {
        Value::Object(object) => {
            for (key, child) in object.iter_mut() {
                if key == "url" {
                    if let Some(url) = child.as_str() {
                        if url.starts_with('/') {
                            *child =
                                Value::String(format!("{}{}", repo.trim_end_matches('/'), url));
                            continue;
                        }
                    }
                    normalize_extra_urls(child, repo);
                } else {
                    normalize_extra_urls(child, repo);
                }
            }
        }
        Value::Array(items) => {
            for child in items {
                normalize_extra_urls(child, repo);
            }
        }
        _ => {}
    }
}

/// Load an installed custom version and resolve its `inheritsFrom` chain.
/// Parent JSON can come from the local versions directory or the Mojang manifest.
async fn load_custom_version(
    game_dir: &std::path::Path,
    version_id: &str,
    manifest: &MojangVersionManifest,
    client: &reqwest::Client,
) -> Result<MinecraftVersionJson, LaunchError> {
    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    let mut current = version_id.to_owned();
    let mut resolved = false;

    for _ in 0..32 {
        if !safe_version_id(&current) {
            return Err(LaunchError::InvalidData(format!(
                "custom version contains an invalid parent ID: {current}"
            )));
        }
        if !seen.insert(current.clone()) {
            return Err(LaunchError::InvalidData(format!(
                "custom version inheritance cycle at {current}"
            )));
        }
        let path = game_dir
            .join("versions")
            .join(&current)
            .join(format!("{current}.json"));
        let bytes = match tokio::fs::read(path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let Some(entry) = manifest.versions.iter().find(|entry| entry.id == current) else {
                    return Err(LaunchError::VersionNotFound(current));
                };
                let parent: Value = if is_extra_version_url(&entry.url) {
                    let (parent, repo) = fetch_extra_version_json(client, &current)
                        .await
                        .map_err(LaunchError::InvalidData)?;
                    let mut normalized = parent;
                    normalize_extra_urls(&mut normalized, repo);
                    normalized
                } else {
                    fetch_json(client, &entry.url)
                        .await
                        .map_err(LaunchError::InvalidData)?
                };
                chain.push(parent);
                resolved = true;
                break;
            }
            Err(error) => return Err(error.into()),
        };
        let local: Value = serde_json::from_slice(&bytes)?;
        let parent_id = local
            .get("inheritsFrom")
            .and_then(Value::as_str)
            .map(str::to_owned);
        chain.push(local);
        let Some(parent_id) = parent_id else {
            resolved = true;
            break;
        };
        current = parent_id;
    }

    if chain.is_empty() {
        return Err(LaunchError::VersionNotFound(version_id.to_owned()));
    }
    if !resolved {
        return Err(LaunchError::InvalidData(
            "custom version inheritance exceeds 32 levels".to_owned(),
        ));
    }

    let mut merged = chain.pop().expect("non-empty version chain");
    while let Some(child) = chain.pop() {
        merged = merge_version_json(merged, child);
    }
    let object = merged.as_object_mut().ok_or_else(|| {
        LaunchError::InvalidData("custom version JSON must be an object".to_owned())
    })?;
    object.remove("inheritsFrom");
    object.insert("id".to_owned(), Value::String(version_id.to_owned()));
    serde_json::from_value(merged).map_err(LaunchError::Json)
}

fn safe_version_id(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._+-".contains(character))
}

fn merge_version_json(mut parent: Value, child: Value) -> Value {
    let Some(parent_object) = parent.as_object_mut() else {
        return child;
    };
    let Some(child_object) = child.as_object() else {
        return parent;
    };
    for (key, child_value) in child_object {
        match key.as_str() {
            "libraries" => {
                let mut libraries = parent_object
                    .get(key)
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if let Some(child_libraries) = child_value.as_array() {
                    for library in child_libraries {
                        let name = library.get("name").and_then(Value::as_str);
                        if let Some(name) = name {
                            if let Some(existing) = libraries.iter_mut().find(|existing| {
                                existing.get("name").and_then(Value::as_str) == Some(name)
                            }) {
                                *existing = library.clone();
                                continue;
                            }
                        }
                        libraries.push(library.clone());
                    }
                }
                parent_object.insert(key.clone(), Value::Array(libraries));
            }
            "arguments" => {
                let mut arguments = parent_object
                    .get(key)
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                if let Some(child_arguments) = child_value.as_object() {
                    for (argument_key, child_entries) in child_arguments {
                        if matches!(argument_key.as_str(), "game" | "jvm") {
                            let mut entries = arguments
                                .get(argument_key)
                                .and_then(Value::as_array)
                                .cloned()
                                .unwrap_or_default();
                            if let Some(child_entries) = child_entries.as_array() {
                                entries.extend(child_entries.iter().cloned());
                            }
                            arguments.insert(argument_key.clone(), Value::Array(entries));
                        } else {
                            arguments.insert(argument_key.clone(), child_entries.clone());
                        }
                    }
                }
                parent_object.insert(key.clone(), Value::Object(arguments));
            }
            "downloads" => {
                let mut downloads = parent_object
                    .get(key)
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                if let Some(child_downloads) = child_value.as_object() {
                    downloads.extend(child_downloads.clone());
                }
                parent_object.insert(key.clone(), Value::Object(downloads));
            }
            "minecraftArguments" => {
                let parent_args = parent_object.get(key).and_then(Value::as_str).unwrap_or("");
                let child_args = child_value.as_str().unwrap_or("");
                let merged_args = match (parent_args.is_empty(), child_args.is_empty()) {
                    (true, _) => child_args.to_owned(),
                    (_, true) => parent_args.to_owned(),
                    _ => format!("{parent_args} {child_args}"),
                };
                parent_object.insert(key.clone(), Value::String(merged_args));
            }
            _ => {
                parent_object.insert(key.clone(), child_value.clone());
            }
        }
    }
    parent_object.remove("inheritsFrom");
    parent
}

/// Map symbolic aliases to the concrete version ID from the manifest.
fn resolve_alias(version: &str, manifest: &MojangVersionManifest) -> String {
    match version {
        "latest_release" | "r" | "lr" => manifest.latest.release.clone(),
        "latest_snapshot" | "s" | "ls" => manifest.latest.snapshot.clone(),
        other => other.to_string(),
    }
}

/// Runtime check: are we on Linux ARM?
///
/// Uses `std::env::consts` so that a cross-compiled binary running on ARM
/// still detects its actual execution environment.
fn is_linux_arm() -> bool {
    std::env::consts::OS == "linux" && matches!(std::env::consts::ARCH, "aarch64" | "arm")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::minecraft::{LatestVersions, MojangVersionManifest, VersionEntry};

    fn manifest(release: &str, snapshot: &str) -> MojangVersionManifest {
        MojangVersionManifest {
            latest: LatestVersions {
                release: release.to_string(),
                snapshot: snapshot.to_string(),
            },
            versions: vec![make_entry("1.20.4"), make_entry("24w14a")],
        }
    }

    fn make_entry(id: &str) -> VersionEntry {
        VersionEntry {
            id: id.to_string(),
            version_type: "release".to_string(),
            url: format!("https://example.com/{id}.json"),
            time: String::new(),
            release_time: String::new(),
        }
    }

    #[test]
    fn alias_latest_release() {
        let m = manifest("1.20.4", "24w14a");
        assert_eq!(resolve_alias("latest_release", &m), "1.20.4");
        assert_eq!(resolve_alias("r", &m), "1.20.4");
        assert_eq!(resolve_alias("lr", &m), "1.20.4");
    }

    #[test]
    fn alias_latest_snapshot() {
        let m = manifest("1.20.4", "24w14a");
        assert_eq!(resolve_alias("latest_snapshot", &m), "24w14a");
        assert_eq!(resolve_alias("s", &m), "24w14a");
        assert_eq!(resolve_alias("ls", &m), "24w14a");
    }

    #[test]
    fn concrete_version_passes_through() {
        let m = manifest("1.20.4", "24w14a");
        assert_eq!(resolve_alias("1.19.4", &m), "1.19.4");
    }

    // ── Mock HTTP tests (wiremock) ────────────────────────────────────────────

    /// Minimal manifest JSON template; `VERSION_URL` is replaced at test time
    /// with the actual mock server URL.
    const MOCK_MANIFEST_TEMPLATE: &str = r#"{
        "latest": { "release": "1.20.4", "snapshot": "1.20.4" },
        "versions": [
            {
                "id": "1.20.4",
                "type": "release",
                "url": "VERSION_URL",
                "time": "2024-01-22T10:00:00+00:00",
                "releaseTime": "2024-01-22T10:00:00+00:00"
            }
        ]
    }"#;

    const MOCK_VERSION_JSON: &str = r#"{
        "id": "1.20.4",
        "type": "release",
        "assets": "16",
        "libraries": [],
        "mainClass": "net.minecraft.client.main.Main",
        "assetIndex": {
            "id": "16",
            "sha1": "abc123",
            "size": 100,
            "url": "https://resources.example.com/16.json"
        },
        "downloads": {
            "client": {
                "sha1": "def456",
                "size": 1000,
                "url": "https://resources.example.com/client.jar"
            }
        }
    }"#;

    fn mock_options(version: &str) -> LaunchOptions {
        use crate::launcher::options::{JavaOptions, LoaderConfig, MemoryConfig, ScreenConfig};
        use crate::models::minecraft::Authenticator;
        LaunchOptions {
            path: std::path::PathBuf::from("/tmp/mc-test"),
            version: version.to_string(),
            authenticator: Authenticator {
                access_token: String::new(),
                name: "TestUser".into(),
                uuid: "test-uuid".into(),
                xbox_account: None,
                user_properties: None,
                client_id: None,
                client_token: None,
            },
            timeout_secs: 5,
            download_concurrency: 1,
            verify_concurrency: 4,
            memory: MemoryConfig::default(),
            java: JavaOptions::default(),
            loader: LoaderConfig::default(),
            screen: ScreenConfig::default(),
            verify: false,
            game_args: vec![],
            jvm_args: vec![],
            instance: None,
            url: None,
            mcp: None,
            library_replace_type: None,
            library_replace_types: vec![],
            intel_enabled_mac: false,
            bypass_offline: false,
            skip_bundle_check: false,
            force_ipv4: false,
            dns: None,
        }
    }

    #[tokio::test]
    async fn fetch_version_json_from_mock_server_concrete_version() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;

        // Register manifest mock.
        let version_url = format!("{}/versions/1.20.4.json", server.uri());
        let manifest_body = MOCK_MANIFEST_TEMPLATE.replace("VERSION_URL", &version_url);
        Mock::given(method("GET"))
            .and(path("/manifest.json"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .set_body_string(manifest_body),
            )
            .mount(&server)
            .await;

        // Register version JSON mock.
        Mock::given(method("GET"))
            .and(path("/versions/1.20.4.json"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .set_body_string(MOCK_VERSION_JSON),
            )
            .mount(&server)
            .await;

        let manifest_url = format!("{}/manifest.json", server.uri());
        let client = reqwest::Client::new();
        let options = mock_options("1.20.4");

        let result = fetch_version_json(&manifest_url, &options, &client)
            .await
            .unwrap();

        assert_eq!(result.id, "1.20.4");
        assert_eq!(result.version_type, "release");
        assert_eq!(
            result.main_class.as_deref(),
            Some("net.minecraft.client.main.Main")
        );
        assert!(result.libraries.is_empty());
    }

    #[tokio::test]
    async fn fetch_version_json_resolves_latest_release_alias() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;

        let version_url = format!("{}/versions/1.20.4.json", server.uri());
        let manifest_body = MOCK_MANIFEST_TEMPLATE.replace("VERSION_URL", &version_url);
        Mock::given(method("GET"))
            .and(path("/manifest.json"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .set_body_string(manifest_body),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/versions/1.20.4.json"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .set_body_string(MOCK_VERSION_JSON),
            )
            .mount(&server)
            .await;

        let manifest_url = format!("{}/manifest.json", server.uri());
        let client = reqwest::Client::new();
        // "latest_release" should resolve to "1.20.4" via the mock manifest.
        let options = mock_options("latest_release");

        let result = fetch_version_json(&manifest_url, &options, &client)
            .await
            .unwrap();

        assert_eq!(result.id, "1.20.4");
    }

    #[tokio::test]
    async fn fetch_version_json_returns_error_for_missing_version() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;

        let version_url = format!("{}/versions/1.20.4.json", server.uri());
        let manifest_body = MOCK_MANIFEST_TEMPLATE.replace("VERSION_URL", &version_url);
        Mock::given(method("GET"))
            .and(path("/manifest.json"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .set_body_string(manifest_body),
            )
            .mount(&server)
            .await;

        let manifest_url = format!("{}/manifest.json", server.uri());
        let client = reqwest::Client::new();
        let options = mock_options("99.99.99");

        let result = fetch_version_json(&manifest_url, &options, &client).await;

        assert!(matches!(result, Err(LaunchError::VersionNotFound(_))));
    }

    #[tokio::test]
    async fn fetch_version_json_returns_http_error_on_500() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/manifest.json"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let manifest_url = format!("{}/manifest.json", server.uri());
        let client = reqwest::Client::new();
        let options = mock_options("1.20.4");

        let result = fetch_version_json(&manifest_url, &options, &client).await;
        assert!(result.is_err());
    }
}
