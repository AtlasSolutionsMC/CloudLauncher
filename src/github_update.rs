use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;

const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/AtlasSolutionsMC/CloudLauncher/releases/latest";
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

pub async fn check_for_update() -> Result<CheckResult, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(concat!("LegacyLauncher-Rust/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("Could not prepare the update check: {error}"))?;
    let response = client
        .get(LATEST_RELEASE_API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|error| format!("Could not check GitHub Releases: {error}"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(CheckResult::NoPublishedRelease);
    }
    let release: GithubRelease = response
        .error_for_status()
        .map_err(|error| format!("GitHub Releases returned an error: {error}"))?
        .json()
        .await
        .map_err(|error| format!("Could not read GitHub release metadata: {error}"))?;
    if release.draft || release.prerelease {
        return Ok(CheckResult::NoPublishedRelease);
    }

    let latest_version = parse_version(&release.tag_name).ok_or_else(|| {
        format!(
            "Latest release tag is not a semantic version: {}",
            release.tag_name
        )
    })?;
    let current_version = Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|error| format!("The bundled launcher version is invalid: {error}"))?;
    if latest_version <= current_version {
        return Ok(CheckResult::UpToDate);
    }
    if !release
        .html_url
        .starts_with("https://github.com/AtlasSolutionsMC/CloudLauncher/releases/")
    {
        return Err("GitHub returned an unexpected release URL.".to_owned());
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
        .ok_or_else(|| "This release does not include the Windows executable.".to_owned())?;
    let checksums_url = release
        .checksums_url
        .as_deref()
        .ok_or_else(|| "This release does not include SHA-256 checksums.".to_owned())?;
    if !exe_url.starts_with(RELEASE_ASSET_PREFIX)
        || !checksums_url.starts_with(RELEASE_ASSET_PREFIX)
    {
        return Err("Release assets use an unexpected download URL.".to_owned());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .user_agent(concat!("LegacyLauncher-Rust/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("Could not prepare the update download: {error}"))?;
    let checksums = read_limited(&client, checksums_url, MAX_CHECKSUM_BYTES).await?;
    let executable = read_limited(&client, exe_url, MAX_RELEASE_EXE_BYTES).await?;
    let expected_hash = checksum_for_asset(&checksums, EXE_ASSET_NAME)?;
    let actual_hash = Sha256::digest(&executable)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if !actual_hash.eq_ignore_ascii_case(&expected_hash) {
        return Err("The downloaded executable failed its SHA-256 checksum.".to_owned());
    }

    #[cfg(windows)]
    {
        prepare_windows_handoff(executable)
    }
    #[cfg(not(windows))]
    {
        let _ = executable;
        Err("Automatic updates are available only on Windows.".to_owned())
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
        .map_err(|error| format!("Could not download the update asset: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Update asset request failed: {error}"))?;
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err("The update asset exceeds the allowed size.".to_owned());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("Could not read the update asset: {error}"))?
    {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err("The update asset exceeds the allowed size.".to_owned());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn checksum_for_asset(checksums: &[u8], asset_name: &str) -> Result<String, String> {
    let text = std::str::from_utf8(checksums)
        .map_err(|_| "The release checksum file is not valid UTF-8.".to_owned())?;
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
    Err(format!(
        "The release checksum file has no valid entry for {asset_name}."
    ))
}

#[cfg(windows)]
fn prepare_windows_handoff(executable: Vec<u8>) -> Result<(), String> {
    use std::{io::Write, os::windows::process::CommandExt, process::Command};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let current_exe = std::env::current_exe()
        .map_err(|error| format!("Could not locate the running launcher: {error}"))?;
    let parent = current_exe
        .parent()
        .ok_or_else(|| "Could not locate the launcher's installation folder.".to_owned())?;
    let file_name = current_exe
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "The launcher's executable name is not valid Unicode.".to_owned())?;
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let replacement = parent.join(format!("{file_name}.{suffix}.update"));
    let helper = parent.join(format!("{file_name}.{suffix}.cmd"));

    fs::write(&replacement, executable)
        .map_err(|error| format!("Could not save the verified update: {error}"))?;
    let mut helper_file = match fs::File::create(&helper) {
        Ok(file) => file,
        Err(error) => {
            let _ = fs::remove_file(&replacement);
            return Err(format!(
                "Could not prepare the Windows update handoff: {error}"
            ));
        }
    };
    if let Err(error) = helper_file.write_all(include_bytes!("../assets/update_windows.cmd")) {
        let _ = fs::remove_file(&replacement);
        let _ = fs::remove_file(&helper);
        return Err(format!(
            "Could not write the Windows update handoff: {error}"
        ));
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
        return Err(format!(
            "Could not start the Windows update handoff: {error}"
        ));
    }
    Ok(())
}
