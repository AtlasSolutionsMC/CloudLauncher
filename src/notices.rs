use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;
use url::Url;

#[derive(Debug, Clone)]
pub struct Notice {
    pub id: String,
    pub order: i64,
    pub text: String,
    pub promoted: bool,
    pub action: Option<NoticeAction>,
    pub image: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub enum NoticeAction {
    Url {
        label: String,
        url: String,
    },
    Scene {
        label: String,
        scene: String,
    },
    Launcher {
        label: String,
        launcher: String,
        download_url: Option<String>,
    },
    Server {
        label: String,
        address: String,
        accounts: Option<Vec<String>>,
        families: Vec<String>,
    },
}

pub async fn fetch(client_id: &str) -> Result<Vec<Notice>, String> {
    let mut url = Url::parse(&format!("https://api.llaun.ch/v1/notices/{client_id}"))
        .map_err(|error| format!("Invalid notice service URL: {error}"))?;
    url.query_pairs_mut()
        .append_pair("version", env!("CARGO_PKG_VERSION"))
        .append_pair("bootstrap_name", "legacylauncher-rust")
        .append_pair("bootstrap_version", env!("CARGO_PKG_VERSION"))
        .append_pair("locale", "en_US");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|error| format!("Could not create notice request: {error}"))?;
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|error| format!("Could not load notices: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Notice service returned an error: {error}"))?;
    let payload = response
        .json::<Value>()
        .await
        .map_err(|error| format!("Notice response was not valid JSON: {error}"))?;
    let items = payload
        .get("notices")
        .and_then(Value::as_array)
        .or_else(|| payload.as_array())
        .ok_or_else(|| "Notice response did not contain a notice list.".to_owned())?;
    let mut notices = Vec::new();
    for item in items {
        let Some((mut notice, image_url)) = parse_notice(item) else {
            continue;
        };
        if let Some(image_url) = image_url {
            notice.image = load_remote_image(&client, &image_url).await.ok().flatten();
        }
        notices.push(notice);
    }
    notices.sort_by_key(|notice| (notice.order, notice.id.clone()));
    Ok(notices)
}

pub fn parse_notice(value: &Value) -> Option<(Notice, Option<String>)> {
    let id = value.get("id").and_then(|value| {
        value.as_i64().map(|id| id.to_string()).or_else(|| {
            value
                .as_str()
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
        })
    })?;
    let text = value.get("text")?.as_str()?;
    let text = strip_html(text);
    if text.is_empty() {
        return None;
    }
    let action = value.get("action").and_then(parse_action);
    let (image, image_url) = parse_image(value.get("image"));
    Some((
        Notice {
            id,
            order: value.get("pos").and_then(Value::as_i64).unwrap_or(-1),
            text,
            promoted: value
                .get("promoted")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            action,
            image,
        },
        image_url,
    ))
}

fn parse_image(value: Option<&Value>) -> (Option<Vec<u8>>, Option<String>) {
    let Some(value) = value else {
        return (None, None);
    };
    let source = if let Some(source) = value.as_str() {
        source.trim()
    } else if value.is_object() {
        value
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
    } else {
        return (None, None);
    };
    if source.is_empty() {
        return (None, None);
    }

    let encoded = source
        .strip_prefix("data:image/")
        .and_then(|payload| payload.split_once(',').map(|(_, data)| data))
        .unwrap_or(source);
    if let Ok(bytes) = STANDARD.decode(encoded)
        && bytes.len() <= 8 * 1024 * 1024
    {
        return (Some(bytes), None);
    }
    (None, safe_http_url(source))
}

pub async fn load_remote_image(
    client: &reqwest::Client,
    url: &str,
) -> Result<Option<Vec<u8>>, String> {
    const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_IMAGE_BYTES)
    {
        return Ok(None);
    }
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
        if bytes.len().saturating_add(chunk.len()) as u64 > MAX_IMAGE_BYTES {
            return Ok(None);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(Some(bytes))
}

pub fn parse_promoted_server(value: &Value) -> Option<NoticeAction> {
    let action = serde_json::json!({ "type": "server", "server": value });
    parse_action(&action)
}

fn parse_action(value: &Value) -> Option<NoticeAction> {
    let action = value
        .get("v2")
        .filter(|value| value.is_object())
        .unwrap_or(value);
    match action.get("type")?.as_str()? {
        "url" => {
            let url = action.get("url")?.as_str()?.trim();
            let url = safe_http_url(url)?;
            let label = action
                .get("name")
                .and_then(Value::as_str)
                .filter(|label| !label.trim().is_empty())
                .unwrap_or("Open link")
                .to_owned();
            Some(NoticeAction::Url { label, url })
        }
        "scene" => {
            let scene = action.get("scene")?.as_str()?.trim();
            let label = action
                .get("translations")
                .and_then(|translations| translations.get("en_US"))
                .and_then(Value::as_str)
                .filter(|label| !label.trim().is_empty())
                .unwrap_or("Open")
                .to_owned();
            Some(NoticeAction::Scene {
                label,
                scene: scene.to_owned(),
            })
        }
        "launcher" => {
            let launcher = action.get("launcher")?.as_str()?.trim();
            let label = format!("Start {launcher}");
            let download_url = action
                .get("url")
                .and_then(Value::as_str)
                .and_then(safe_http_url);
            Some(NoticeAction::Launcher {
                label,
                launcher: launcher.to_owned(),
                download_url,
            })
        }
        "server" => {
            let server = action.get("server")?;
            let name = server
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("Server");
            let address = server.get("address").and_then(Value::as_str)?.trim();
            if address.is_empty() || address.chars().any(char::is_control) {
                return None;
            }
            let full_address = if let Some(port) = server.get("port").and_then(Value::as_u64) {
                if !(1..=65535).contains(&port) {
                    return None;
                }
                if address.contains(':') && !address.starts_with('[') {
                    if port == 25565 {
                        format!("[{address}]")
                    } else {
                        format!("[{address}]:{port}")
                    }
                } else if port == 25565 {
                    address.to_owned()
                } else {
                    format!("{address}:{port}")
                }
            } else {
                address.to_owned()
            };
            Some(NoticeAction::Server {
                label: format!("Join {name}"),
                address: full_address,
                families: server
                    .get("family")
                    .and_then(|family| {
                        family
                            .as_str()
                            .map(|value| vec![value.to_owned()])
                            .or_else(|| {
                                family.as_array().map(|values| {
                                    values
                                        .iter()
                                        .filter_map(Value::as_str)
                                        .map(str::to_owned)
                                        .collect()
                                })
                            })
                    })
                    .unwrap_or_default(),
                accounts: server
                    .get("accounts")
                    .and_then(Value::as_array)
                    .map(|accounts| {
                        accounts
                            .iter()
                            .filter_map(Value::as_str)
                            .map(|account| account.to_ascii_lowercase())
                            .collect::<Vec<_>>()
                    })
                    .filter(|accounts| !accounts.is_empty()),
            })
        }
        _ => None,
    }
}

pub fn safe_http_url(value: &str) -> Option<String> {
    let parsed = Url::parse(value.trim()).ok()?;
    matches!(parsed.scheme(), "http" | "https").then(|| parsed.to_string())
}

fn strip_html(input: &str) -> String {
    let mut plain = String::with_capacity(input.len());
    let mut in_tag = false;
    let mut tag = String::new();
    for character in input.chars() {
        match character {
            '<' if !in_tag => {
                in_tag = true;
                tag.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let tag = tag.trim().to_ascii_lowercase();
                if tag.starts_with("br") || tag.starts_with("/p") || tag.starts_with("/div") {
                    plain.push('\n');
                }
            }
            _ if in_tag => tag.push(character),
            _ => plain.push(character),
        }
    }
    plain
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}
