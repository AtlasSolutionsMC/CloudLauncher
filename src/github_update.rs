use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;

const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/AtlasSolutionsMC/CloudLauncher/releases/latest";
const RELEASES_API: &str =
    "https://api.github.com/repos/AtlasSolutionsMC/CloudLauncher/releases?per_page=100";
const RELEASE_ASSET_PREFIX: &str =
    "https://github.com/AtlasSolutionsMC/CloudLauncher/releases/download/";
const MAX_RELEASE_EXE_BYTES: usize = 256 * 1024 * 1024;
const MAX_CHECKSUM_BYTES: usize = 64 * 1024;
const EXE_ASSET_NAME: &str = "LegacyLauncher-Rust.exe";
const CHECKSUM_ASSET_NAME: &str = "SHA256SUMS.txt";

#[derive(Debug, Clone)]
pub enum CheckResult {
    NoPublishedRelease,
    UpToDate,
    UpdateAvailable(Release),
}

#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub name: String,
    #[serde(default)]
    pub body: String,
    pub html_url: String,
    pub executable_url: Option<String>,
    pub checksums_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    draft: bool,
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

pub async fn check_for_update(include_prereleases: bool) -> Result<CheckResult, String> {
    let client = crate::http::client_builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(concat!("LegacyLauncher-Rust/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| {
            crate::tr("rust.error.update.check-client").replace("%0", &error.to_string())
        })?;
    let release = if include_prereleases {
        let response = client
            .get(RELEASES_API)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|error| {
                crate::tr("rust.error.update.check").replace("%0", &error.to_string())
            })?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(CheckResult::NoPublishedRelease);
        }
        let releases = response
            .error_for_status()
            .map_err(|error| crate::tr("rust.error.update.http").replace("%0", &error.to_string()))?
            .json::<Vec<GithubRelease>>()
            .await
            .map_err(|error| {
                crate::tr("rust.error.update.metadata").replace("%0", &error.to_string())
            })?;
        let Some((_, release)) = releases
            .into_iter()
            .filter(|release| !release.draft)
            .filter_map(|release| {
                parse_version(&release.tag_name).map(|version| (version, release))
            })
            .max_by(|(left, _), (right, _)| left.cmp(right))
        else {
            return Ok(CheckResult::NoPublishedRelease);
        };
        release
    } else {
        let response = client
            .get(LATEST_RELEASE_API)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|error| {
                crate::tr("rust.error.update.check").replace("%0", &error.to_string())
            })?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(CheckResult::NoPublishedRelease);
        }
        let release: GithubRelease = response
            .error_for_status()
            .map_err(|error| crate::tr("rust.error.update.http").replace("%0", &error.to_string()))?
            .json()
            .await
            .map_err(|error| {
                crate::tr("rust.error.update.metadata").replace("%0", &error.to_string())
            })?;
        if release.draft || release.prerelease {
            return Ok(CheckResult::NoPublishedRelease);
        }
        release
    };

    let latest_version = parse_version(&release.tag_name).ok_or_else(|| {
        crate::tr("rust.error.update.tag-invalid").replace("%0", &release.tag_name)
    })?;
    let current_version = Version::parse(env!("CARGO_PKG_VERSION")).map_err(|error| {
        crate::tr("rust.error.update.version-invalid").replace("%0", &error.to_string())
    })?;
    if latest_version <= current_version {
        return Ok(CheckResult::UpToDate);
    }
    if !release
        .html_url
        .starts_with("https://github.com/AtlasSolutionsMC/CloudLauncher/releases/")
    {
        return Err(crate::tr("rust.error.update.release-url").to_owned());
    }

    let executable_url = release
        .assets
        .iter()
        .find(|asset| asset.name == EXE_ASSET_NAME)
        .map(|asset| asset.browser_download_url.clone())
        .filter(|url| url.starts_with(RELEASE_ASSET_PREFIX));
    let checksums_url = release
        .assets
        .iter()
        .find(|asset| asset.name == CHECKSUM_ASSET_NAME)
        .map(|asset| asset.browser_download_url.clone())
        .filter(|url| url.starts_with(RELEASE_ASSET_PREFIX));
    Ok(CheckResult::UpdateAvailable(Release {
        tag_name: release.tag_name,
        name: release.name.unwrap_or_else(|| latest_version.to_string()),
        body: release.body.unwrap_or_default(),
        html_url: release.html_url,
        executable_url,
        checksums_url,
    }))
}

fn parse_version(tag: &str) -> Option<Version> {
    Version::parse(tag.trim().strip_prefix('v').unwrap_or(tag.trim())).ok()
}

pub async fn download_and_handoff(release: Release) -> Result<(), String> {
    let exe_url = release
        .executable_url
        .as_deref()
        .ok_or_else(|| crate::tr("rust.error.update.exe-missing").to_owned())?;
    let checksums_url = release
        .checksums_url
        .as_deref()
        .ok_or_else(|| crate::tr("rust.error.update.checksums-missing").to_owned())?;
    if !exe_url.starts_with(RELEASE_ASSET_PREFIX)
        || !checksums_url.starts_with(RELEASE_ASSET_PREFIX)
    {
        return Err(crate::tr("rust.error.update.asset-url").to_owned());
    }

    let client = crate::http::client_builder()
        .timeout(std::time::Duration::from_secs(180))
        .user_agent(concat!("LegacyLauncher-Rust/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| {
            crate::tr("rust.error.update.download-client").replace("%0", &error.to_string())
        })?;
    let checksums = read_limited(&client, checksums_url, MAX_CHECKSUM_BYTES).await?;
    let executable = read_limited(&client, exe_url, MAX_RELEASE_EXE_BYTES).await?;
    let expected_hash = checksum_for_asset(&checksums, EXE_ASSET_NAME)?;
    let actual_hash = Sha256::digest(&executable)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if !actual_hash.eq_ignore_ascii_case(&expected_hash) {
        return Err(crate::tr("rust.error.update.checksum-mismatch").to_owned());
    }

    #[cfg(windows)]
    {
        prepare_windows_handoff(executable)
    }
    #[cfg(not(windows))]
    {
        let _ = executable;
        Err(crate::tr("rust.error.update.windows-only").to_owned())
    }
}

async fn read_limited(
    client: &reqwest::Client,
    url: &str,
    limit: usize,
) -> Result<Vec<u8>, String> {
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|error| crate::tr("rust.error.update.download").replace("%0", &error.to_string()))?
        .error_for_status()
        .map_err(|error| {
            crate::tr("rust.error.update.asset-http").replace("%0", &error.to_string())
        })?;
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(crate::tr("rust.error.update.asset-large").to_owned());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        crate::tr("rust.error.update.asset-read").replace("%0", &error.to_string())
    })? {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(crate::tr("rust.error.update.asset-large").to_owned());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn checksum_for_asset(checksums: &[u8], asset_name: &str) -> Result<String, String> {
    let text = std::str::from_utf8(checksums)
        .map_err(|_| crate::tr("rust.error.update.checksums-utf8").to_owned())?;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(hash) = parts.next() else { continue };
        if parts.last() == Some(asset_name)
            && hash.len() == 64
            && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Ok(hash.to_ascii_lowercase());
        }
    }
    Err(crate::tr("rust.error.update.checksum-entry-missing").replace("%0", asset_name))
}

#[cfg(windows)]
fn prepare_windows_handoff(executable: Vec<u8>) -> Result<(), String> {
    use std::{io::Write, os::windows::process::CommandExt, process::Command};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let current_exe = std::env::current_exe().map_err(|error| {
        crate::tr("rust.error.update.exe-locate").replace("%0", &error.to_string())
    })?;
    let parent = current_exe
        .parent()
        .ok_or_else(|| crate::tr("rust.error.update.install-folder").to_owned())?;
    let file_name = current_exe
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| crate::tr("rust.error.update.exe-name-invalid").to_owned())?;
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let replacement = parent.join(format!("{file_name}.{suffix}.update"));
    let helper = parent.join(format!("{file_name}.{suffix}.cmd"));

    fs::write(&replacement, executable)
        .map_err(|error| crate::tr("rust.error.update.save").replace("%0", &error.to_string()))?;
    let mut helper_file = match fs::File::create(&helper) {
        Ok(file) => file,
        Err(error) => {
            let _ = fs::remove_file(&replacement);
            return Err(
                crate::tr("rust.error.update.handoff-prepare").replace("%0", &error.to_string())
            );
        }
    };
    if let Err(error) = helper_file.write_all(include_bytes!("../assets/update_windows.cmd")) {
        let _ = fs::remove_file(&replacement);
        let _ = fs::remove_file(&helper);
        return Err(crate::tr("rust.error.update.handoff-write").replace("%0", &error.to_string()));
    }
    drop(helper_file);

    let spawn_result = Command::new("cmd.exe")
        .arg("/D")
        .arg("/C")
        .arg(&helper)
        .arg(std::process::id().to_string())
        .arg(&current_exe)
        .arg(&replacement)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
    if let Err(error) = spawn_result {
        let _ = fs::remove_file(&replacement);
        let _ = fs::remove_file(&helper);
        return Err(crate::tr("rust.error.update.handoff-start").replace("%0", &error.to_string()));
    }
    Ok(())
}
