use super::super::*;

pub(in crate::app) fn spawn_catalog_fetch(
    repositories: Vec<String>,
    cancel_requested: Arc<AtomicBool>,
) -> Receiver<Result<MojangVersionManifest, String>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime.block_on(async {
                tokio::select! {
                    result = catalog::fetch_manifest(&repositories) => Some(result),
                    _ = wait_for_cancel(cancel_requested) => None,
                }
            }),
            Err(error) => Some(Err(
                tr("rust.error.runtime.catalog").replace("%0", &error.to_string())
            )),
        };
        if let Some(result) = result {
            let _ = sender.send(result);
        }
    });
    receiver
}

pub(in crate::app) fn spawn_connectivity_check() -> Receiver<connectivity::Report> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let report = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime.block_on(connectivity::check()),
            Err(error) => connectivity::unavailable_report(
                &tr("rust.error.runtime.connectivity-check").replace("%0", &error.to_string()),
            ),
        };
        let _ = sender.send(report);
    });
    receiver
}

pub(in crate::app) fn spawn_update_check(
    include_prereleases: bool,
) -> Receiver<Result<github_update::CheckResult, String>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| {
                tr("rust.error.runtime.update-check").replace("%0", &error.to_string())
            })
            .and_then(|runtime| {
                runtime.block_on(github_update::check_for_update(include_prereleases))
            });
        let _ = sender.send(result);
    });
    receiver
}

pub(in crate::app) fn spawn_update_install(
    release: github_update::Release,
) -> Receiver<Result<(), String>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| {
                tr("rust.error.runtime.update-install").replace("%0", &error.to_string())
            })
            .and_then(|runtime| runtime.block_on(github_update::download_and_handoff(release)));
        let _ = sender.send(result);
    });
    receiver
}

pub(in crate::app) fn spawn_bootstrap_fetch() -> Receiver<Result<bootstrap::Configuration, String>>
{
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| tr("rust.error.runtime.bootstrap").replace("%0", &error.to_string()))
            .and_then(|runtime| runtime.block_on(bootstrap::fetch()));
        let _ = sender.send(result);
    });
    receiver
}

pub(in crate::app) fn spawn_crash_signature_fetch(
    repositories: Vec<String>,
    launcher_log: Arc<Mutex<VecDeque<LogRecord>>>,
) {
    thread::spawn(move || {
        let result = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| {
                tr("rust.error.runtime.crash-signatures").replace("%0", &error.to_string())
            })
            .and_then(|runtime| {
                runtime.block_on(crash::refresh_external_signatures(&repositories))
            });
        match result {
            Ok(Some(revision)) => record_launcher_log_event(
                &launcher_log,
                LogLevel::Info,
                LogTarget::Crash,
                &tr("rust.log.crash.signatures.updated").replace("%0", &revision.to_string()),
            ),
            Ok(None) => record_launcher_log_event(
                &launcher_log,
                LogLevel::Debug,
                LogTarget::Crash,
                &tr("rust.log.crash.signatures.current"),
            ),
            Err(error) => record_launcher_log_event(
                &launcher_log,
                LogLevel::Debug,
                LogTarget::Crash,
                &tr("rust.log.crash.signatures.failed").replace("%0", &error),
            ),
        }
    });
}

pub(in crate::app) fn spawn_diagnostic_upload(report: String) -> Receiver<Result<String, String>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| tr("rust.error.runtime.diagnostics").replace("%0", &error.to_string()))
            .and_then(|runtime| runtime.block_on(support::upload_diagnostic_report(report)));
        let _ = sender.send(result);
    });
    receiver
}

pub(in crate::app) fn spawn_notice_fetch(
    client_id: String,
) -> Receiver<Result<Vec<notices::Notice>, String>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| tr("rust.error.runtime.notices").replace("%0", &error.to_string()))
            .and_then(|runtime| runtime.block_on(notices::fetch(&client_id)));
        let _ = sender.send(result);
    });
    receiver
}

pub(in crate::app) fn unix_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

pub(in crate::app) async fn wait_for_cancel(cancel_requested: Arc<AtomicBool>) {
    while !cancel_requested.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

pub(in crate::app) fn make_launch_options(
    version: String,
    username: String,
    path: PathBuf,
    force_update: bool,
    preferences: SavedSettings,
) -> LaunchOptions {
    make_launch_options_with_authenticator(
        version,
        offline_authenticator(username),
        path,
        force_update,
        preferences,
        None,
    )
}

pub(in crate::app) fn make_launch_options_with_authenticator(
    version: String,
    authenticator: Authenticator,
    path: PathBuf,
    force_update: bool,
    preferences: SavedSettings,
    release_time: Option<&str>,
) -> LaunchOptions {
    let instance = instance_for_version(&version, &preferences);
    let memory = memory_config(&preferences, &version, release_time);
    LaunchOptions {
        path,
        version,
        authenticator,
        timeout_secs: 30,
        download_concurrency: 8,
        verify_concurrency: 4,
        memory,
        java: JavaOptions {
            path: if preferences.java_path.trim().is_empty() {
                None
            } else {
                Some(PathBuf::from(preferences.java_path.trim()))
            },
            runtime_dir: if preferences.java_runtime_directory.trim().is_empty() {
                None
            } else {
                Some(PathBuf::from(
                    preferences.java_runtime_directory.trim().to_owned(),
                ))
            },
            optimized_args_type: preferences.optimized_java_args.clone(),
            use_current_trust_store: preferences.use_current_trust_store,
            wrapper_command: if preferences.wrapper_command.trim().is_empty() {
                None
            } else {
                Some(preferences.wrapper_command.clone())
            },
            ..JavaOptions::default()
        },
        loader: loader_config(&preferences),
        screen: ScreenConfig {
            width: Some(preferences.screen_width.clamp(320, 7680)),
            height: Some(preferences.screen_height.clamp(240, 4320)),
            fullscreen: preferences.fullscreen,
        },
        verify: force_update,
        game_args: split_arguments(&preferences.game_args),
        jvm_args: split_arguments(&preferences.jvm_args),
        instance,
        url: None,
        mcp: None,
        library_replace_type: None,
        library_replace_types: vec![],
        intel_enabled_mac: false,
        bypass_offline: false,
        skip_bundle_check: !force_update,
        force_ipv4: false,
        dns: None,
    }
}

pub(in crate::app) fn append_legacy_experiment_arguments(
    options: &mut LaunchOptions,
    experiments: &HashSet<String>,
    java_major: Option<u32>,
) -> Option<String> {
    if options.java.optimized_args_type != "none" {
        if experiments.contains("max_xms") {
            options.jvm_args.push(format!("-Xms{}", options.memory.max));
        }
        if experiments.contains("more_gc_threads") {
            let threads = std::thread::available_parallelism()
                .map(|count| count.get() / 2)
                .unwrap_or(1)
                .max(1);
            options
                .jvm_args
                .push(format!("-XX:ConcGCThreads={threads}"));
        }
        if experiments.contains("tenuring") {
            options
                .jvm_args
                .push("-XX:MaxTenuringThreshold=1".to_owned());
            options.jvm_args.push("-XX:SurvivorRatio=32".to_owned());
        }
    }
    if experiments.contains("zgc_generational") && options.java.optimized_args_type == "zgc" {
        match java_major {
            Some(21..=22) => {
                options.jvm_args.push("-XX:+ZGenerational".to_owned());
            }
            Some(major) => {
                return Some(
                    tr("rust.status.experiment.zgc-generational-unavailable")
                        .replace("%0", &major.to_string()),
                );
            }
            None => {
                return Some(
                    tr("rust.status.experiment.zgc-generational-unavailable")
                        .replace("%0", "unknown"),
                );
            }
        }
    }
    None
}

pub(in crate::app) fn append_promoted_server(
    servers: &mut Vec<SavedServer>,
    version: &str,
    family: Option<&str>,
    label: &str,
    address: &str,
    families: &[String],
) {
    if !families.is_empty()
        && !families
            .iter()
            .any(|candidate| candidate == version || Some(candidate.as_str()) == family)
    {
        return;
    }
    let name = label.strip_prefix("Join ").unwrap_or(label).trim();
    if !valid_server_name(name) || parse_server_address(address).is_none() {
        return;
    }
    if servers.iter().any(|server| {
        parse_server_address(&server.address)
            .zip(parse_server_address(address))
            .is_some_and(|((_, _, left), (_, _, right))| left.eq_ignore_ascii_case(&right))
    }) {
        return;
    }
    servers.push(SavedServer {
        name: if name.starts_with("§r") {
            name.to_owned()
        } else {
            format!("§r{name}")
        },
        address: address.to_owned(),
        ..Default::default()
    });
}

pub(in crate::app) fn legacy_promoted_version_family(version: &str) -> Option<String> {
    let bytes = version.as_bytes();
    let mut cursor = 0;
    while bytes.get(cursor).is_some_and(u8::is_ascii_lowercase) {
        cursor += 1;
    }
    let major_start = cursor;
    while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    if cursor == major_start || bytes.get(cursor) != Some(&b'.') {
        return None;
    }
    cursor += 1;
    let minor_start = cursor;
    while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    if cursor == minor_start {
        return None;
    }
    let family = &version[..cursor];
    (family != "26.1").then(|| family.to_owned())
}

pub(in crate::app) fn valid_server_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty() && name.chars().count() <= 64 && !name.chars().any(char::is_control)
}

pub(in crate::app) fn parse_server_address(address: &str) -> Option<(String, u16, String)> {
    let address = address.trim();
    if address.is_empty()
        || address.len() > 255
        || address.chars().any(|character| {
            character.is_whitespace() || character.is_control() || matches!(character, '/' | '\\')
        })
    {
        return None;
    }

    let (host, port) = if let Some(ipv6) = address.strip_prefix('[') {
        let (host, suffix) = ipv6.split_once(']')?;
        if host.is_empty() || !host.contains(':') {
            return None;
        }
        let port = if suffix.is_empty() {
            25565
        } else {
            suffix.strip_prefix(':')?.parse::<u16>().ok()?
        };
        if port == 0 {
            return None;
        }
        (format!("[{host}]"), port)
    } else if address.matches(':').count() == 1 {
        let (host, port) = address.rsplit_once(':')?;
        let port = port.parse::<u16>().ok()?;
        if host.is_empty() || port == 0 {
            return None;
        }
        (host.to_owned(), port)
    } else if address.contains(':') {
        (format!("[{address}]"), 25565)
    } else {
        (address.to_owned(), 25565)
    };

    let raw_host = host.trim_matches(['[', ']']);
    if raw_host.is_empty()
        || !raw_host.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_' | ':')
        })
    {
        return None;
    }
    let full_address = if port == 25565 {
        host.clone()
    } else {
        format!("{host}:{port}")
    };
    Some((host, port, full_address))
}

pub(in crate::app) fn selected_version_supports_quick_play(
    version: &str,
    game_root: &std::path::Path,
) -> bool {
    let version_json = game_root
        .join("versions")
        .join(version)
        .join(format!("{version}.json"));
    if let Ok(bytes) = fs::read(version_json)
        && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
    {
        let supported = value
            .get("arguments")
            .and_then(|arguments| arguments.get("game"))
            .and_then(serde_json::Value::as_array)
            .is_some_and(|arguments| {
                arguments.iter().any(|argument| {
                    argument
                        .get("rules")
                        .and_then(serde_json::Value::as_array)
                        .is_some_and(|rules| {
                            rules.iter().any(|rule| {
                                rule.get("features")
                                    .and_then(|features| features.get("is_quick_play_multiplayer"))
                                    .and_then(serde_json::Value::as_bool)
                                    == Some(true)
                            })
                        })
                })
            });
        return supported;
    }

    let mut parts = version.split('.');
    let major = parts.next().and_then(|value| value.parse::<u32>().ok());
    let minor = parts.next().and_then(|value| value.parse::<u32>().ok());
    let patch = parts
        .next()
        .and_then(|value| {
            value
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse::<u32>()
                .ok()
        })
        .unwrap_or_default();
    match (major, minor) {
        (Some(major), _) if major > 1 => true,
        (Some(1), Some(minor)) if minor > 20 => true,
        (Some(1), Some(20)) => patch >= 5,
        _ => false,
    }
}

pub(in crate::app) fn append_server_launch_arguments(
    options: &mut LaunchOptions,
    address: &str,
    supports_quick_play: bool,
) {
    let Some((host, port, full_address)) = parse_server_address(address) else {
        return;
    };
    if supports_quick_play {
        options
            .game_args
            .extend(["--quickPlayMultiplayer".to_owned(), full_address]);
    } else {
        options.game_args.extend(["--server".to_owned(), host]);
        if port != 25565 {
            options
                .game_args
                .extend(["--port".to_owned(), port.to_string()]);
        }
    }
}

pub(in crate::app) fn read_minecraft_server_list(
    path: &std::path::Path,
) -> Result<Vec<SavedServer>, String> {
    const MAX_COMPRESSED_BYTES: u64 = 8 * 1024 * 1024;
    const MAX_NBT_BYTES: u64 = 16 * 1024 * 1024;
    let compressed = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(tr("rust.error.servers.read")
                .replace("%0", &path.display().to_string())
                .replace("%1", &error.to_string()));
        }
    };
    if compressed.len() as u64 > MAX_COMPRESSED_BYTES {
        return Err(tr("rust.error.servers.compressed-large").to_owned());
    }
    let decoder = flate2::read::GzDecoder::new(compressed.as_slice());
    let mut nbt_bytes = Vec::new();
    decoder
        .take(MAX_NBT_BYTES + 1)
        .read_to_end(&mut nbt_bytes)
        .map_err(|error| {
            tr("rust.error.servers.decompress")
                .replace("%0", &path.display().to_string())
                .replace("%1", &error.to_string())
        })?;
    if nbt_bytes.len() as u64 > MAX_NBT_BYTES {
        return Err(tr("rust.error.servers.decompressed-large").to_owned());
    }
    let file: MinecraftServerList = fastnbt::from_bytes(&nbt_bytes).map_err(|error| {
        tr("rust.error.servers.parse")
            .replace("%0", &path.display().to_string())
            .replace("%1", &error.to_string())
    })?;
    Ok(file
        .servers
        .into_iter()
        .filter(|server| {
            valid_server_name(&server.name) && parse_server_address(&server.address).is_some()
        })
        .map(|server| SavedServer {
            name: server.name,
            address: server.address,
            hide_address: server.hide_address,
            accept_textures: server.accept_textures,
        })
        .collect())
}

pub(in crate::app) fn write_minecraft_server_list(
    path: &std::path::Path,
    servers: &[SavedServer],
) -> Result<(), String> {
    let file = MinecraftServerList {
        servers: servers
            .iter()
            .map(|server| MinecraftServerRecord {
                name: server.name.clone(),
                address: server.address.clone(),
                hide_address: server.hide_address,
                accept_textures: server.accept_textures,
            })
            .collect(),
    };
    let nbt_bytes = fastnbt::to_bytes(&file)
        .map_err(|error| tr("rust.error.servers.encode").replace("%0", &error.to_string()))?;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(&nbt_bytes)
        .map_err(|error| tr("rust.error.servers.compress").replace("%0", &error.to_string()))?;
    let compressed = encoder.finish().map_err(|error| {
        tr("rust.error.servers.compress-finish").replace("%0", &error.to_string())
    })?;

    let parent = path
        .parent()
        .ok_or_else(|| tr("rust.error.servers.parent-missing").to_owned())?;
    fs::create_dir_all(parent).map_err(|error| {
        tr("rust.error.servers.directory")
            .replace("%0", &parent.display().to_string())
            .replace("%1", &error.to_string())
    })?;
    let temporary_path = path_with_suffix(path, ".tmp");
    let backup_path = path_with_suffix(path, ".bak");
    fs::write(&temporary_path, compressed).map_err(|error| {
        tr("rust.error.servers.write-temp")
            .replace("%0", &temporary_path.display().to_string())
            .replace("%1", &error.to_string())
    })?;

    let had_original = path.is_file();
    if had_original {
        if let Err(error) = fs::copy(path, &backup_path) {
            let _ = fs::remove_file(&temporary_path);
            return Err(tr("rust.error.servers.backup")
                .replace("%0", &path.display().to_string())
                .replace("%1", &error.to_string()));
        }
        if let Err(error) = fs::remove_file(path) {
            let _ = fs::remove_file(&temporary_path);
            return Err(tr("rust.error.servers.replace")
                .replace("%0", &path.display().to_string())
                .replace("%1", &error.to_string()));
        }
    }
    if let Err(error) = fs::rename(&temporary_path, path) {
        if had_original {
            let _ = fs::copy(&backup_path, path);
        }
        let _ = fs::remove_file(&temporary_path);
        return Err(tr("rust.error.servers.install")
            .replace("%0", &path.display().to_string())
            .replace("%1", &error.to_string()));
    }
    Ok(())
}

pub(in crate::app) fn prepare_minecraft_server_list(
    path: &std::path::Path,
    quick_play_server: Option<&SavedServer>,
    promoted_servers: &[SavedServer],
    add_promoted_servers: bool,
) -> Result<(), String> {
    let mut existing = read_minecraft_server_list(path).unwrap_or_default();
    // The Java launcher tags its generated entries with the reset formatting
    // code so stale promoted records can be removed before rebuilding the list.
    existing.retain(|server| !server.name.starts_with("§r"));

    let mut merged = Vec::new();
    let mut seen_addresses = HashSet::new();
    let mut add_once = |server: SavedServer| {
        if let Some((_, _, normalized)) = parse_server_address(&server.address) {
            if seen_addresses.insert(normalized.to_ascii_lowercase()) {
                merged.push(server);
            }
        }
    };

    if let Some(server) = quick_play_server {
        add_once(server.clone());
    }
    if add_promoted_servers {
        for promoted in promoted_servers {
            let Some((_, _, address)) = parse_server_address(&promoted.address) else {
                continue;
            };
            if let Some(index) = existing.iter().position(|saved| {
                parse_server_address(&saved.address).is_some_and(|(_, _, saved_address)| {
                    saved_address.eq_ignore_ascii_case(&address)
                })
            }) {
                add_once(existing.remove(index));
            } else {
                add_once(promoted.clone());
            }
        }
    }
    for server in existing {
        add_once(server);
    }
    write_minecraft_server_list(path, &merged)
}

pub(in crate::app) fn path_with_suffix(path: &std::path::Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

pub(in crate::app) fn instance_for_version(
    version: &str,
    preferences: &SavedSettings,
) -> Option<String> {
    let loader = match preferences.loader_type.as_str() {
        "forge" | "neoforge" | "fabric" | "legacyfabric" | "quilt" => {
            preferences.loader_type.as_str()
        }
        _ => return None,
    };
    let version = safe_path_component(version, 48);
    let build = safe_path_component(&preferences.loader_build, 48);
    let build = if build.is_empty() {
        "latest".to_owned()
    } else {
        build
    };
    Some(format!("{version}-{loader}-{build}"))
}

pub(in crate::app) fn safe_path_component(value: &str, max_chars: usize) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || "._+-".contains(*character))
        .take(max_chars)
        .collect()
}

pub(in crate::app) fn offline_authenticator(username: String) -> Authenticator {
    offline_authenticator_with_uuid(username, None)
}

pub(in crate::app) fn offline_authenticator_with_uuid(
    username: String,
    preserved_uuid: Option<&str>,
) -> Authenticator {
    Authenticator {
        access_token: "offline".to_owned(),
        name: username.clone(),
        uuid: preserved_uuid
            .and_then(|value| uuid::Uuid::parse_str(value).ok())
            .map(|value| value.to_string())
            .unwrap_or_else(|| offline_uuid(&username)),
        xbox_account: None,
        user_properties: None,
        client_id: None,
        client_token: None,
    }
}

pub(in crate::app) fn offline_account_uuid_for(
    settings: &SavedSettings,
    username: &str,
) -> Option<String> {
    settings
        .offline_account_uuids
        .iter()
        .find(|(saved, _)| saved.eq_ignore_ascii_case(username))
        .map(|(_, uuid)| uuid.clone())
}

pub(in crate::app) fn has_licensed_minecraft_account(settings: &SavedSettings) -> bool {
    !settings.microsoft_accounts.is_empty()
        || settings.migrated_mojang_accounts.iter().any(|legacy_name| {
            settings
                .accounts
                .iter()
                .any(|saved| saved.eq_ignore_ascii_case(legacy_name))
        })
}

pub(in crate::app) fn offline_ely_skins_for(settings: &SavedSettings, username: &str) -> bool {
    settings
        .offline_ely_skins
        .iter()
        .find(|(saved, _)| saved.eq_ignore_ascii_case(username))
        .map(|(_, enabled)| *enabled)
        .unwrap_or(true)
}

pub(in crate::app) async fn authenticate_microsoft_account(
    account: &MicrosoftAccountProfile,
) -> Result<Authenticator, String> {
    let refresh_token = vault::load_refresh_token(&account.storage_key())
        .map_err(|error| {
            tr("rust.status.account.microsoft.credentials-read").replace("%0", &error.to_string())
        })?
        .ok_or_else(|| tr("rust.status.account.microsoft.credentials-missing").to_owned())?;
    let authenticated = auth::refresh_sign_in(&refresh_token).await?;
    vault::store_refresh_token(&account.storage_key(), &authenticated.refresh_token).map_err(
        |error| {
            tr("rust.status.account.microsoft.credentials-save").replace("%0", &error.to_string())
        },
    )?;
    Ok(authenticated.authenticator)
}

pub(in crate::app) async fn authenticate_ely_legacy_account(
    account: &ElyLegacyAccountProfile,
) -> Result<Authenticator, String> {
    let stored = vault::load_refresh_token(&account.storage_key())
        .map_err(|error| {
            tr("rust.status.account.ely.credentials-read").replace("%0", &error.to_string())
        })?
        .ok_or_else(|| tr("rust.status.account.ely.credentials-missing").to_owned())?;
    let credentials: ely_legacy::Credentials = serde_json::from_str(&stored).map_err(|error| {
        tr("rust.status.account.ely.credentials-invalid").replace("%0", &error.to_string())
    })?;
    let (profile, refreshed) = ely_legacy::refresh(&account.username, &credentials).await?;
    let serialized = serde_json::to_string(&refreshed).map_err(|error| {
        tr("rust.status.account.ely.credentials-encode").replace("%0", &error.to_string())
    })?;
    vault::store_refresh_token(&account.storage_key(), &serialized).map_err(|error| {
        tr("rust.status.account.ely.credentials-save").replace("%0", &error.to_string())
    })?;

    // LegacyLauncher's ElyLegacyUser exposes Ely's display name as its auth XUID and
    // reports auth type "msa". The core derives that type from xbox_account presence.
    Ok(Authenticator {
        access_token: refreshed.access_token,
        name: profile.display_name.clone(),
        uuid: profile.uuid,
        xbox_account: Some(XboxAccount {
            xuid: profile.display_name,
        }),
        user_properties: None,
        client_id: None,
        client_token: Some(refreshed.client_token),
    })
}

pub(in crate::app) async fn authenticate_ely_account(
    account: &ElyAccountProfile,
) -> Result<Authenticator, String> {
    let stored = vault::load_refresh_token(&account.storage_key())
        .map_err(|error| {
            tr("rust.status.account.ely.credentials-read").replace("%0", &error.to_string())
        })?
        .ok_or_else(|| tr("rust.status.account.ely.credentials-missing").to_owned())?;
    let credentials: ely::Credentials = serde_json::from_str(&stored).map_err(|error| {
        tr("rust.status.account.ely.credentials-invalid").replace("%0", &error.to_string())
    })?;
    let (profile, refreshed) = ely::refresh(&credentials).await?;
    let serialized = serde_json::to_string(&refreshed).map_err(|error| {
        tr("rust.status.account.ely.credentials-encode").replace("%0", &error.to_string())
    })?;
    vault::store_refresh_token(&account.storage_key(), &serialized).map_err(|error| {
        tr("rust.status.account.ely.credentials-save").replace("%0", &error.to_string())
    })?;
    Ok(ely::authenticator(profile, refreshed))
}

pub(in crate::app) fn launch_event_message(event: LaunchEvent) -> Option<String> {
    match event {
        LaunchEvent::Progress {
            downloaded,
            total,
            kind,
        } => {
            let percent = if total == 0 {
                0
            } else {
                downloaded.saturating_mul(100) / total
            };
            Some(
                tr("rust.status.download.progress")
                    .replace("%0", &kind)
                    .replace("%1", &percent.to_string()),
            )
        }
        LaunchEvent::Check {
            current,
            total,
            kind,
        } => Some(
            tr("rust.status.download.check")
                .replace("%0", &kind)
                .replace("%1", &current.to_string())
                .replace("%2", &total.to_string()),
        ),
        LaunchEvent::GameDownloadFinished => Some(tr("rust.status.download.ready").to_owned()),
        LaunchEvent::Extract(name) => Some(tr("rust.status.download.extract").replace("%0", &name)),
        LaunchEvent::Patch(message) => Some(message),
        LaunchEvent::Close(code) => {
            Some(tr("rust.status.download.game-closed").replace("%0", &code.to_string()))
        }
        LaunchEvent::Error(message) => {
            Some(tr("rust.status.download.warning").replace("%0", &message))
        }
        LaunchEvent::Speed(bytes_per_second) => Some(
            tr("rust.status.download.speed").replace("%0", &format_data_rate(bytes_per_second)),
        ),
        LaunchEvent::Estimated(seconds) => {
            Some(tr("rust.status.download.eta").replace("%0", &format_duration(seconds)))
        }
        LaunchEvent::Command(_) => None,
        LaunchEvent::Data(_) => None,
    }
}

pub(in crate::app) fn launch_event_message_or_record(
    buffer: &Arc<Mutex<VecDeque<String>>>,
    launcher_log: &Arc<Mutex<VecDeque<LogRecord>>>,
    event: LaunchEvent,
    show_full_game_command: bool,
) -> Option<LaunchUiEvent> {
    match event {
        LaunchEvent::Command(line) if show_full_game_command => {
            record_launcher_log_event(
                launcher_log,
                LogLevel::Debug,
                LogTarget::MinecraftCommand,
                &line,
            );
            record_game_log_line(buffer, line);
            None
        }
        LaunchEvent::Command(_) => None,
        LaunchEvent::Data(line) => {
            record_launcher_log_event(launcher_log, LogLevel::Trace, LogTarget::Minecraft, &line);
            record_game_log_line(buffer, line);
            None
        }
        event => launch_event_ui_message(event).map(LaunchUiEvent::Message),
    }
}

pub(in crate::app) enum MalwareScanOutcome {
    Clean,
    Skipped,
    Cancelled,
    Infected(malware::MalwareDetection),
}

pub(in crate::app) async fn scan_mods_before_launch(
    mods_directory: &Path,
    cancel_requested: Arc<AtomicBool>,
    sender: &mpsc::Sender<LaunchUiEvent>,
) -> MalwareScanOutcome {
    send_launch_message(
        sender,
        LogLevel::Info,
        LogTarget::Mods,
        tr("launcher.step.jarscanner"),
    );

    let scanner_cancel = Arc::new(AtomicBool::new(false));
    let scan_directory = mods_directory.to_path_buf();
    let worker_cancel = Arc::clone(&scanner_cancel);
    let mut scan_task =
        tokio::task::spawn_blocking(move || malware::scan_mods(&scan_directory, &worker_cancel));
    let prompt_delay = tokio::time::sleep(Duration::from_secs(60));
    tokio::pin!(prompt_delay);

    loop {
        tokio::select! {
            result = &mut scan_task => {
                return finish_malware_scan(result, sender);
            }
            _ = wait_for_cancel(Arc::clone(&cancel_requested)) => {
                scanner_cancel.store(true, Ordering::Release);
                let _ = scan_task.await;
                return MalwareScanOutcome::Cancelled;
            }
            _ = &mut prompt_delay => {
                let (reply, answer) = tokio::sync::oneshot::channel();
                if sender.send(LaunchUiEvent::LongMalwareScanConfirmation(reply)).is_err() {
                    scanner_cancel.store(true, Ordering::Release);
                    let _ = scan_task.await;
                    return MalwareScanOutcome::Cancelled;
                }
                let continue_scanning = tokio::select! {
                    answer = answer => answer.unwrap_or(false),
                    _ = wait_for_cancel(Arc::clone(&cancel_requested)) => {
                        scanner_cancel.store(true, Ordering::Release);
                        let _ = scan_task.await;
                        return MalwareScanOutcome::Cancelled;
                    }
                };
                if !continue_scanning {
                    scanner_cancel.store(true, Ordering::Release);
                    let _ = scan_task.await;
                    send_launch_message(
                        sender,
                        LogLevel::Info,
                        LogTarget::Mods,
                        tr("rust.status.malware.skipped"),
                    );
                    return MalwareScanOutcome::Skipped;
                }
                prompt_delay.as_mut().reset(
                    tokio::time::Instant::now() + Duration::from_secs(10 * 60),
                );
            }
        }
    }
}

pub(in crate::app) fn finish_malware_scan(
    result: Result<std::io::Result<malware::ScanResult>, tokio::task::JoinError>,
    sender: &mpsc::Sender<LaunchUiEvent>,
) -> MalwareScanOutcome {
    let report = match result {
        Ok(Ok(report)) => report,
        Ok(Err(error)) => {
            send_launch_message(
                sender,
                LogLevel::Warn,
                LogTarget::Mods,
                tr("rust.status.malware.scan-error").replace("%0", &error.to_string()),
            );
            return MalwareScanOutcome::Skipped;
        }
        Err(error) => {
            send_launch_message(
                sender,
                LogLevel::Warn,
                LogTarget::Mods,
                tr("rust.status.malware.scan-error").replace("%0", &error.to_string()),
            );
            return MalwareScanOutcome::Skipped;
        }
    };

    if report.cancelled {
        return MalwareScanOutcome::Cancelled;
    }
    if report.unreadable_archives > 0 {
        send_launch_message(
            sender,
            LogLevel::Warn,
            LogTarget::Mods,
            tr("rust.status.malware.archive-errors")
                .replace("%0", &report.unreadable_archives.to_string()),
        );
    }
    if report.malformed_classes > 0 {
        send_launch_message(
            sender,
            LogLevel::Warn,
            LogTarget::Mods,
            tr("rust.status.malware.class-errors")
                .replace("%0", &report.malformed_classes.to_string()),
        );
    }
    if report.oversized_classes > 0 {
        send_launch_message(
            sender,
            LogLevel::Warn,
            LogTarget::Mods,
            tr("rust.status.malware.large-classes")
                .replace("%0", &report.oversized_classes.to_string()),
        );
    }
    if let Some(detection) = report.detection {
        return MalwareScanOutcome::Infected(detection);
    }

    send_launch_message(
        sender,
        LogLevel::Info,
        LogTarget::Mods,
        tr("rust.status.malware.clean"),
    );
    MalwareScanOutcome::Clean
}

pub(in crate::app) fn send_launch_message(
    sender: &mpsc::Sender<LaunchUiEvent>,
    level: LogLevel,
    target: LogTarget,
    message: impl Into<String>,
) {
    let _ = sender.send(LaunchUiEvent::Message(LaunchUiMessage {
        message: message.into(),
        level,
        target,
    }));
}

pub(in crate::app) fn send_launch_error_dialog(
    sender: &mpsc::Sender<LaunchUiEvent>,
    title: &str,
    message: impl Into<String>,
    details: Option<String>,
) {
    let _ = sender.send(LaunchUiEvent::ErrorDialog(LaunchErrorDialog {
        title: title.to_owned(),
        message: message.into(),
        details,
    }));
}

pub(in crate::app) fn launch_event_ui_message(event: LaunchEvent) -> Option<LaunchUiMessage> {
    let (level, target) = match &event {
        LaunchEvent::Progress { .. }
        | LaunchEvent::Check { .. }
        | LaunchEvent::Speed(_)
        | LaunchEvent::Estimated(_) => (LogLevel::Debug, LogTarget::MinecraftDownload),
        LaunchEvent::GameDownloadFinished | LaunchEvent::Extract(_) => {
            (LogLevel::Info, LogTarget::MinecraftDownload)
        }
        LaunchEvent::Patch(_) => (LogLevel::Info, LogTarget::Mods),
        LaunchEvent::Close(code) if *code == 0 => (LogLevel::Info, LogTarget::Minecraft),
        LaunchEvent::Close(_) => (LogLevel::Error, LogTarget::Minecraft),
        LaunchEvent::Error(_) => (LogLevel::Error, LogTarget::MinecraftDownload),
        LaunchEvent::Command(_) | LaunchEvent::Data(_) => return None,
    };
    let message = launch_event_message(event)?;
    Some(LaunchUiMessage {
        message,
        level,
        target,
    })
}

pub(in crate::app) fn launch_action_label_key(action: &str) -> &'static str {
    match action {
        "hide" => "settings.launch-action.hide",
        "exit" => "settings.launch-action.exit",
        _ => "settings.launch-action.nothing",
    }
}
