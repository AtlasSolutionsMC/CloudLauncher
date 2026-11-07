use super::super::*;

pub(in crate::app) fn parse_memory_size(value: &str) -> Option<u64> {
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

pub(in crate::app) fn default_legacy_xmx_mib(total_memory_mb: u32, is_64_bit: bool) -> u32 {
    if !is_64_bit || total_memory_mb <= 2_000 {
        total_memory_mb.min(1_024)
    } else if total_memory_mb <= 2_500 {
        1_536
    } else {
        2_048
    }
}

pub(in crate::app) fn legacy_release_at_or_after(release_time: &str, threshold: &str) -> bool {
    use time::format_description::well_known::Rfc3339;

    match (
        time::OffsetDateTime::parse(release_time, &Rfc3339),
        time::OffsetDateTime::parse(threshold, &Rfc3339),
    ) {
        (Ok(release_time), Ok(threshold)) => release_time >= threshold,
        _ => false,
    }
}

pub(in crate::app) fn legacy_auto_xmx_mib(
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

pub(in crate::app) fn memory_config(
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

pub(in crate::app) fn loader_config(preferences: &SavedSettings) -> LoaderConfig {
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

pub(in crate::app) fn split_arguments(arguments: &str) -> Vec<String> {
    arguments.split_whitespace().map(str::to_owned).collect()
}

pub(in crate::app) fn detect_java() -> Result<(PathBuf, String), String> {
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

pub(in crate::app) async fn verify_java_compatibility(
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

pub(in crate::app) async fn selected_java_major_for_experiments(
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

pub(in crate::app) fn local_required_java_major(
    game_dir: &std::path::Path,
    version_id: &str,
) -> Option<u32> {
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

pub(in crate::app) fn inspect_java(path: &std::path::Path) -> Result<(u32, String), String> {
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
