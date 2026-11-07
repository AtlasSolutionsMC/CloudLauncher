use minecraft_java_rs_core::models::minecraft::{Authenticator, XboxAccount};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use url::Url;

const CLIENT_ID: &str = "a332a5f6-c4dc-45b6-9fe3-d881490252b2";
const SCOPES: &str = "Xboxlive.signin offline_access";
const AUTHORIZATION_ENDPOINT: &str = "https://login.live.com/oauth20_authorize.srf";
const TOKEN_ENDPOINT: &str = "https://login.live.com/oauth20_token.srf";

#[derive(Debug, Clone)]
pub struct AuthenticatedMicrosoftAccount {
    pub username: String,
    pub uuid: String,
    pub xuid: String,
    pub refresh_token: String,
    pub authenticator: Authenticator,
}

#[derive(Clone)]
pub struct PendingMinecraftProfile {
    access_token: String,
    refresh_token: String,
    xuid: String,
}

pub enum SignInOutcome {
    Authenticated(AuthenticatedMicrosoftAccount),
    ProfileCreationRequired(PendingMinecraftProfile),
}

#[derive(Deserialize)]
struct MicrosoftTokenResponse {
    access_token: String,
    refresh_token: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct XboxTokenResponse {
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: XboxDisplayClaims,
}

#[derive(Deserialize)]
struct XboxDisplayClaims {
    xui: Vec<XboxIdentity>,
}

#[derive(Deserialize)]
struct XboxIdentity {
    uhs: String,
    #[serde(default)]
    xid: Option<String>,
}

#[derive(Deserialize)]
struct MinecraftServiceToken {
    access_token: String,
}

#[derive(Deserialize)]
struct Entitlements {
    items: Vec<Entitlement>,
}

#[derive(Deserialize)]
struct Entitlement {
    name: String,
}

#[derive(Deserialize)]
struct MinecraftProfile {
    id: String,
    name: String,
}

pub async fn sign_in(cancel_requested: Arc<AtomicBool>) -> Result<SignInOutcome, String> {
    let (code, redirect_uri) =
        tokio::task::spawn_blocking(move || request_authorization_code(cancel_requested))
            .await
            .map_err(|error| {
                crate::tr("rust.status.account.microsoft.callback-error")
                    .replace("%0", &error.to_string())
            })??;

    let client = http_client()?;
    let response = client
        .post(TOKEN_ENDPOINT)
        .form(&[
            ("client_id", CLIENT_ID),
            ("code", code.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri.as_str()),
        ])
        .send()
        .await
        .map_err(|error| {
            crate::tr("rust.status.account.microsoft.exchange-error")
                .replace("%0", &error.to_string())
        })?;
    let microsoft_token = decode_response::<MicrosoftTokenResponse>(
        response,
        crate::tr("rust.status.account.microsoft.operation.oauth"),
    )
    .await?;

    authenticate_minecraft(client, microsoft_token).await
}

pub async fn refresh_sign_in(refresh_token: &str) -> Result<AuthenticatedMicrosoftAccount, String> {
    let client = http_client()?;
    let response = client
        .post(TOKEN_ENDPOINT)
        .form(&[
            ("client_id", CLIENT_ID),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|error| {
            crate::tr("rust.status.account.microsoft.refresh-error")
                .replace("%0", &error.to_string())
        })?;
    let microsoft_token = decode_response::<MicrosoftTokenResponse>(
        response,
        crate::tr("rust.status.account.microsoft.operation.token-refresh"),
    )
    .await?;

    match authenticate_minecraft(client, microsoft_token).await? {
        SignInOutcome::Authenticated(account) => Ok(account),
        SignInOutcome::ProfileCreationRequired(_) => {
            Err(crate::tr("rust.status.account.microsoft.profile-missing").to_owned())
        }
    }
}

async fn authenticate_minecraft(
    client: reqwest::Client,
    microsoft_token: MicrosoftTokenResponse,
) -> Result<SignInOutcome, String> {
    let xbox_live: XboxTokenResponse = post_json(
        &client,
        "https://user.auth.xboxlive.com/user/authenticate",
        json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={}", microsoft_token.access_token),
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT",
        }),
        crate::tr("rust.status.account.microsoft.operation.xbox-live"),
    )
    .await?;

    let xsts: XboxTokenResponse = post_json(
        &client,
        "https://xsts.auth.xboxlive.com/xsts/authorize",
        json!({
            "Properties": {
                "SandboxId": "RETAIL",
                "UserTokens": [xbox_live.token],
            },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT",
        }),
        crate::tr("rust.status.account.microsoft.operation.xbox-xsts"),
    )
    .await?;
    let identity =
        xsts.display_claims.xui.first().ok_or_else(|| {
            crate::tr("rust.status.account.microsoft.identity-missing").to_owned()
        })?;

    let minecraft_token: MinecraftServiceToken = post_json(
        &client,
        "https://api.minecraftservices.com/authentication/login_with_xbox",
        json!({
            "identityToken": format!("XBL3.0 x={};{}", identity.uhs, xsts.token),
        }),
        crate::tr("rust.status.account.microsoft.operation.minecraft"),
    )
    .await?;

    let ownership_url = format!(
        "https://api.minecraftservices.com/entitlements/license?requestId={}",
        uuid::Uuid::new_v4()
    );
    let ownership_response = client
        .get(ownership_url)
        .bearer_auth(&minecraft_token.access_token)
        .send()
        .await
        .map_err(|error| {
            crate::tr("rust.status.account.microsoft.ownership-error")
                .replace("%0", &error.to_string())
        })?;
    let ownership: Entitlements = decode_response(
        ownership_response,
        crate::tr("rust.status.account.microsoft.operation.ownership"),
    )
    .await?;
    let has_product = ownership
        .items
        .iter()
        .any(|item| item.name == "product_minecraft");
    let has_game = ownership
        .items
        .iter()
        .any(|item| item.name == "game_minecraft");
    if !has_product || !has_game {
        return Err(crate::tr("rust.status.account.microsoft.not-owned").to_owned());
    }

    let profile_response = client
        .get("https://api.minecraftservices.com/minecraft/profile")
        .bearer_auth(&minecraft_token.access_token)
        .send()
        .await
        .map_err(|error| {
            crate::tr("rust.status.account.microsoft.profile-load-error")
                .replace("%0", &error.to_string())
        })?;
    if profile_response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(SignInOutcome::ProfileCreationRequired(
            PendingMinecraftProfile {
                access_token: minecraft_token.access_token,
                refresh_token: microsoft_token.refresh_token,
                xuid: identity.xid.clone().unwrap_or_default(),
            },
        ));
    }
    let profile: MinecraftProfile = decode_response(
        profile_response,
        crate::tr("rust.status.account.microsoft.operation.profile"),
    )
    .await?;
    Ok(SignInOutcome::Authenticated(build_authenticated_account(
        profile,
        microsoft_token.refresh_token,
        identity.xid.clone().unwrap_or_default(),
        minecraft_token.access_token,
    )?))
}

pub async fn create_minecraft_profile(
    pending: PendingMinecraftProfile,
    profile_name: &str,
) -> Result<AuthenticatedMicrosoftAccount, String> {
    let profile_name = profile_name.trim();
    if !(3..=16).contains(&profile_name.len())
        || !profile_name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err(crate::tr("rust.status.account.microsoft.profile-name-format").to_owned());
    }
    let client = http_client()?;
    let response = client
        .post("https://api.minecraftservices.com/minecraft/profile")
        .bearer_auth(&pending.access_token)
        .json(&json!({ "profileName": profile_name }))
        .send()
        .await
        .map_err(|error| {
            crate::tr("rust.status.account.microsoft.profile-create-error")
                .replace("%0", &error.to_string())
        })?;
    let profile: MinecraftProfile = decode_response(
        response,
        crate::tr("rust.status.account.microsoft.operation.profile-create"),
    )
    .await?;
    build_authenticated_account(
        profile,
        pending.refresh_token,
        pending.xuid,
        pending.access_token,
    )
}

fn build_authenticated_account(
    profile: MinecraftProfile,
    refresh_token: String,
    xuid: String,
    access_token: String,
) -> Result<AuthenticatedMicrosoftAccount, String> {
    let parsed_uuid = uuid::Uuid::parse_str(&profile.id)
        .map_err(|_| crate::tr("rust.status.account.microsoft.invalid-profile-id").to_owned())?;
    let uuid = parsed_uuid.to_string();
    let authenticator = Authenticator {
        access_token,
        name: profile.name.clone(),
        uuid: uuid.clone(),
        xbox_account: Some(XboxAccount { xuid: xuid.clone() }),
        user_properties: None,
        client_id: Some(CLIENT_ID.to_owned()),
        client_token: None,
    };

    Ok(AuthenticatedMicrosoftAccount {
        username: profile.name,
        uuid,
        xuid,
        refresh_token,
        authenticator,
    })
}

async fn post_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    endpoint: &str,
    body: Value,
    operation: &str,
) -> Result<T, String> {
    let response = client
        .post(endpoint)
        .json(&body)
        .send()
        .await
        .map_err(|error| {
            crate::tr("rust.status.account.microsoft.request-error")
                .replace("%0", operation)
                .replace("%1", &error.to_string())
        })?;
    decode_response(response, operation).await
}

async fn decode_response<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
    operation: &str,
) -> Result<T, String> {
    let operation_label = operation;
    let status = response.status();
    if !status.is_success() {
        let body = response.json::<Value>().await.unwrap_or(Value::Null);
        let xerr = body.get("XErr").and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_str().and_then(|code| code.parse().ok()))
        });
        if let Some(code) = xerr {
            let message = match code {
                2_148_916_233 => crate::tr("rust.status.account.microsoft.xbox-no-profile"),
                2_148_916_235 => crate::tr("rust.status.account.microsoft.xbox-region"),
                2_148_916_238 => crate::tr("rust.status.account.microsoft.xbox-child"),
                _ => crate::tr("rust.status.account.microsoft.xbox-rejected"),
            };
            return Err(message.to_owned());
        }
        if let Some(message) = ["errorMessage", "developerMessage", "message", "error"]
            .iter()
            .find_map(|field| body.get(field).and_then(Value::as_str))
        {
            let detail = message.chars().take(240).collect::<String>();
            return Err(crate::tr("rust.status.account.microsoft.http-detail")
                .replace("%0", operation_label)
                .replace("%1", &status.to_string())
                .replace("%2", &detail));
        }
        return Err(crate::tr("rust.status.account.microsoft.http-error")
            .replace("%0", operation_label)
            .replace("%1", &status.to_string()));
    }
    response.json::<T>().await.map_err(|_| {
        crate::tr("rust.status.account.microsoft.response-error").replace("%0", operation_label)
    })
}

fn http_client() -> Result<reqwest::Client, String> {
    crate::http::client_builder()
        .timeout(Duration::from_secs(30))
        .user_agent("LegacyLauncher-Rust/0.1")
        .build()
        .map_err(|error| {
            crate::tr("rust.status.account.microsoft.client-error")
                .replace("%0", &error.to_string())
        })
}

fn request_authorization_code(
    cancel_requested: Arc<AtomicBool>,
) -> Result<(String, String), String> {
    let mut listener_and_port = None;
    for port in [46521, 47522, 48523, 49524] {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)) {
            listener_and_port = Some((listener, port));
            break;
        }
    }
    let (listener, port) = listener_and_port
        .ok_or_else(|| crate::tr("rust.status.account.microsoft.callback-port").to_owned())?;
    listener.set_nonblocking(true).map_err(|error| {
        crate::tr("rust.status.account.microsoft.callback-prepare")
            .replace("%0", &error.to_string())
    })?;

    let redirect_uri = format!("http://localhost:{port}");
    let state = uuid::Uuid::new_v4().simple().to_string();
    let mut login_url = Url::parse(AUTHORIZATION_ENDPOINT).map_err(|error| {
        crate::tr("rust.status.account.microsoft.url-invalid").replace("%0", &error.to_string())
    })?;
    login_url.query_pairs_mut().extend_pairs([
        ("client_id", CLIENT_ID),
        ("response_type", "code"),
        ("redirect_uri", redirect_uri.as_str()),
        ("scope", SCOPES),
        ("state", state.as_str()),
        ("prompt", "select_account"),
    ]);
    crate::open_url(login_url.as_str())?;

    let deadline = Instant::now() + Duration::from_secs(300);
    while Instant::now() < deadline {
        if cancel_requested.load(Ordering::Acquire) {
            return Err(crate::tr("rust.status.account.microsoft.cancelled").to_owned());
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
                let request_target = match read_request_target(&mut stream) {
                    Ok(target) => target,
                    Err(error) => {
                        write_callback_response(&mut stream, false);
                        return Err(error);
                    }
                };
                let callback = Url::parse(&redirect_uri)
                    .and_then(|base| base.join(&request_target))
                    .map_err(|error| {
                        crate::tr("rust.status.account.microsoft.callback-read")
                            .replace("%0", &error.to_string())
                    })?;
                let parameters: std::collections::HashMap<String, String> =
                    callback.query_pairs().into_owned().collect();
                if parameters.get("state").map(String::as_str) != Some(state.as_str()) {
                    write_callback_response(&mut stream, false);
                    continue;
                }
                if let Some(error) = parameters.get("error") {
                    write_callback_response(&mut stream, true);
                    return Err(if error == "access_denied" {
                        crate::tr("rust.status.account.microsoft.cancelled").to_owned()
                    } else {
                        crate::tr("rust.status.account.microsoft.rejected").to_owned()
                    });
                }
                let code = parameters.get("code").cloned().ok_or_else(|| {
                    crate::tr("rust.status.account.microsoft.code-missing").to_owned()
                })?;
                write_callback_response(&mut stream, true);
                return Ok((code, redirect_uri));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => {
                return Err(crate::tr("rust.status.account.microsoft.callback-receive")
                    .replace("%0", &error.to_string()));
            }
        }
    }
    Err(crate::tr("rust.status.account.microsoft.timeout").to_owned())
}

fn read_request_target(stream: &mut TcpStream) -> Result<String, String> {
    let mut request = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];
    loop {
        let count = stream.read(&mut chunk).map_err(|error| {
            crate::tr("rust.status.account.microsoft.request-read")
                .replace("%0", &error.to_string())
        })?;
        if count == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..count]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if request.len() > 16 * 1024 {
            return Err(crate::tr("rust.status.account.microsoft.callback-too-large").to_owned());
        }
    }
    let first_line = String::from_utf8_lossy(&request)
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned();
    first_line
        .split_whitespace()
        .nth(1)
        .filter(|target| target.starts_with('/'))
        .map(str::to_owned)
        .ok_or_else(|| crate::tr("rust.status.account.microsoft.callback-malformed").to_owned())
}

fn write_callback_response(stream: &mut TcpStream, success: bool) {
    let (status, body) = if success {
        (
            "200 OK",
            crate::tr("rust.status.account.microsoft.browser-complete"),
        )
    } else {
        (
            "400 Bad Request",
            crate::tr("rust.status.account.microsoft.browser-invalid"),
        )
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}
