use super::*;

impl LauncherApp {
    pub(in crate::app) fn promoted_version_family(&self, version: &str) -> Option<String> {
        for root in self.installation_roots() {
            let path = root
                .join("versions")
                .join(version)
                .join(format!("{version}.json"));
            if let Ok(bytes) = fs::read(path) {
                if let Ok(metadata) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                    if let Some(family) = metadata
                        .get("family")
                        .and_then(serde_json::Value::as_str)
                        .map(str::trim)
                        .filter(|family| !family.is_empty())
                    {
                        return Some(family.to_owned());
                    }
                }
            }
        }
        legacy_promoted_version_family(version)
    }

    pub(in crate::app) fn promoted_family_versions(&self, families: &[String]) -> Vec<String> {
        let mut result = Vec::new();
        for family in families
            .iter()
            .map(|family| family.trim())
            .filter(|family| !family.is_empty())
        {
            let mut candidates: HashMap<String, String> = HashMap::new();
            if let Some(manifest) = &self.version_manifest {
                for entry in manifest
                    .versions
                    .iter()
                    .filter(|entry| !entry.version_type.eq_ignore_ascii_case("snapshot"))
                {
                    let matches = entry.id == family
                        || self
                            .promoted_version_family(&entry.id)
                            .is_some_and(|candidate_family| candidate_family == family);
                    if matches {
                        candidates.insert(entry.id.clone(), entry.release_time.clone());
                    }
                }
            }
            for installed in self.installed_versions() {
                if let Some(metadata) = self.installed_version_metadata(&installed) {
                    if metadata
                        .get("type")
                        .or_else(|| metadata.get("releaseType"))
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|kind| kind.eq_ignore_ascii_case("snapshot"))
                    {
                        continue;
                    }
                    let candidate_family = metadata
                        .get("family")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                        .or_else(|| legacy_promoted_version_family(&installed));
                    if installed == family || candidate_family.as_deref() == Some(family) {
                        let release_time = metadata
                            .get("releaseTime")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .to_owned();
                        candidates.insert(installed, release_time);
                    }
                }
            }
            let mut family_candidates = candidates.into_iter().collect::<Vec<_>>();
            family_candidates.sort_by(|left, right| right.1.cmp(&left.1));
            result.extend(
                family_candidates
                    .into_iter()
                    .take(2)
                    .map(|(version, _)| version),
            );
        }
        let mut seen = HashSet::new();
        result.retain(|version| seen.insert(version.clone()));
        result
    }

    pub(in crate::app) fn promoted_servers_for_version(&self, version: &str) -> Vec<SavedServer> {
        let family = self.promoted_version_family(version);
        let mut servers = Vec::new();
        for notice in &self.notice_list {
            if !notice.promoted {
                continue;
            }
            let Some(notices::NoticeAction::Server {
                label,
                address,
                families,
                ..
            }) = &notice.action
            else {
                continue;
            };
            append_promoted_server(
                &mut servers,
                version,
                family.as_deref(),
                label,
                address,
                families,
            );
        }
        for action in &self.bootstrap_promoted_servers {
            let notices::NoticeAction::Server {
                label,
                address,
                families,
                ..
            } = action
            else {
                continue;
            };
            append_promoted_server(
                &mut servers,
                version,
                family.as_deref(),
                label,
                address,
                families,
            );
        }
        servers
    }

    pub(in crate::app) fn installed_version_metadata(
        &self,
        version: &str,
    ) -> Option<serde_json::Value> {
        self.installation_roots().into_iter().find_map(|root| {
            let path = root
                .join("versions")
                .join(version)
                .join(format!("{version}.json"));
            fs::read(path)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        })
    }

    pub(in crate::app) fn installed_version_is_outdated(&self, version: &str) -> bool {
        let remote_time = self
            .version_manifest
            .as_ref()
            .and_then(|manifest| manifest.versions.iter().find(|entry| entry.id == version))
            .and_then(|entry| OffsetDateTime::parse(&entry.time, &Rfc3339).ok());
        let local_time = self
            .installed_version_metadata(version)
            .and_then(|metadata| metadata.get("time")?.as_str().map(str::to_owned))
            .and_then(|time| OffsetDateTime::parse(&time, &Rfc3339).ok());
        matches!((remote_time, local_time), (Some(remote), Some(local)) if remote > local)
    }

    pub(in crate::app) fn update_installed_version_timestamp(
        &self,
        version: &str,
    ) -> Result<(), String> {
        let root = self
            .installed_version_root(version)
            .ok_or_else(|| tr("rust.error.version-metadata.missing").to_owned())?;
        let path = root
            .join("versions")
            .join(version)
            .join(format!("{version}.json"));
        let bytes = fs::read(&path).map_err(|error| {
            tr("rust.error.version-metadata.read")
                .replace("%0", &path.display().to_string())
                .replace("%1", &error.to_string())
        })?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err(tr("rust.error.version-metadata.too-large").to_owned());
        }
        let mut metadata: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
            tr("rust.error.version-metadata.parse").replace("%0", &error.to_string())
        })?;
        let object = metadata
            .as_object_mut()
            .ok_or_else(|| tr("rust.error.version-metadata.not-object").to_owned())?;
        let timestamp = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|error| {
                tr("rust.error.version-metadata.timestamp").replace("%0", &error.to_string())
            })?;
        object.insert("time".to_owned(), serde_json::Value::String(timestamp));
        let bytes = serde_json::to_vec_pretty(&metadata).map_err(|error| {
            tr("rust.error.version-metadata.serialize").replace("%0", &error.to_string())
        })?;
        fs::write(&path, bytes).map_err(|error| {
            tr("rust.error.version-metadata.write")
                .replace("%0", &path.display().to_string())
                .replace("%1", &error.to_string())
        })
    }

    pub(in crate::app) fn installed_versions(&self) -> Vec<String> {
        let mut versions = Vec::new();
        for root in self.installation_roots() {
            if let Ok(entries) = fs::read_dir(root.join("versions")) {
                for entry in entries.flatten() {
                    if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        if entry.path().join(format!("{name}.json")).is_file() {
                            versions.push(name);
                        }
                    }
                }
            }
        }
        versions.sort();
        versions.dedup();
        versions
    }

    pub(in crate::app) fn available_versions(&self) -> Vec<String> {
        let mut versions: Vec<String> = self
            .version_manifest
            .as_ref()
            .map(|manifest| {
                manifest
                    .versions
                    .iter()
                    .map(|entry| entry.id.clone())
                    .collect()
            })
            .unwrap_or_else(|| {
                FALLBACK_VERSIONS
                    .iter()
                    .map(|value| (*value).to_owned())
                    .collect()
            });
        versions.extend(self.installed_versions());
        let mut seen = HashSet::new();
        versions.retain(|version| seen.insert(version.clone()));
        let installed: HashSet<String> = self.installed_versions().into_iter().collect();
        versions.retain(|version| {
            let installed_version = installed.contains(version);
            let category = self
                .version_manifest
                .as_ref()
                .and_then(|manifest| {
                    manifest
                        .versions
                        .iter()
                        .find(|entry| entry.id == *version)
                        .map(|entry| normalize_catalog_type(&entry.version_type))
                })
                .or_else(|| {
                    self.installed_version_type(version)
                        .map(|kind| normalize_catalog_type(&kind))
                })
                .unwrap_or_else(|| "unknown".to_owned());
            let type_enabled = match category.as_str() {
                "release" | "unknown" | "installed" => true,
                _ => self
                    .settings
                    .version_catalog_types
                    .get(&category)
                    .copied()
                    .unwrap_or_else(|| default_version_type_enabled(&category)),
            };
            type_enabled
                && (self.settings.version_include_remote || installed_version)
                && (!self.settings.version_only_installed || installed_version)
                && (self.settings.version_show_old_releases || !self.is_old_release(version))
        });
        versions
    }

    pub(in crate::app) fn refresh_version_catalog(&mut self) {
        if self.version_catalog_loading {
            return;
        }
        self.start_version_catalog_fetch();
    }

    pub(in crate::app) fn start_version_catalog_fetch(&mut self) {
        if let Some(cancel_requested) = self.version_catalog_cancel.take() {
            cancel_requested.store(true, Ordering::Release);
        }
        let cancel_requested = Arc::new(AtomicBool::new(false));
        self.version_catalog_events = Some(spawn_catalog_fetch(
            self.extra_version_repositories.clone(),
            Arc::clone(&cancel_requested),
        ));
        self.version_catalog_cancel = Some(cancel_requested);
        self.version_catalog_loading = true;
        self.set_logged_status(
            LogLevel::Info,
            LogTarget::VersionCatalog,
            tr("rust.status.catalog.refreshing"),
        );
    }

    pub(in crate::app) fn cancel_version_catalog_fetch(&mut self) {
        if !self.version_catalog_loading {
            return;
        }
        if let Some(cancel_requested) = self.version_catalog_cancel.take() {
            cancel_requested.store(true, Ordering::Release);
        }
        self.version_catalog_events = None;
        self.version_catalog_loading = false;
        self.set_logged_status(
            LogLevel::Warn,
            LogTarget::VersionCatalog,
            tr("rust.status.catalog.cancelled"),
        );
    }

    pub(in crate::app) fn remove_version_from_disk(
        &mut self,
        version: &str,
        remove_libraries: bool,
    ) {
        let version_root = self
            .installed_version_root(version)
            .unwrap_or_else(|| self.game_root_for_version(version));
        match remove_installed_version(
            &version_root,
            version,
            remove_libraries,
            self.settings.screen_width > 0 && self.settings.screen_height > 0,
        ) {
            Ok(removed_libraries) => {
                let invalidated = invalidate_game_data_cache(&version_root, version).unwrap_or(0);
                let message = if remove_libraries && removed_libraries > 0 {
                    tr("rust.status.versions.removed-libraries")
                        .replace("%0", version)
                        .replace("%1", &removed_libraries.to_string())
                } else if invalidated == 0 {
                    tr("rust.status.versions.removed").replace("%0", version)
                } else {
                    tr("rust.status.versions.removed-caches")
                        .replace("%0", version)
                        .replace("%1", &invalidated.to_string())
                };
                self.set_logged_status(LogLevel::Info, LogTarget::VersionManager, message);
            }
            Err(error) => self.set_logged_status(
                LogLevel::Error,
                LogTarget::VersionManager,
                tr("rust.status.versions.remove-error").replace("%0", &error.to_string()),
            ),
        }
        if self.settings.selected_version == version
            && !self
                .installed_versions()
                .iter()
                .any(|installed| installed == version)
        {
            self.settings.selected_version = self
                .available_versions()
                .into_iter()
                .find(|candidate| candidate != version)
                .unwrap_or_else(|| "latest_release".to_owned());
            self.persist();
        }
        self.confirm_remove_version = None;
    }

    pub(in crate::app) fn request_version_install(&mut self, version: String, force_update: bool) {
        if force_update {
            let has_remote_version = self
                .version_manifest
                .as_ref()
                .is_some_and(|manifest| manifest.versions.iter().any(|entry| entry.id == version));
            if !has_remote_version {
                let title = tr("version.manager.downloader.error.title").to_owned();
                let message = tr("version.manager.downloader.error.local").replace("%0", &version);
                self.set_logged_status(LogLevel::Error, LogTarget::VersionManager, message.clone());
                self.launch_error_dialog = Some(LaunchErrorDialog {
                    title,
                    message,
                    details: None,
                });
                return;
            }
            if self.installed_version_root(&version).is_some()
                && !self.installed_version_is_outdated(&version)
            {
                self.version_install_force_confirmation = Some(version);
                return;
            }
        }
        self.start_version_install(version, force_update);
    }

    pub(in crate::app) fn start_update_check(&mut self) {
        if self.update_checking {
            return;
        }
        self.update_check_events = Some(spawn_update_check(self.settings.beta_updates));
        self.update_checking = true;
        self.set_logged_status(LogLevel::Info, LogTarget::Update, tr("update.checking"));
    }

    pub(in crate::app) fn start_update_install(&mut self, release: github_update::Release) {
        if self.update_installing {
            return;
        }
        self.update_install_events = Some(spawn_update_install(release));
        self.update_installing = true;
        self.update_install_error = None;
        record_launcher_log_event(
            &self.launcher_log,
            LogLevel::Info,
            LogTarget::Update,
            &tr("rust.log.update.download"),
        );
    }
}
