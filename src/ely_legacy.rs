use serde::{Deserialize, Serialize};

const AUTH_SERVER: &str = "https://authserver.ely.by/auth";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credentials {
    pub access_token: String,
    pub client_token: String,
}

#[derive(Debug, Clone)]
pub struct Profile {
    pub username: String,
    pub display_name: String,
    pub uuid: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Agent {
    name: &'static str,
    version: u8,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthenticateRequest<'a> {
    agent: Agent,
    username: &'a str,
    password: &'a str,
    client_token: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RefreshRequest<'a> {
    client_token: &'a str,
    access_token: &'a str,
    request_user: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthResponse {
    access_token: Option<String>,
    client_token: Option<String>,
    selected_profile: Option<GameProfile>,
    error: Option<String>,
    error_message: Option<String>,
}

#[derive(Deserialize)]
struct GameProfile {
    id: String,
    name: String,
}

pub async fn authenticate(
    username: &str,
    password: &str,
) -> Result<(Profile, Credentials), String> {
    let username = username.trim();
    if username.is_empty() || password.is_empty() {
        return Err(crate::tr("rust.status.account.ely.legacy.credentials-required").to_owned());
    }
    let request = AuthenticateRequest {
        agent: Agent {
            name: "Minecraft",
            version: 1,
        },
        username,
        password,
        client_token: uuid::Uuid::new_v4().to_string(),
    };
    send_request("authenticate", &request, username).await
}

pub async fn refresh(
    username: &str,
    credentials: &Credentials,
) -> Result<(Profile, Credentials), String> {
    let request = RefreshRequest {
        client_token: &credentials.client_token,
        access_token: &credentials.access_token,
        request_user: true,
    };
    send_request("refresh", &request, username).await
}

async fn send_request<T: Serialize>(
    endpoint: &str,
    body: &T,
    username: &str,
) -> Result<(Profile, Credentials), String> {
    let client = crate::http::client_builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|error| {
            crate::tr("rust.status.account.ely.legacy.client-error")
                .replace("%0", &error.to_string())
        })?;
    let response = client
        .post(format!("{AUTH_SERVER}/{endpoint}"))
        .header(reqwest::header::ACCEPT, "application/json")
        .json(body)
        .send()
        .await
        .map_err(|error| {
            crate::tr("rust.status.account.ely.legacy.request-error")
                .replace("%0", &error.to_string())
        })?;
    let status = response.status();
    let payload = response.json::<AuthResponse>().await.map_err(|error| {
        crate::tr("rust.status.account.ely.legacy.response-error").replace("%0", &error.to_string())
    })?;
    if !status.is_success() || payload.error.is_some() || payload.error_message.is_some() {
        let detail = payload
            .error_message
            .or(payload.error)
            .unwrap_or_else(|| status.to_string());
        return Err(
            crate::tr("rust.status.account.ely.legacy.sign-in-error").replace("%0", &detail)
        );
    }
    let access_token = payload
        .access_token
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            crate::tr("rust.status.account.ely.legacy.access-token-missing").to_owned()
        })?;
    let client_token = payload
        .client_token
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            crate::tr("rust.status.account.ely.legacy.client-token-missing").to_owned()
        })?;
    let game_profile = payload
        .selected_profile
        .ok_or_else(|| crate::tr("rust.status.account.ely.legacy.profile-missing").to_owned())?;
    let uuid = uuid::Uuid::parse_str(&game_profile.id)
        .map(|uuid| uuid.to_string())
        .map_err(|_| crate::tr("rust.status.account.ely.legacy.profile-uuid-invalid").to_owned())?;
    if game_profile.name.trim().is_empty() {
        return Err(crate::tr("rust.status.account.ely.legacy.profile-name-empty").to_owned());
    }

    Ok((
        Profile {
            username: username.trim().to_owned(),
            display_name: game_profile.name,
            uuid,
        },
        Credentials {
            access_token,
            client_token,
        },
    ))
}
