use super::*;

impl LauncherApp {
    pub(in crate::app) fn game_dir_path(&self) -> PathBuf {
        let configured = PathBuf::from(self.settings.game_directory.trim());
        if configured.is_absolute() {
            configured
        } else {
            default_game_directory()
        }
    }

    pub(in crate::app) fn installation_roots(&self) -> Vec<PathBuf> {
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

    pub(in crate::app) fn installed_version_root(&self, version: &str) -> Option<PathBuf> {
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

    pub(in crate::app) fn game_root_for_version(&self, version: &str) -> PathBuf {
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

    pub(in crate::app) fn game_data_directory_for_version(&self, version: &str) -> PathBuf {
        let root = self.game_root_for_version(version);
        instance_for_version(version, &self.settings)
            .map(|instance| root.join("instances").join(instance))
            .unwrap_or(root)
    }

    pub(in crate::app) fn version_family(&self, version: &str) -> String {
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
}
