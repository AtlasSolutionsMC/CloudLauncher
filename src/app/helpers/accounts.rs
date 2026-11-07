use super::super::*;

pub(in crate::app) fn import_legacy_accounts(
    path: &std::path::Path,
    settings: &mut SavedSettings,
) -> Result<LegacyImportSummary, String> {
    let bytes = fs::read(path).map_err(|error| {
        tr("rust.error.import.accounts-read")
            .replace("%0", &path.display().to_string())
            .replace("%1", &error.to_string())
    })?;
    let root = serde_json::from_slice::<serde_json::Value>(&bytes)
        .map_err(|error| tr("rust.error.import.accounts-json").replace("%0", &error.to_string()))?;
    let user_set = root.get("userSet").unwrap_or(&root);
    let users = user_set
        .get("list")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| tr("rust.error.import.accounts-list-missing").to_owned())?;
    let selected = user_set.get("selected").and_then(|value| {
        Some((
            value.get("type")?.as_str()?.to_ascii_lowercase(),
            value.get("username")?.as_str()?.to_owned(),
        ))
    });
    let mut summary = LegacyImportSummary::default();
    let mut imported_selected = None;

    for user in users {
        let Some(kind) = user.get("type").and_then(serde_json::Value::as_str) else {
            summary.skipped += 1;
            continue;
        };
        match kind {
            "mojang" => {
                let login_name = user
                    .get("username")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                let Some((username, player_uuid)) = legacy_mojang_profile(user) else {
                    summary.skipped += 1;
                    continue;
                };
                if !valid_player_name(&username) {
                    summary.skipped += 1;
                    continue;
                }
                let already_present = settings
                    .accounts
                    .iter()
                    .any(|saved| saved.eq_ignore_ascii_case(&username));
                if !already_present {
                    settings.accounts.push(username.clone());
                    settings
                        .offline_account_uuids
                        .entry(username.clone())
                        .or_insert(player_uuid);
                    summary.mojang_migrated += 1;
                }
                if !settings
                    .migrated_mojang_accounts
                    .iter()
                    .any(|saved| saved.eq_ignore_ascii_case(&username))
                {
                    settings.migrated_mojang_accounts.push(username.clone());
                }
                if selected
                    .as_ref()
                    .is_some_and(|(selected_kind, selected_name)| {
                        selected_kind == "mojang"
                            && (selected_name.eq_ignore_ascii_case(login_name)
                                || selected_name.eq_ignore_ascii_case(&username))
                    })
                {
                    imported_selected = Some(username);
                }
            }
            "plain" => {
                let Some(username) = user.get("username").and_then(serde_json::Value::as_str)
                else {
                    summary.skipped += 1;
                    continue;
                };
                let username = username.trim();
                if username.is_empty() {
                    summary.skipped += 1;
                    continue;
                }
                let already_present = settings
                    .accounts
                    .iter()
                    .any(|saved| saved.eq_ignore_ascii_case(username));
                if !already_present {
                    settings.accounts.push(username.to_owned());
                    summary.offline += 1;
                    let use_ely_skins = user
                        .get("elySkins")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(true);
                    settings
                        .offline_ely_skins
                        .insert(username.to_owned(), use_ely_skins);
                }
                if let Some(uuid) = user
                    .get("uuid")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|value| uuid::Uuid::parse_str(value).ok())
                {
                    let key = settings
                        .accounts
                        .iter()
                        .find(|saved| saved.eq_ignore_ascii_case(username))
                        .cloned()
                        .unwrap_or_else(|| username.to_owned());
                    settings
                        .offline_account_uuids
                        .entry(key)
                        .or_insert_with(|| uuid.to_string());
                }
                if selected
                    .as_ref()
                    .is_some_and(|(selected_kind, selected_name)| {
                        selected_kind == "plain" && selected_name.eq_ignore_ascii_case(username)
                    })
                {
                    imported_selected = Some(username.to_owned());
                }
            }
            "minecraft" => {
                let Some(username) = user.get("name").and_then(serde_json::Value::as_str) else {
                    summary.skipped += 1;
                    continue;
                };
                let Some(profile_uuid) = user
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|value| uuid::Uuid::parse_str(value).ok())
                else {
                    summary.skipped += 1;
                    continue;
                };
                let Some(refresh_token) = user
                    .pointer("/microsoft/refreshToken")
                    .and_then(serde_json::Value::as_str)
                    .filter(|token| !token.trim().is_empty())
                else {
                    summary.skipped += 1;
                    continue;
                };
                let uuid = profile_uuid.to_string();
                let profile = MicrosoftAccountProfile {
                    id: format!("microsoft:{uuid}"),
                    username: username.to_owned(),
                    uuid,
                    xuid: String::new(),
                };
                vault::store_refresh_token(&profile.storage_key(), refresh_token).map_err(
                    |error| {
                        tr("rust.error.import.microsoft-token").replace("%0", &error.to_string())
                    },
                )?;
                let exists = settings
                    .microsoft_accounts
                    .iter()
                    .any(|saved| saved.id == profile.id);
                settings
                    .microsoft_accounts
                    .retain(|saved| saved.id != profile.id);
                settings.microsoft_accounts.push(profile.clone());
                if !exists {
                    summary.microsoft += 1;
                }
                if selected
                    .as_ref()
                    .is_some_and(|(selected_kind, selected_name)| {
                        selected_kind == "minecraft" && selected_name.eq_ignore_ascii_case(username)
                    })
                {
                    imported_selected = Some(profile.id);
                }
            }
            "ely" => {
                let Some(username) = user
                    .get("username")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    summary.skipped += 1;
                    continue;
                };
                let Some(account_id) = user
                    .get("id")
                    .and_then(serde_json::Value::as_i64)
                    .filter(|value| *value >= 0)
                else {
                    summary.skipped += 1;
                    continue;
                };
                let Some(profile_uuid) = user
                    .get("uuid")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|value| uuid::Uuid::parse_str(value).ok())
                else {
                    summary.skipped += 1;
                    continue;
                };
                let display_name = user
                    .get("displayName")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or(username);
                let Some(access_token) = user
                    .get("accessToken")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                else {
                    summary.skipped += 1;
                    continue;
                };
                let refresh_token = user
                    .get("refreshToken")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .map(str::to_owned);
                let expiry_time = user.get("expiryTime").and_then(serde_json::Value::as_u64);
                let uuid = profile_uuid.to_string();
                let account = ElyAccountProfile {
                    id: format!("ely:{uuid}"),
                    account_id,
                    username: username.to_owned(),
                    display_name: display_name.to_owned(),
                    uuid,
                };
                let credentials = ely::Credentials {
                    access_token: access_token.to_owned(),
                    refresh_token,
                    expiry_time,
                };
                let serialized = serde_json::to_string(&credentials).map_err(|error| {
                    tr("rust.error.import.ely-encode").replace("%0", &error.to_string())
                })?;
                vault::store_refresh_token(&account.storage_key(), &serialized).map_err(
                    |error| tr("rust.error.import.ely-protect").replace("%0", &error.to_string()),
                )?;
                let exists = settings
                    .ely_accounts
                    .iter()
                    .any(|saved| saved.id == account.id);
                settings.ely_accounts.retain(|saved| saved.id != account.id);
                settings.ely_accounts.push(account.clone());
                if !exists {
                    summary.ely += 1;
                }
                if selected
                    .as_ref()
                    .is_some_and(|(selected_kind, selected_name)| {
                        selected_kind == "ely" && selected_name.eq_ignore_ascii_case(username)
                    })
                {
                    imported_selected = Some(account.id);
                }
            }
            "ely_legacy" => {
                let Some(username) = user
                    .get("username")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    summary.skipped += 1;
                    continue;
                };
                let Some(profile_uuid) = user
                    .get("uuid")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|value| uuid::Uuid::parse_str(value).ok())
                else {
                    summary.skipped += 1;
                    continue;
                };
                let Some(display_name) = user
                    .get("displayName")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    summary.skipped += 1;
                    continue;
                };
                let Some(client_token) = user
                    .get("clientToken")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                else {
                    summary.skipped += 1;
                    continue;
                };
                let Some(access_token) = user
                    .get("accessToken")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                else {
                    summary.skipped += 1;
                    continue;
                };
                let uuid = profile_uuid.to_string();
                let profile = ElyLegacyAccountProfile {
                    id: format!("ely_legacy:{uuid}"),
                    username: username.to_owned(),
                    display_name: display_name.to_owned(),
                    uuid,
                };
                let credentials = ely_legacy::Credentials {
                    access_token: access_token.to_owned(),
                    client_token: client_token.to_owned(),
                };
                let serialized = serde_json::to_string(&credentials).map_err(|error| {
                    tr("rust.error.import.ely-encode").replace("%0", &error.to_string())
                })?;
                vault::store_refresh_token(&profile.storage_key(), &serialized).map_err(
                    |error| tr("rust.error.import.ely-protect").replace("%0", &error.to_string()),
                )?;
                let exists = settings
                    .ely_legacy_accounts
                    .iter()
                    .any(|saved| saved.id == profile.id);
                settings
                    .ely_legacy_accounts
                    .retain(|saved| saved.id != profile.id);
                settings.ely_legacy_accounts.push(profile.clone());
                if !exists {
                    summary.ely_legacy += 1;
                }
                if selected
                    .as_ref()
                    .is_some_and(|(selected_kind, selected_name)| {
                        selected_kind == "ely_legacy"
                            && selected_name.eq_ignore_ascii_case(username)
                    })
                {
                    imported_selected = Some(profile.id);
                }
            }
            _ => summary.skipped += 1,
        }
    }

    if let Some(selected) = imported_selected {
        settings.selected_account = Some(selected);
    } else if settings.selected_account.is_none() {
        settings.selected_account = settings.accounts.first().cloned().or_else(|| {
            settings
                .microsoft_accounts
                .first()
                .map(|account| account.id.clone())
                .or_else(|| {
                    settings
                        .ely_legacy_accounts
                        .first()
                        .map(|account| account.id.clone())
                })
                .or_else(|| {
                    settings
                        .ely_accounts
                        .first()
                        .map(|account| account.id.clone())
                })
        });
    }
    Ok(summary)
}
