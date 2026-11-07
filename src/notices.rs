use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;
use url::Url;

#[derive(Debug, Clone)]
pub struct Notice {
    pub id: String,
    pub order: i64,
    pub text_lines: Vec<Vec<RichTextSpan>>,
    pub promoted: bool,
    pub action: Option<NoticeAction>,
    pub image: Option<Vec<u8>>,
    pub image_load_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RichTextSpan {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub link: Option<String>,
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
        .map_err(|error| crate::tr("rust.error.notices.url").replace("%0", &error.to_string()))?;
    url.query_pairs_mut()
        .append_pair("version", env!("CARGO_PKG_VERSION"))
        .append_pair("bootstrap_name", "legacylauncher-rust")
        .append_pair("bootstrap_version", env!("CARGO_PKG_VERSION"))
        .append_pair("locale", "en_US");
    let client = crate::http::client_builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|error| {
            crate::tr("rust.error.notices.client").replace("%0", &error.to_string())
        })?;
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|error| crate::tr("rust.error.notices.fetch").replace("%0", &error.to_string()))?
        .error_for_status()
        .map_err(|error| crate::tr("rust.error.notices.http").replace("%0", &error.to_string()))?;
    let payload = response
        .json::<Value>()
        .await
        .map_err(|error| crate::tr("rust.error.notices.json").replace("%0", &error.to_string()))?;
    let items = payload
        .get("notices")
        .and_then(Value::as_array)
        .or_else(|| payload.as_array())
        .ok_or_else(|| crate::tr("rust.error.notices.list-missing").to_owned())?;
    let mut notices = Vec::new();
    for item in items {
        let Some((mut notice, image_url)) = parse_notice(item) else {
            continue;
        };
        if let Some(image_url) = image_url {
            match load_remote_image(&client, &image_url).await {
                Ok(image) => notice.image = image,
                Err(error) => notice.image_load_error = Some(error),
            }
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
    let raw_text = value.get("text")?.as_str()?;
    let (plain_text, text_lines) = parse_safe_html(raw_text);
    if plain_text.is_empty() {
        return None;
    }
    let action = value.get("action").and_then(parse_action);
    let (image, image_url) = parse_image(value.get("image"));
    Some((
        Notice {
            id,
            order: value.get("pos").and_then(Value::as_i64).unwrap_or(-1),
            text_lines,
            promoted: value
                .get("promoted")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            action,
            image,
            image_load_error: None,
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
    let host = Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| "remote server".to_owned());
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| {
            crate::tr("rust.error.notices.image-request")
                .replace("%0", &host)
                .replace("%1", &error.without_url().to_string())
        })?
        .error_for_status()
        .map_err(|error| {
            crate::tr("rust.error.notices.image-http")
                .replace("%0", &host)
                .replace(
                    "%1",
                    &error
                        .status()
                        .map_or_else(|| "unknown".to_owned(), |status| status.to_string()),
                )
        })?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_IMAGE_BYTES)
    {
        return Err(crate::tr("rust.error.notices.image-too-large").replace("%0", &host));
    }
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        crate::tr("rust.error.notices.image-read")
            .replace("%0", &host)
            .replace("%1", &error.without_url().to_string())
    })? {
        if bytes.len().saturating_add(chunk.len()) as u64 > MAX_IMAGE_BYTES {
            return Err(crate::tr("rust.error.notices.image-too-large").replace("%0", &host));
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
                .unwrap_or(crate::tr("rust.notice.action.open-link"))
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
                .unwrap_or(crate::tr("rust.notice.action.open"))
                .to_owned();
            Some(NoticeAction::Scene {
                label,
                scene: scene.to_owned(),
            })
        }
        "launcher" => {
            let launcher = action.get("launcher")?.as_str()?.trim();
            let label = crate::tr("rust.notice.action.start-launcher").replace("%0", launcher);
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
                .unwrap_or(crate::tr("rust.notice.action.server"));
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
                label: crate::tr("rust.notice.action.join-server").replace("%0", name),
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

#[derive(Debug, Clone, Default)]
struct NoticeHtmlStyle {
    bold: bool,
    italic: bool,
    underline: bool,
    link: Option<String>,
}

pub fn parse_safe_html(input: &str) -> (String, Vec<Vec<RichTextSpan>>) {
    let mut plain = String::with_capacity(input.len());
    let mut lines: Vec<Vec<RichTextSpan>> = vec![Vec::new()];
    let mut buffer = String::new();
    let mut style = NoticeHtmlStyle::default();
    let mut stack: Vec<(String, NoticeHtmlStyle)> = Vec::new();
    let mut hidden_tag: Option<String> = None;
    let mut index = 0;

    while index < input.len() {
        let rest = &input[index..];
        if rest.starts_with('<')
            && let Some(offset) = rest.find('>')
        {
            let raw_tag = &rest[1..offset];
            let tag = raw_tag.trim();
            let is_closing = tag.starts_with('/');
            let name = tag
                .trim_start_matches('/')
                .split(|character: char| character.is_whitespace() || character == '/')
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            index += offset + 1;

            if let Some(hidden) = hidden_tag.as_deref() {
                if is_closing && name == hidden {
                    hidden_tag = None;
                }
                continue;
            }
            if !is_closing && matches!(name.as_str(), "script" | "style") {
                flush_notice_text(&mut buffer, &mut plain, lines.last_mut().unwrap(), &style);
                hidden_tag = Some(name);
                continue;
            }
            if name.is_empty() || name.starts_with('!') || name.starts_with('?') {
                continue;
            }

            match name.as_str() {
                "br" => {
                    flush_notice_text(&mut buffer, &mut plain, lines.last_mut().unwrap(), &style);
                    push_notice_line_break(&mut plain, &mut lines, false);
                }
                "p" | "div" => {
                    flush_notice_text(&mut buffer, &mut plain, lines.last_mut().unwrap(), &style);
                    if is_closing {
                        push_notice_line_break(&mut plain, &mut lines, true);
                    } else if lines.last().is_some_and(|line| !line.is_empty()) {
                        push_notice_line_break(&mut plain, &mut lines, true);
                    }
                }
                "li" => {
                    flush_notice_text(&mut buffer, &mut plain, lines.last_mut().unwrap(), &style);
                    if is_closing {
                        push_notice_line_break(&mut plain, &mut lines, false);
                    } else {
                        if lines.last().is_some_and(|line| !line.is_empty()) {
                            push_notice_line_break(&mut plain, &mut lines, false);
                        }
                        buffer.push_str("• ");
                    }
                }
                "b" | "strong" | "i" | "em" | "u" | "a" => {
                    flush_notice_text(&mut buffer, &mut plain, lines.last_mut().unwrap(), &style);
                    if is_closing {
                        if let Some(position) = stack.iter().rposition(|(tag, _)| tag == &name) {
                            style = stack[position].1.clone();
                            stack.truncate(position);
                        }
                    } else if !tag.ends_with('/') {
                        stack.push((name.clone(), style.clone()));
                        match name.as_str() {
                            "b" | "strong" => style.bold = true,
                            "i" | "em" => style.italic = true,
                            "u" => style.underline = true,
                            "a" => {
                                style.link = html_tag_attribute(raw_tag, "href")
                                    .map(|href| decode_html_entities(&href))
                                    .and_then(|href| safe_http_url(&href));
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            continue;
        }

        let character = rest.chars().next().unwrap_or_default();
        buffer.push(character);
        index += character.len_utf8();
    }
    flush_notice_text(&mut buffer, &mut plain, lines.last_mut().unwrap(), &style);
    while lines.len() > 1 && lines.last().is_some_and(Vec::is_empty) {
        lines.pop();
    }
    (plain.trim().to_owned(), lines)
}

fn flush_notice_text(
    buffer: &mut String,
    plain: &mut String,
    line: &mut Vec<RichTextSpan>,
    style: &NoticeHtmlStyle,
) {
    if buffer.is_empty() {
        return;
    }
    let text = decode_html_entities(buffer);
    plain.push_str(&text);
    if let Some(previous) = line.last_mut()
        && previous.bold == style.bold
        && previous.italic == style.italic
        && previous.underline == style.underline
        && previous.link == style.link
    {
        previous.text.push_str(&text);
    } else {
        line.push(RichTextSpan {
            text,
            bold: style.bold,
            italic: style.italic,
            underline: style.underline,
            link: style.link.clone(),
        });
    }
    buffer.clear();
}

fn push_notice_line_break(plain: &mut String, lines: &mut Vec<Vec<RichTextSpan>>, paragraph: bool) {
    if !paragraph || lines.last().is_some_and(|line| !line.is_empty()) {
        plain.push('\n');
        lines.push(Vec::new());
    }
}

fn html_tag_attribute(tag: &str, attribute: &str) -> Option<String> {
    let bytes = tag.as_bytes();
    let mut index = tag
        .find(|character: char| character.is_whitespace())
        .unwrap_or(tag.len());
    while index < bytes.len() {
        while index < bytes.len() && (bytes[index].is_ascii_whitespace() || bytes[index] == b'/') {
            index += 1;
        }
        let name_start = index;
        while index < bytes.len()
            && (bytes[index].is_ascii_alphanumeric() || matches!(bytes[index], b'-' | b'_'))
        {
            index += 1;
        }
        if name_start == index {
            index += 1;
            continue;
        }
        let name = &tag[name_start..index];
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] != b'=' {
            continue;
        }
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() {
            return None;
        }
        let (value_start, value_end) = if matches!(bytes[index], b'\'' | b'"') {
            let quote = bytes[index];
            index += 1;
            let start = index;
            while index < bytes.len() && bytes[index] != quote {
                index += 1;
            }
            let end = index;
            if index < bytes.len() {
                index += 1;
            }
            (start, end)
        } else {
            let start = index;
            while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            (start, index)
        };
        if name.eq_ignore_ascii_case(attribute) {
            return tag.get(value_start..value_end).map(str::to_owned);
        }
    }
    None
}

fn decode_html_entities(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        let rest = &input[index..];
        if rest.starts_with('&')
            && let Some(end) = rest.find(';')
            && end <= 12
        {
            let entity = &rest[1..end];
            let decoded = match entity {
                "nbsp" => Some(' '),
                "amp" => Some('&'),
                "quot" => Some('"'),
                "apos" | "#39" => Some('\''),
                "lt" => Some('<'),
                "gt" => Some('>'),
                _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                    u32::from_str_radix(&entity[2..], 16)
                        .ok()
                        .and_then(char::from_u32)
                }
                _ if entity.starts_with('#') => {
                    entity[1..].parse::<u32>().ok().and_then(char::from_u32)
                }
                _ => None,
            };
            if let Some(character) = decoded {
                output.push(character);
                index += end + 1;
                continue;
            }
        }
        let character = rest.chars().next().unwrap_or_default();
        output.push(character);
        index += character.len_utf8();
    }
    output
}
