use crate::notices::{self, Notice, NoticeAction};
use bzip2::read::BzDecoder;
use flate2::read::GzDecoder;
use rsa::{
    RsaPublicKey,
    pkcs1v15::{Signature, VerifyingKey},
    pkcs8::DecodePublicKey,
    signature::Verifier,
};
use serde_json::{Map, Value};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::io::Read;

const MAX_COMPRESSED_BYTES: usize = 8 * 1024 * 1024;
const MAX_CONFIG_BYTES: usize = 8 * 1024 * 1024;
const SIGNATURE_BYTES: usize = 256;
const BOOTSTRAP_HOSTS: [&str; 2] = ["bootstrap.llaun.ch", "bootstrap.legacylauncher.ru"];

#[derive(Debug, Clone, Default)]
pub struct Configuration {
    pub notices: Vec<Notice>,
    pub promoted_servers: Vec<NoticeAction>,
    pub notification: Option<Notification>,
    pub feedback_url: Option<String>,
    allow_notice_disable_modulo: u32,
    pub allow_ely_everywhere: bool,
}

impl Configuration {
    pub fn allows_notice_disable(&self, client_id: &str) -> bool {
        if self.allow_notice_disable_modulo == 0 {
            return false;
        }
        let Ok(client_id) = uuid::Uuid::parse_str(client_id) else {
            return false;
        };
        let raw = client_id.as_u128();
        let most_significant = (raw >> 64) as u64;
        let least_significant = raw as u64;
        let combined = most_significant ^ least_significant;
        let java_hash = ((combined >> 32) as u32) ^ (combined as u32);
        java_hash % self.allow_notice_disable_modulo == 0
    }
}

#[derive(Debug, Clone)]
pub struct Notification {
    pub id: String,
    pub url: String,
    pub image: Option<Vec<u8>>,
}

pub async fn fetch() -> Result<Configuration, String> {
    let brand = option_env!("LEGACYLAUNCHER_SHORT_BRAND").unwrap_or("develop");
    let client = crate::http::client_builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(concat!("LegacyLauncher-Rust/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| {
            crate::tr("rust.error.bootstrap.client").replace("%0", &error.to_string())
        })?;
    let mut last_error = None;
    for host in BOOTSTRAP_HOSTS {
        let url = format!("https://{host}/{brand}/bootstrap.json.mgz.signed");
        let response = match client.get(&url).send().await {
            Ok(response) => match response.error_for_status() {
                Ok(response) => response,
                Err(error) => {
                    last_error = Some(
                        crate::tr("rust.error.bootstrap.http").replace("%0", &error.to_string()),
                    );
                    continue;
                }
            },
            Err(error) => {
                last_error =
                    Some(crate::tr("rust.error.bootstrap.fetch").replace("%0", &error.to_string()));
                continue;
            }
        };
        let payload =
            match read_limited_response(response, MAX_COMPRESSED_BYTES + SIGNATURE_BYTES).await {
                Ok(payload) => payload,
                Err(error) => {
                    last_error = Some(error);
                    continue;
                }
            };
        match decode_signed_options(&payload) {
            Ok(options) => return parse_configuration(&client, options).await,
            Err(error) => {
                last_error = Some(crate::tr("rust.error.bootstrap.invalid").replace("%0", &error));
            }
        }
    }
    Err(last_error.unwrap_or_else(|| crate::tr("rust.error.bootstrap.unavailable").to_owned()))
}

async fn read_limited_response(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(crate::tr("rust.error.bootstrap.response-large").to_owned());
    }
    let mut payload = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| crate::tr("rust.error.bootstrap.read").replace("%0", &error.to_string()))?
    {
        if payload.len().saturating_add(chunk.len()) > limit {
            return Err(crate::tr("rust.error.bootstrap.response-large").to_owned());
        }
        payload.extend_from_slice(&chunk);
    }
    Ok(payload)
}

fn decode_signed_options(payload: &[u8]) -> Result<Value, String> {
    if payload.len() <= SIGNATURE_BYTES {
        return Err(crate::tr("rust.error.bootstrap.signed-incomplete").to_owned());
    }
    let signed_content = &payload[SIGNATURE_BYTES..];
    if signed_content.len() < 5 {
        return Err(crate::tr("rust.error.bootstrap.marker-missing").to_owned());
    }
    if signed_content.len() > MAX_COMPRESSED_BYTES {
        return Err(crate::tr("rust.error.bootstrap.compressed-large").to_owned());
    }
    let public_key =
        RsaPublicKey::from_public_key_der(include_bytes!("../assets/bootstrap_public.der"))
            .map_err(|error| {
                crate::tr("rust.error.bootstrap.key-read").replace("%0", &error.to_string())
            })?;
    let signature = Signature::try_from(&payload[..SIGNATURE_BYTES]).map_err(|error| {
        crate::tr("rust.error.bootstrap.signature-format").replace("%0", &error.to_string())
    })?;
    let digest = Sha256::digest(signed_content);
    let digest_hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    VerifyingKey::<Sha1>::new(public_key)
        .verify(digest_hex.as_bytes(), &signature)
        .map_err(|_| crate::tr("rust.error.bootstrap.signature-invalid").to_owned())?;

    let (marker, compressed) = signed_content.split_at(5);
    let mut decoded = Vec::new();
    match marker {
        b"gzip:" => GzDecoder::new(compressed)
            .take(MAX_CONFIG_BYTES as u64 + 1)
            .read_to_end(&mut decoded)
            .map_err(|error| {
                crate::tr("rust.error.bootstrap.gzip").replace("%0", &error.to_string())
            })?,
        b"bzip:" => BzDecoder::new(compressed)
            .take(MAX_CONFIG_BYTES as u64 + 1)
            .read_to_end(&mut decoded)
            .map_err(|error| {
                crate::tr("rust.error.bootstrap.bzip2").replace("%0", &error.to_string())
            })?,
        _ => return Err(crate::tr("rust.error.bootstrap.marker-unknown").to_owned()),
    };
    if decoded.len() > MAX_CONFIG_BYTES {
        return Err(crate::tr("rust.error.bootstrap.decompressed-large").to_owned());
    }

    let root: Value = serde_json::from_slice(&decoded).map_err(|error| {
        crate::tr("rust.error.bootstrap.json").replace("%0", &error.to_string())
    })?;
    let Some(mut options) = root
        .get("options")
        .filter(|value| value.is_object())
        .cloned()
    else {
        return Ok(Value::Object(Map::new()));
    };
    if let Some(java) = root.get("java")
        && let Some(options) = options.as_object_mut()
        && !options.contains_key("java")
    {
        options.insert("java".to_owned(), java.clone());
    }
    Ok(options)
}

async fn parse_configuration(
    client: &reqwest::Client,
    options: Value,
) -> Result<Configuration, String> {
    let mut configuration = Configuration::default();
    configuration.allow_notice_disable_modulo = options
        .get("allowNoticeDisable")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or_default();
    configuration.allow_ely_everywhere = options
        .get("ely")
        .and_then(Value::as_bool)
        .unwrap_or_default();
    if let Some(notices) = options
        .get("notices")
        .and_then(Value::as_object)
        .and_then(|locales| locales.get("en_US"))
        .and_then(Value::as_array)
    {
        for item in notices {
            let Some((mut notice, image_url)) = notices::parse_notice(item) else {
                continue;
            };
            if let Some(image_url) = image_url {
                notice.image = notices::load_remote_image(client, &image_url)
                    .await
                    .ok()
                    .flatten();
            }
            configuration.notices.push(notice);
        }
        configuration
            .notices
            .sort_by_key(|notice| (notice.order, notice.id.clone()));
    }

    if let Some(promotions) = options.get("promotedServers").and_then(Value::as_object) {
        let server_list = promotions
            .get("en_US")
            .or_else(|| promotions.get("global"))
            .and_then(Value::as_array);
        if let Some(server_list) = server_list {
            configuration.promoted_servers = server_list
                .iter()
                .filter_map(notices::parse_promoted_server)
                .collect();
        }
    }

    if let Some(value) = options
        .get("notifications")
        .and_then(Value::as_object)
        .and_then(|locales| locales.get("en_US"))
        .filter(|value| value.is_object())
    {
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let url = value
            .get("url")
            .and_then(Value::as_str)
            .and_then(notices::safe_http_url);
        let image_url = value
            .get("image")
            .and_then(Value::as_str)
            .and_then(notices::safe_http_url);
        if !id.is_empty()
            && let Some(url) = url
        {
            let image = if let Some(image_url) = image_url {
                notices::load_remote_image(client, &image_url)
                    .await
                    .ok()
                    .flatten()
            } else {
                None
            };
            configuration.notification = Some(Notification {
                id: id.to_owned(),
                url,
                image,
            });
        }
    }
    if let Some(feedback) = options.get("feedback").and_then(Value::as_object) {
        configuration.feedback_url = feedback
            .get("en_US")
            .or_else(|| feedback.get("global"))
            .and_then(Value::as_str)
            .and_then(notices::safe_http_url);
    }
    Ok(configuration)
}
