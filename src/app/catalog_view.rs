use super::*;

impl LauncherApp {
    pub(in crate::app) fn visible_catalog_entries(&self) -> Vec<(String, String)> {
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

    pub(in crate::app) fn installed_version_type(&self, version: &str) -> Option<String> {
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

    pub(in crate::app) fn is_old_release(&self, version: &str) -> bool {
        let release_time = self
            .version_manifest
            .as_ref()
            .and_then(|manifest| manifest.versions.iter().find(|entry| entry.id == version))
            .map(|entry| entry.release_time.clone())
            .or_else(|| self.installed_version_release_time(version));
        release_time.is_some_and(|time| time.as_str() < "2014-05-14T00:00:00")
    }

    pub(in crate::app) fn installed_version_release_time(&self, version: &str) -> Option<String> {
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

    pub(in crate::app) fn version_release_time(&self, version: &str) -> Option<String> {
        self.version_manifest
            .as_ref()
            .and_then(|manifest| manifest.versions.iter().find(|entry| entry.id == version))
            .map(|entry| entry.release_time.clone())
            .or_else(|| self.installed_version_release_time(version))
    }

    pub(in crate::app) fn version_is_installed(&self) -> bool {
        self.installed_version_root(&self.settings.selected_version)
            .is_some()
    }
}
