use super::super::*;

pub(in crate::app) fn valid_player_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 16
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub(in crate::app) fn valid_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= 16
        && name
            .chars()
            .all(|character| (' '..'\u{7f}').contains(&character))
}

pub(in crate::app) fn valid_version_id(version: &str) -> bool {
    !version.is_empty()
        && version != "."
        && version != ".."
        && version
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._+-".contains(character))
}

pub(in crate::app) fn remove_installed_version(
    game_dir: &std::path::Path,
    version: &str,
    remove_libraries: bool,
    custom_resolution_enabled: bool,
) -> std::io::Result<usize> {
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
    let library_paths = if remove_libraries {
        let mut visited_versions = HashSet::new();
        let mut paths = HashSet::new();
        collect_installed_library_paths(
            game_dir,
            version,
            custom_resolution_enabled,
            &mut visited_versions,
            &mut paths,
        )?;
        paths
    } else {
        HashSet::new()
    };

    fs::remove_dir_all(canonical_target)?;
    let mut removed_libraries = 0;
    for relative_path in library_paths {
        if let Some(path) = resolve_installed_library_path(game_dir, &relative_path)? {
            match fs::remove_file(path) {
                Ok(()) => removed_libraries += 1,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
    }
    Ok(removed_libraries)
}

pub(in crate::app) fn collect_installed_library_paths(
    game_dir: &std::path::Path,
    version: &str,
    custom_resolution_enabled: bool,
    visited_versions: &mut HashSet<String>,
    paths: &mut HashSet<String>,
) -> std::io::Result<()> {
    if !valid_version_id(version) || !visited_versions.insert(version.to_owned()) {
        return Ok(());
    }
    let metadata_path = game_dir
        .join("versions")
        .join(version)
        .join(format!("{version}.json"));
    let bytes = match fs::read(&metadata_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "version metadata exceeds the size limit",
        ));
    }
    let metadata: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    if let Some(libraries) = metadata
        .get("libraries")
        .and_then(serde_json::Value::as_array)
    {
        for library in libraries {
            if library.get("mod").and_then(serde_json::Value::as_bool) == Some(true)
                || library
                    .get("downloadOnly")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
                || library
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|kind| kind != "library")
            {
                continue;
            }
            if !library_applies_to_windows(library, custom_resolution_enabled) {
                continue;
            }
            let downloads = library.get("downloads");
            let native_classifier = library
                .get("natives")
                .and_then(|natives| natives.get("windows"))
                .and_then(serde_json::Value::as_str)
                .map(|classifier| {
                    classifier.replace("${arch}", if usize::BITS >= 64 { "64" } else { "32" })
                });
            let artifact_path = if let Some(classifier) = native_classifier.as_ref() {
                downloads
                    .and_then(|downloads| downloads.get("classifiers"))
                    .and_then(|classifiers| classifiers.get(classifier))
                    .and_then(|artifact| artifact.get("path"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| {
                        library
                            .get("name")
                            .and_then(serde_json::Value::as_str)
                            .and_then(|name| maven_library_path(name, Some(classifier)))
                            .map(|path| path.to_string_lossy().into_owned())
                    })
            } else {
                downloads
                    .and_then(|downloads| downloads.get("artifact"))
                    .and_then(|artifact| artifact.get("path"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| {
                        library
                            .get("name")
                            .and_then(serde_json::Value::as_str)
                            .and_then(|name| maven_library_path(name, None))
                            .map(|path| path.to_string_lossy().into_owned())
                    })
            };
            if let Some(path) = artifact_path {
                paths.insert(path);
            }
        }
    }
    if let Some(parent) = metadata
        .get("inheritsFrom")
        .and_then(serde_json::Value::as_str)
    {
        collect_installed_library_paths(
            game_dir,
            parent,
            custom_resolution_enabled,
            visited_versions,
            paths,
        )?;
    }
    Ok(())
}

pub(in crate::app) fn library_applies_to_windows(
    library: &serde_json::Value,
    custom_resolution_enabled: bool,
) -> bool {
    let Some(rules) = library.get("rules").and_then(serde_json::Value::as_array) else {
        return true;
    };
    let mut allowed = false;
    for rule in rules {
        if let Some(os) = rule.get("os") {
            if os
                .get("name")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|name| name != "windows")
                || os
                    .get("version")
                    .and_then(serde_json::Value::as_str)
                    .is_some()
            {
                continue;
            }
        }
        let features_match = rule
            .get("features")
            .and_then(serde_json::Value::as_object)
            .is_none_or(|features| {
                features.iter().all(|(name, value)| {
                    name == "has_custom_resolution"
                        && value.as_bool() == Some(true)
                        && custom_resolution_enabled
                })
            });
        if !features_match {
            continue;
        }
        allowed = rule
            .get("action")
            .and_then(serde_json::Value::as_str)
            .is_none_or(|action| action == "allow");
    }
    allowed
}

pub(in crate::app) fn maven_library_path(name: &str, classifier: Option<&str>) -> Option<PathBuf> {
    let (coordinate, extension) = name.split_once('@').unwrap_or((name, "jar"));
    let parts = coordinate.split(':').collect::<Vec<_>>();
    if !(3..=4).contains(&parts.len()) || parts.iter().any(|part| part.trim().is_empty()) {
        return None;
    }
    let group_path = parts[0].replace('.', "/");
    let selected_classifier = classifier.or_else(|| parts.get(3).copied());
    let filename = match selected_classifier {
        Some(classifier) => format!("{}-{}-{classifier}.{extension}", parts[1], parts[2]),
        None => format!("{}-{}.{}", parts[1], parts[2], extension),
    };
    Some(
        PathBuf::from(group_path)
            .join(parts[1])
            .join(parts[2])
            .join(filename),
    )
}

pub(in crate::app) fn resolve_installed_library_path(
    game_dir: &std::path::Path,
    relative_path: &str,
) -> std::io::Result<Option<PathBuf>> {
    let relative_path = std::path::Path::new(relative_path);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "library path contains unsupported path components",
        ));
    }
    let library_root = game_dir.join("libraries");
    let canonical_game_dir = game_dir.canonicalize()?;
    let canonical_root = match library_root.canonicalize() {
        Ok(root) => root,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if canonical_root.parent() != Some(canonical_game_dir.as_path()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "libraries folder resolves outside the selected Minecraft folder",
        ));
    }
    let path = library_root.join(relative_path);
    if !path.is_file() {
        return Ok(None);
    }
    let canonical_path = match path.canonicalize() {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !canonical_path.starts_with(&canonical_root) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "library path resolves outside the selected Minecraft folder",
        ));
    }
    Ok(Some(path))
}

pub(in crate::app) fn invalidate_game_data_cache(
    game_dir: &std::path::Path,
    version: &str,
) -> std::io::Result<usize> {
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
