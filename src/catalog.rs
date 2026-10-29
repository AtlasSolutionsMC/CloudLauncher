use minecraft_java_rs_core::models::minecraft::{MojangVersionManifest, VersionEntry};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;
use std::time::Duration;

const VERSION_MANIFEST_URL: &str =
    "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";
const EXTRA_VERSION_REPOS: &[&str] = &[
    "https://repo.llaun.ch/",
    "https://repo.legacylauncher.ru/",
    "https://repo.legacylauncherservices.com/",
];

#[derive(Deserialize)]
struct ExtraVersionList {
    #[serde(default)]
    versions: Vec<ExtraVersionEntry>,
}

#[derive(Deserialize)]
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

pub async fn fetch_manifest() -> Result<MojangVersionManifest, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("Could not initialize version metadata request: {error}"))?;
    let response = client
        .get(VERSION_MANIFEST_URL)
        .header(reqwest::header::USER_AGENT, "LegacyLauncher-Rust/0.1")
        .send()
        .await
        .map_err(|error| format!("Could not reach Mojang's version catalog: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Mojang's version catalog returned an error: {error}"))?;

    let mut manifest = response
        .json::<MojangVersionManifest>()
        .await
        .map_err(|error| format!("Could not read Mojang's version catalog: {error}"))?;

    let mut extra_entries = Vec::new();
    for base in EXTRA_VERSION_REPOS {
        let url = format!("{base}versions/versions.json");
        let Ok(response) = client
            .get(&url)
            .header(reqwest::header::USER_AGENT, "LegacyLauncher-Rust/0.1")
            .timeout(Duration::from_secs(5))
            .send()
            .await
        else {
            continue;
        };
        let Ok(response) = response.error_for_status() else {
            continue;
        };
        let Ok(extra) = response.json::<ExtraVersionList>().await else {
            continue;
        };
        extra_entries = extra
            .versions
            .into_iter()
            .filter(|entry| valid_version_id(&entry.id) && !entry.version_type.trim().is_empty())
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

    if !extra_entries.is_empty() {
        let mut seen = HashSet::new();
        let mut merged = Vec::with_capacity(extra_entries.len() + manifest.versions.len());
        for entry in extra_entries.into_iter().chain(manifest.versions) {
            if seen.insert(entry.id.clone()) {
                merged.push(entry);
            }
        }
        manifest.versions = merged;
    }

    Ok(manifest)
}

fn valid_version_id(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._+-".contains(character))
}

pub async fn fetch_required_java_major(
    version_id: &str,
    manifest: &MojangVersionManifest,
) -> Result<Option<u32>, String> {
    let version = manifest
        .versions
        .iter()
        .find(|entry| entry.id == version_id)
        .ok_or_else(|| format!("Minecraft version {version_id} is not in Mojang's catalog."))?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("Could not initialize Java requirement check: {error}"))?;
    let metadata = if let Some(path) = EXTRA_VERSION_REPOS
        .iter()
        .find_map(|base| version.url.strip_prefix(base))
    {
        let mut metadata = None;
        for base in EXTRA_VERSION_REPOS {
            let url = format!("{base}{path}");
            let Ok(response) = client
                .get(&url)
                .header(reqwest::header::USER_AGENT, "LegacyLauncher-Rust/0.1")
                .timeout(Duration::from_secs(10))
                .send()
                .await
            else {
                continue;
            };
            let Ok(response) = response.error_for_status() else {
                continue;
            };
            if let Ok(value) = response.json::<Value>().await {
                metadata = Some(value);
                break;
            }
        }
        metadata.ok_or_else(|| {
            format!("Could not read {version_id} metadata from Legacy Launcher repositories.")
        })?
    } else {
        let response = client
            .get(&version.url)
            .header(reqwest::header::USER_AGENT, "LegacyLauncher-Rust/0.1")
            .send()
            .await
            .map_err(|error| format!("Could not read Minecraft {version_id} metadata: {error}"))?
            .error_for_status()
            .map_err(|error| {
                format!("Minecraft {version_id} metadata returned an error: {error}")
            })?;
        response
            .json::<Value>()
            .await
            .map_err(|error| format!("Could not parse Minecraft {version_id} metadata: {error}"))?
    };
    Ok(metadata
        .pointer("/javaVersion/majorVersion")
        .and_then(Value::as_u64)
        .and_then(|major| u32::try_from(major).ok()))
}
