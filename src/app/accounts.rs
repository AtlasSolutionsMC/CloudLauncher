use super::*;

impl LauncherApp {
    pub(in crate::app) fn open_account_dialog(&mut self) {
        if !self.allow_non_minecraft_account() {
            return;
        }
        self.new_account.clear();
        self.new_account_allow_invalid = false;
        self.new_account_ely_skins = true;
        self.editing_account = None;
        self.show_account_dialog = true;
    }

    pub(in crate::app) fn open_edit_account_dialog(&mut self, account: String) {
        self.new_account = account.clone();
        self.new_account_allow_invalid = false;
        self.new_account_ely_skins = offline_ely_skins_for(&self.settings, &account);
        self.editing_account = Some(account);
        self.show_account_dialog = true;
    }

    pub(in crate::app) fn start_microsoft_sign_in(&mut self) {
        if self.microsoft_signin_pending || self.launching {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.microsoft_signin_events = Some(receiver);
        self.microsoft_signin_pending = true;
        self.cancel_requested.store(false, Ordering::Release);
        let cancel_requested = self.cancel_requested.clone();
        self.set_logged_status(
            LogLevel::Info,
            LogTarget::Authentication,
            tr("rust.status.account.microsoft-browser"),
        );
        thread::spawn(move || {
            let result = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime.block_on(async {
                    tokio::select! {
                        result = auth::sign_in(cancel_requested.clone()) => result,
                        _ = wait_for_cancel(cancel_requested) => {
                            Err(tr("rust.status.account.microsoft.cancelled").to_owned())
                        }
                    }
                }),
                Err(error) => Err(tr("rust.status.account.microsoft.runtime-error")
                    .replace("%0", &error.to_string())),
            };
            let _ = sender.send(result);
        });
    }

    pub(in crate::app) fn open_ely_legacy_dialog(&mut self) {
        if !self.allow_non_minecraft_account() {
            return;
        }
        self.ely_legacy_username.clear();
        self.ely_legacy_password.clear();
        self.editing_ely_legacy_account = None;
        self.show_ely_legacy_dialog = true;
    }

    pub(in crate::app) fn open_edit_ely_legacy_dialog(
        &mut self,
        account: &ElyLegacyAccountProfile,
    ) {
        self.ely_legacy_username.clone_from(&account.username);
        self.ely_legacy_password.clear();
        self.editing_ely_legacy_account = Some(account.id.clone());
        self.show_ely_legacy_dialog = true;
    }

    pub(in crate::app) fn open_ely_dialog(&mut self) {
        if !self.allow_non_minecraft_account() {
            return;
        }
        self.show_ely_dialog = true;
    }

    pub(in crate::app) fn allow_non_minecraft_account(&mut self) -> bool {
        if !self.require_minecraft_account || has_licensed_minecraft_account(&self.settings) {
            return true;
        }
        self.show_require_minecraft_account_notice = true;
        false
    }

    pub(in crate::app) fn start_ely_legacy_sign_in(&mut self) {
        if self.ely_legacy_signin_pending || self.launching {
            return;
        }
        let username = self.ely_legacy_username.trim().to_owned();
        let password = self.ely_legacy_password.clone();
        if username.is_empty() || password.is_empty() {
            self.set_logged_status(
                LogLevel::Warn,
                LogTarget::Authentication,
                tr("rust.status.account.ely.enter-credentials"),
            );
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.ely_legacy_signin_events = Some(receiver);
        self.ely_legacy_signin_pending = true;
        self.cancel_requested.store(false, Ordering::Release);
        let cancel_requested = self.cancel_requested.clone();
        self.set_logged_status(
            LogLevel::Info,
            LogTarget::Authentication,
            tr("rust.status.account.ely.signing-in"),
        );
        thread::spawn(move || {
            let result = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime.block_on(async {
                    tokio::select! {
                        result = ely_legacy::authenticate(&username, &password) => result,
                        _ = wait_for_cancel(cancel_requested) => Err(tr("rust.status.account.ely.oauth.cancelled").to_owned()),
                    }
                }),
                Err(error) => Err(tr("rust.status.account.ely.runtime-error")
                    .replace("%0", &error.to_string())),
            };
            let _ = sender.send(result);
        });
    }

    pub(in crate::app) fn save_ely_legacy_account(
        &mut self,
        result: Result<(ely_legacy::Profile, ely_legacy::Credentials), String>,
    ) {
        self.ely_legacy_signin_events = None;
        self.ely_legacy_signin_pending = false;
        match result {
            Ok((profile, credentials)) => {
                let success_kind = if self.editing_ely_legacy_account.is_some() {
                    "success-edit"
                } else {
                    "success-add"
                };
                let account = ElyLegacyAccountProfile {
                    id: format!("ely_legacy:{}", profile.uuid),
                    username: profile.username,
                    display_name: profile.display_name,
                    uuid: profile.uuid,
                };
                let secret = match serde_json::to_string(&credentials) {
                    Ok(secret) => secret,
                    Err(error) => {
                        self.set_logged_status(
                            LogLevel::Error,
                            LogTarget::Authentication,
                            tr("rust.status.account.ely.save-error")
                                .replace("%0", &error.to_string()),
                        );
                        return;
                    }
                };
                let editing_account =
                    self.editing_ely_legacy_account
                        .as_ref()
                        .and_then(|account_id| {
                            self.settings
                                .ely_legacy_accounts
                                .iter()
                                .find(|saved| &saved.id == account_id)
                                .cloned()
                        });
                let mut replaced_ely_accounts = Vec::new();
                for saved in &self.settings.ely_accounts {
                    if saved.id != account.id
                        && (saved.uuid == account.uuid
                            || saved.username.eq_ignore_ascii_case(&account.username))
                    {
                        replaced_ely_accounts.push(saved.id.clone());
                    }
                }
                for saved in &self.settings.ely_legacy_accounts {
                    if saved.id != account.id
                        && (saved.uuid == account.uuid
                            || saved.username.eq_ignore_ascii_case(&account.username)
                            || editing_account
                                .as_ref()
                                .is_some_and(|previous| saved.id == previous.id))
                    {
                        replaced_ely_accounts.push(saved.id.clone());
                    }
                }
                if let Err(error) = vault::store_refresh_token_replacing_accounts(
                    &account.storage_key(),
                    &secret,
                    &replaced_ely_accounts,
                ) {
                    self.set_logged_status(
                        LogLevel::Error,
                        LogTarget::Authentication,
                        tr("rust.status.account.ely.protect-error")
                            .replace("%0", &error.to_string()),
                    );
                    return;
                }
                self.settings.ely_accounts.retain(|saved| {
                    saved.id != account.id && !replaced_ely_accounts.contains(&saved.id)
                });
                self.settings.ely_legacy_accounts.retain(|saved| {
                    saved.id != account.id && !replaced_ely_accounts.contains(&saved.id)
                });
                self.settings.ely_legacy_accounts.push(account.clone());
                self.settings.selected_account = Some(account.id);
                self.show_ely_legacy_dialog = false;
                self.editing_ely_legacy_account = None;
                self.ely_legacy_password.clear();
                self.account_success_kind = Some(success_kind);
                self.set_logged_status(
                    LogLevel::Info,
                    LogTarget::Authentication,
                    tr("rust.status.account.ely.signed-in").replace("%0", &account.display_name),
                );
                self.persist();
            }
            Err(error) => self.set_logged_status(LogLevel::Error, LogTarget::Authentication, error),
        }
    }

    pub(in crate::app) fn start_ely_sign_in(&mut self) {
        if self.ely_signin_pending || self.launching {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.ely_signin_events = Some(receiver);
        self.ely_signin_pending = true;
        self.cancel_requested.store(false, Ordering::Release);
        let cancel_requested = self.cancel_requested.clone();
        self.set_logged_status(
            LogLevel::Info,
            LogTarget::Authentication,
            tr("rust.status.account.ely.browser"),
        );
        thread::spawn(move || {
            let result =
                match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime.block_on(ely::sign_in(cancel_requested.clone())),
                    Err(error) => Err(tr("rust.status.account.ely.runtime-error")
                        .replace("%0", &error.to_string())),
                };
            let _ = sender.send(result);
        });
    }

    pub(in crate::app) fn save_ely_account(
        &mut self,
        result: Result<(ely::Profile, ely::Credentials), String>,
    ) {
        self.ely_signin_events = None;
        self.ely_signin_pending = false;
        match result {
            Ok((profile, credentials)) => {
                let account = ElyAccountProfile {
                    id: format!("ely:{}", profile.uuid),
                    account_id: profile.id,
                    username: profile.username,
                    display_name: profile.display_name,
                    uuid: profile.uuid,
                };
                let secret = match serde_json::to_string(&credentials) {
                    Ok(secret) => secret,
                    Err(error) => {
                        self.set_logged_status(
                            LogLevel::Error,
                            LogTarget::Authentication,
                            tr("rust.status.account.ely.save-error")
                                .replace("%0", &error.to_string()),
                        );
                        return;
                    }
                };
                let replaced_ely_accounts = self
                    .settings
                    .ely_accounts
                    .iter()
                    .filter(|saved| {
                        saved.id != account.id
                            && (saved.uuid == account.uuid
                                || saved.username.eq_ignore_ascii_case(&account.username))
                    })
                    .map(|saved| saved.id.clone())
                    .chain(
                        self.settings
                            .ely_legacy_accounts
                            .iter()
                            .filter(|saved| {
                                saved.uuid == account.uuid
                                    || saved.username.eq_ignore_ascii_case(&account.username)
                            })
                            .map(|saved| saved.id.clone()),
                    )
                    .collect::<Vec<_>>();
                if let Err(error) = vault::store_refresh_token_replacing_accounts(
                    &account.storage_key(),
                    &secret,
                    &replaced_ely_accounts,
                ) {
                    self.set_logged_status(
                        LogLevel::Error,
                        LogTarget::Authentication,
                        tr("rust.status.account.ely.protect-error")
                            .replace("%0", &error.to_string()),
                    );
                    return;
                }
                self.settings.ely_accounts.retain(|saved| {
                    saved.id != account.id && !replaced_ely_accounts.contains(&saved.id)
                });
                self.settings.ely_legacy_accounts.retain(|saved| {
                    saved.id != account.id && !replaced_ely_accounts.contains(&saved.id)
                });
                self.settings.ely_accounts.push(account.clone());
                self.settings.selected_account = Some(account.id);
                self.show_ely_dialog = false;
                self.account_success_kind = Some("success-add");
                self.set_logged_status(
                    LogLevel::Info,
                    LogTarget::Authentication,
                    tr("rust.status.account.ely.signed-in").replace("%0", &account.display_name),
                );
                self.persist();
            }
            Err(error) => self.set_logged_status(LogLevel::Error, LogTarget::Authentication, error),
        }
    }

    pub(in crate::app) fn apply_microsoft_sign_in(
        &mut self,
        result: Result<auth::SignInOutcome, String>,
    ) {
        self.microsoft_signin_pending = false;
        self.microsoft_signin_events = None;
        match result {
            Ok(auth::SignInOutcome::Authenticated(account)) => {
                self.save_microsoft_account(account);
            }
            Ok(auth::SignInOutcome::ProfileCreationRequired(pending)) => {
                self.pending_minecraft_profile = Some(pending);
                self.new_minecraft_profile_name.clear();
                self.show_minecraft_profile_dialog = true;
                self.set_logged_status(
                    LogLevel::Info,
                    LogTarget::Authentication,
                    tr("rust.status.account.microsoft.profile.choose"),
                );
            }
            Err(error) => self.set_logged_status(LogLevel::Error, LogTarget::Authentication, error),
        }
    }

    pub(in crate::app) fn save_microsoft_account(
        &mut self,
        account: auth::AuthenticatedMicrosoftAccount,
    ) {
        let profile = MicrosoftAccountProfile {
            id: format!("microsoft:{}", account.uuid),
            username: account.username.clone(),
            uuid: account.uuid,
            xuid: account.xuid,
        };
        if let Err(error) =
            vault::store_refresh_token(&profile.storage_key(), &account.refresh_token)
        {
            self.set_logged_status(
                LogLevel::Error,
                LogTarget::Authentication,
                tr("rust.status.account.microsoft.save-error").replace("%0", &error.to_string()),
            );
            return;
        }
        self.settings
            .microsoft_accounts
            .retain(|saved| saved.id != profile.id);
        self.settings.microsoft_accounts.push(profile.clone());
        self.settings.selected_account = Some(profile.id);
        self.account_success_kind = Some("success-add");
        self.set_logged_status(
            LogLevel::Info,
            LogTarget::Authentication,
            tr("rust.status.account.microsoft.signed-in").replace("%0", &account.username),
        );
        self.pending_minecraft_profile = None;
        self.show_minecraft_profile_dialog = false;
        self.persist();
    }

    pub(in crate::app) fn start_minecraft_profile_creation(&mut self) {
        if self.profile_creation_pending {
            return;
        }
        let Some(pending) = self.pending_minecraft_profile.clone() else {
            self.show_minecraft_profile_dialog = false;
            self.set_logged_status(
                LogLevel::Warn,
                LogTarget::Authentication,
                tr("rust.status.account.microsoft.profile.restart"),
            );
            return;
        };
        let name = self.new_minecraft_profile_name.trim().to_owned();
        if !(3..=16).contains(&name.len())
            || !name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            self.set_logged_status(
                LogLevel::Warn,
                LogTarget::Authentication,
                tr("rust.status.account.microsoft.profile.invalid-name"),
            );
            return;
        }
        let (sender, receiver) = mpsc::channel();
        self.profile_creation_events = Some(receiver);
        self.profile_creation_pending = true;
        self.set_logged_status(
            LogLevel::Info,
            LogTarget::Authentication,
            tr("rust.status.account.microsoft.profile.creating").replace("%0", &name),
        );
        thread::spawn(move || {
            let result = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime.block_on(auth::create_minecraft_profile(pending, &name)),
                Err(error) => Err(tr("rust.status.account.microsoft.profile-runtime-error")
                    .replace("%0", &error.to_string())),
            };
            let _ = sender.send(result);
        });
    }

    pub(in crate::app) fn selected_account_label(&self) -> String {
        self.settings
            .selected_account
            .as_ref()
            .and_then(|id| {
                self.settings
                    .microsoft_accounts
                    .iter()
                    .find(|account| &account.id == id)
                    .map(|account| account.username.clone())
            })
            .or_else(|| {
                self.settings
                    .ely_accounts
                    .iter()
                    .find(|account| {
                        Some(account.id.as_str()) == self.settings.selected_account.as_deref()
                    })
                    .map(|account| account.display_name.clone())
            })
            .or_else(|| {
                self.settings
                    .ely_legacy_accounts
                    .iter()
                    .find(|account| {
                        Some(account.id.as_str()) == self.settings.selected_account.as_deref()
                    })
                    .map(|account| account.display_name.clone())
            })
            .or_else(|| self.settings.selected_account.clone())
            .unwrap_or_else(|| tr("account.empty").to_owned())
    }

    pub(in crate::app) fn first_account_key(&self) -> Option<String> {
        self.settings
            .accounts
            .first()
            .cloned()
            .or_else(|| {
                self.settings
                    .microsoft_accounts
                    .first()
                    .map(|account| account.id.clone())
            })
            .or_else(|| {
                self.settings
                    .ely_accounts
                    .first()
                    .map(|account| account.id.clone())
            })
            .or_else(|| {
                self.settings
                    .ely_legacy_accounts
                    .first()
                    .map(|account| account.id.clone())
            })
    }
}
