use minecraft_java_rs_core::models::minecraft::{Authenticator, XboxAccount};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use url::Url;

const CLIENT_ID: &str = "tlauncher";
const TOKEN_ENDPOINT: &str = "https://account.ely.by/api/oauth2/v1/token";
const ACCOUNT_INFO_ENDPOINT: &str = "https://account.ely.by/api/account/v1/info";
const CALLBACK_SUCCESS: &str =
    "https://account.ely.by/oauth2/code/success?appName=Legacy%20Launcher";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credentials {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expiry_time: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Profile {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub uuid: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    token_type: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountInfo {
    id: i64,
    username: String,
    display_name: Option<String>,
    uuid: String,
}

pub async fn sign_in(cancel_requested: Arc<AtomicBool>) -> Result<(Profile, Credentials), String> {
    let client_secret = load_client_secret()?;
    let callback =
        tokio::task::spawn_blocking(move || request_authorization_code(cancel_requested))
            .await
            .map_err(|error| {
                crate::tr("rust.status.account.ely.oauth.callback-error")
                    .replace("%0", &error.to_string())
            })??;
    let client = http_client()?;
    let response = client
        .post(TOKEN_ENDPOINT)
        .form(&[
            ("client_id", CLIENT_ID),
            ("client_secret", client_secret.as_str()),
            ("code", callback.code.as_str()),
            ("state", callback.state.as_str()),
            ("redirect_uri", callback.redirect_uri.as_str()),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await
        .map_err(|error| {
            crate::tr("rust.status.account.ely.oauth.exchange-error")
                .replace("%0", &error.to_string())
        })?;
    let token = decode_token(
        response,
        crate::tr("rust.status.account.ely.oauth.operation.sign-in"),
    )
    .await?;
    let credentials = build_credentials(token, None)?;
    let profile = load_profile(&client, &credentials.access_token).await?;
    Ok((profile, credentials))
}

pub async fn refresh(credentials: &Credentials) -> Result<(Profile, Credentials), String> {
    let Some(refresh_token) = credentials
        .refresh_token
        .as_deref()
        .filter(|token| !token.trim().is_empty())
    else {
        let client = http_client()?;
        let profile = load_profile(&client, &credentials.access_token).await?;
        return Ok((profile, credentials.clone()));
    };
    let client_secret = load_client_secret()?;
    let client = http_client()?;
    let response = client
        .post(TOKEN_ENDPOINT)
        .form(&[
            ("client_id", CLIENT_ID),
            ("client_secret", client_secret.as_str()),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|error| {
            crate::tr("rust.status.account.ely.oauth.refresh-error")
                .replace("%0", &error.to_string())
        })?;
    let token = decode_token(
        response,
        crate::tr("rust.status.account.ely.oauth.operation.token-refresh"),
    )
    .await?;
    let refreshed = build_credentials(token, credentials.refresh_token.clone())?;
    let profile = load_profile(&client, &refreshed.access_token).await?;
    Ok((profile, refreshed))
}

fn load_client_secret() -> Result<String, String> {
    std::env::var("LEGACYLAUNCHER_ELY_CLIENT_SECRET")
        .ok()
        .filter(|secret| !secret.trim().is_empty())
        .ok_or_else(|| crate::tr("rust.status.account.ely.oauth.secret-required").to_owned())
}

pub fn authenticator(profile: Profile, credentials: Credentials) -> Authenticator {
    // LegacyLauncher's ElyUser reports auth type "msa" and uses displayName as auth XUID.
    Authenticator {
        access_token: credentials.access_token,
        name: profile.display_name.clone(),
        uuid: profile.uuid,
        xbox_account: Some(XboxAccount {
            xuid: profile.display_name,
        }),
        user_properties: None,
        client_id: None,
        client_token: None,
    }
}

async fn load_profile(client: &reqwest::Client, access_token: &str) -> Result<Profile, String> {
    let response = client
        .get(ACCOUNT_INFO_ENDPOINT)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|error| {
            crate::tr("rust.status.account.ely.oauth.profile-load-error")
                .replace("%0", &error.to_string())
        })?;
    let status = response.status();
    if !status.is_success() {
        let detail = response
            .json::<serde_json::Value>()
            .await
            .ok()
            .and_then(|value| {
                ["message", "error_description", "error"]
                    .into_iter()
                    .find_map(|key| value.get(key).and_then(serde_json::Value::as_str))
                    .map(|message| message.chars().take(200).collect::<String>())
            });
        return Err(detail.map_or_else(
            || {
                crate::tr("rust.status.account.ely.oauth.profile-http-error")
                    .replace("%0", &status.to_string())
            },
            |message| {
                crate::tr("rust.status.account.ely.oauth.profile-detail-error")
                    .replace("%0", &message)
            },
        ));
    }
    let info = response
        .json::<AccountInfo>()
        .await
        .map_err(|_| crate::tr("rust.status.account.ely.oauth.profile-unreadable").to_owned())?;
    let uuid = uuid::Uuid::parse_str(&info.uuid)
        .map(|uuid| uuid.to_string())
        .map_err(|_| crate::tr("rust.status.account.ely.oauth.profile-uuid-invalid").to_owned())?;
    let display_name = info
        .display_name
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| info.username.clone());
    if info.id < 0 || info.username.trim().is_empty() {
        return Err(crate::tr("rust.status.account.ely.oauth.profile-invalid").to_owned());
    }
    Ok(Profile {
        id: info.id,
        username: info.username,
        display_name,
        uuid,
    })
}

fn build_credentials(
    token: TokenResponse,
    previous_refresh_token: Option<String>,
) -> Result<Credentials, String> {
    if token.token_type != "Bearer" || token.access_token.trim().is_empty() {
        return Err(crate::tr("rust.status.account.ely.oauth.token-invalid").to_owned());
    }
    let expiry_time = token
        .expires_in
        .filter(|seconds| *seconds > 0)
        .and_then(|seconds| {
            let now_millis: u64 = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()?
                .as_millis()
                .try_into()
                .ok()?;
            now_millis.checked_add(seconds.checked_mul(1000)?)
        });
    Ok(Credentials {
        access_token: token.access_token,
        refresh_token: token
            .refresh_token
            .filter(|value| !value.trim().is_empty())
            .or(previous_refresh_token),
        expiry_time,
    })
}

async fn decode_token(
    response: reqwest::Response,
    operation: &str,
) -> Result<TokenResponse, String> {
    let status = response.status();
    if !status.is_success() {
        let body = response
            .json::<serde_json::Value>()
            .await
            .unwrap_or(serde_json::Value::Null);
        let detail = ["error_description", "message", "error"]
            .into_iter()
            .find_map(|key| body.get(key).and_then(serde_json::Value::as_str))
            .map(|message| message.chars().take(200).collect::<String>());
        return Err(detail.map_or_else(
            || {
                crate::tr("rust.status.account.ely.oauth.http-error")
                    .replace("%0", operation)
                    .replace("%1", &status.to_string())
            },
            |message| {
                crate::tr("rust.status.account.ely.oauth.detail-error")
                    .replace("%0", operation)
                    .replace("%1", &message)
            },
        ));
    }
    response.json::<TokenResponse>().await.map_err(|_| {
        crate::tr("rust.status.account.ely.oauth.token-unreadable").replace("%0", operation)
    })
}

fn http_client() -> Result<reqwest::Client, String> {
    crate::http::client_builder()
        .timeout(Duration::from_secs(30))
        .user_agent("LegacyLauncher-Rust/0.1")
        .build()
        .map_err(|error| {
            crate::tr("rust.status.account.ely.oauth.client-error")
                .replace("%0", &error.to_string())
        })
}

struct AuthorizationCallback {
    code: String,
    state: String,
    redirect_uri: String,
}

fn request_authorization_code(
    cancel_requested: Arc<AtomicBool>,
) -> Result<AuthorizationCallback, String> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|error| {
        crate::tr("rust.status.account.ely.oauth.callback-open-error")
            .replace("%0", &error.to_string())
    })?;
    listener.set_nonblocking(true).map_err(|error| {
        crate::tr("rust.status.account.ely.oauth.callback-prepare-error")
            .replace("%0", &error.to_string())
    })?;
    let port = listener
        .local_addr()
        .map_err(|error| {
            crate::tr("rust.status.account.ely.oauth.callback-port-error")
                .replace("%0", &error.to_string())
        })?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}/");
    let state = uuid::Uuid::new_v4().simple().to_string();
    let mut login_url = Url::parse("https://account.ely.by/oauth2/v1").map_err(|error| {
        crate::tr("rust.status.account.ely.oauth.url-error").replace("%0", &error.to_string())
    })?;
    login_url.query_pairs_mut().extend_pairs([
        ("client_id", CLIENT_ID),
        ("response_type", "code"),
        ("scope", "account_info minecraft_server_session"),
        ("redirect_uri", redirect_uri.as_str()),
        ("state", state.as_str()),
        ("prompt", "select_account"),
    ]);
    crate::open_url(login_url.as_str());

    let deadline = Instant::now() + Duration::from_secs(300);
    while Instant::now() < deadline {
        if cancel_requested.load(Ordering::Acquire) {
            return Err(crate::tr("rust.status.account.ely.oauth.cancelled").to_owned());
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
                let target = read_request_target(&mut stream)?;
                let callback = Url::parse(&redirect_uri)
                    .and_then(|base| base.join(&target))
                    .map_err(|error| {
                        crate::tr("rust.status.account.ely.oauth.callback-read-error")
                            .replace("%0", &error.to_string())
                    })?;
                let parameters: std::collections::HashMap<String, String> =
                    callback.query_pairs().into_owned().collect();
                if parameters.get("state") != Some(&state) {
                    write_callback_response(&mut stream, false);
                    continue;
                }
                if let Some(error) = parameters.get("error") {
                    write_callback_response(&mut stream, true);
                    return Err(if error == "access_denied" {
                        crate::tr("rust.status.account.ely.oauth.cancelled").to_owned()
                    } else {
                        crate::tr("rust.status.account.ely.oauth.rejected").to_owned()
                    });
                }
                let code = parameters
                    .get("code")
                    .filter(|value| !value.trim().is_empty())
                    .cloned()
                    .ok_or_else(|| {
                        crate::tr("rust.status.account.ely.oauth.code-missing").to_owned()
                    })?;
                write_callback_response(&mut stream, true);
                return Ok(AuthorizationCallback {
                    code,
                    state,
                    redirect_uri,
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => {
                return Err(
                    crate::tr("rust.status.account.ely.oauth.callback-receive-error")
                        .replace("%0", &error.to_string()),
                );
            }
        }
    }
    Err(crate::tr("rust.status.account.ely.oauth.timeout").to_owned())
}

fn read_request_target(stream: &mut TcpStream) -> Result<String, String> {
    let mut request = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = stream.read(&mut chunk).map_err(|error| {
            crate::tr("rust.status.account.ely.oauth.callback-read-error")
                .replace("%0", &error.to_string())
        })?;
        if count == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..count]);
        if request.len() > 16 * 1024 {
            return Err(crate::tr("rust.status.account.ely.oauth.callback-too-large").to_owned());
        }
    }
    let request = String::from_utf8_lossy(&request);
    request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .map(str::to_owned)
        .ok_or_else(|| crate::tr("rust.status.account.ely.oauth.callback-malformed").to_owned())
}

fn write_callback_response(stream: &mut TcpStream, accepted: bool) {
    let (status, body, location) = if accepted {
        (
            "302 Found",
            crate::tr("rust.status.account.ely.oauth.browser-complete"),
            Some(CALLBACK_SUCCESS),
        )
    } else {
        (
            "400 Bad Request",
            crate::tr("rust.status.account.ely.oauth.browser-invalid"),
            None,
        )
    };
    let mut response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if let Some(location) = location {
        response.push_str(&format!("Location: {location}\r\n"));
    }
    response.push_str("\r\n");
    response.push_str(body);
    let _ = stream.write_all(response.as_bytes());
}
