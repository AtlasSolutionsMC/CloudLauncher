use regex::Regex;
use serde_json::Value;

const SIGNATURES: &str = include_str!("../assets/crash/signature.json");

#[derive(Debug, Clone)]
pub struct CrashSuggestion {
    pub title: String,
    pub body: String,
    pub actions: Vec<CrashSuggestionAction>,
}

#[derive(Debug, Clone)]
pub struct CrashSuggestionAction {
    pub label: String,
    pub kind: CrashActionKind,
}

#[derive(Debug, Clone)]
pub enum CrashActionKind {
    OpenUrl(String),
    OpenFolder(String),
    OpenLogs,
    OpenSettings,
    ForceUpdate,
    RecommendedJava,
    SetJvmArgument(String),
}

pub fn diagnose(log: &str, version_id: &str, loader: &str) -> Vec<CrashSuggestion> {
    let Ok(root) = serde_json::from_str::<Value>(SIGNATURES) else {
        return Vec::new();
    };
    let variables = root.get("variables").and_then(Value::as_object);
    let buttons = root.get("buttons").and_then(Value::as_array);
    let mut result = Vec::new();
    let version_context = format!("{version_id}-{loader}");

    for signature in root
        .get("signatures")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if signature
            .get("fake")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            continue;
        }
        let Some(pattern) = signature.get("pattern").and_then(Value::as_str) else {
            continue;
        };
        let pattern = expand_vars(pattern, variables);
        let Ok(pattern) = Regex::new(&format!("^(?:{pattern})$")) else {
            // Java's signature database occasionally uses syntax unsupported by Rust's
            // regex engine. Ignore just that rule and retain the rest of the catalog.
            continue;
        };
        if !log.lines().any(|line| pattern.is_match(line)) {
            continue;
        }

        if let Some(version_pattern) = signature.get("version").and_then(Value::as_str) {
            let version_pattern = expand_vars(version_pattern, variables);
            let Ok(version_pattern) = Regex::new(&format!("^(?:{version_pattern})$")) else {
                continue;
            };
            if !version_pattern.is_match(&version_context) {
                continue;
            }
        }

        let title = localized(signature.get("title"), variables).unwrap_or_else(|| {
            signature
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(crate::tr("rust.crash.issue.known"))
                .replace('-', " ")
        });
        let body = localized(signature.get("body"), variables).unwrap_or_default();
        let actions = signature
            .get("buttons")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|button| resolve_button(button, buttons, variables))
            .collect();
        result.push(CrashSuggestion {
            title,
            body: clean_html(&body),
            actions,
        });

        // The Java launcher presents one most relevant crash explanation at a time.
        if result.len() >= 3 {
            break;
        }
    }
    result
}

fn resolve_button(
    button: &Value,
    shared_buttons: Option<&Vec<Value>>,
    variables: Option<&serde_json::Map<String, Value>>,
) -> Option<CrashSuggestionAction> {
    let resolved;
    let button = if let Some(name) = button.as_str() {
        resolved = shared_buttons?
            .iter()
            .find(|candidate| candidate.get("name").and_then(Value::as_str) == Some(name))?;
        resolved
    } else {
        button
    };
    let raw_label = localized(button.get("text"), variables).or_else(|| {
        button
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned)
    })?;
    let label = friendly_button_label(&raw_label);
    let actions = button.get("actions")?.as_array()?;
    let first_action = actions.first()?.as_str()?;
    let kind = if let Some(url) = first_action.strip_prefix("browse --www ") {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return None;
        }
        CrashActionKind::OpenUrl(expand_vars(url, variables))
    } else if let Some(folder) = first_action.strip_prefix("browse --folder ") {
        CrashActionKind::OpenFolder(expand_vars(folder, variables))
    } else if first_action == "gui args" {
        CrashActionKind::OpenLogs
    } else if first_action.starts_with("gui settings") {
        CrashActionKind::OpenSettings
    } else if first_action == "force-update" {
        CrashActionKind::ForceUpdate
    } else if first_action == "set --minecraft.jre.type recommended" {
        CrashActionKind::RecommendedJava
    } else if let Some(argument) = first_action.strip_prefix("set --minecraft.javaargs ") {
        CrashActionKind::SetJvmArgument(expand_vars(argument, variables))
    } else if let Some(folder) = first_action.strip_prefix("browse --folder ./") {
        CrashActionKind::OpenFolder(format!("./{folder}"))
    } else {
        return None;
    };
    Some(CrashSuggestionAction { label, kind })
}

fn localized(
    value: Option<&Value>,
    variables: Option<&serde_json::Map<String, Value>>,
) -> Option<String> {
    let value = value?;
    let value = if let Some(object) = value.as_object() {
        object.get("en_US")?
    } else {
        value
    };
    value.as_str().map(|text| expand_vars(text, variables))
}

fn expand_vars(input: &str, variables: Option<&serde_json::Map<String, Value>>) -> String {
    let mut output = input.to_owned();
    if let Some(variables) = variables {
        for (name, value) in variables {
            if let Some(value) = value.as_str() {
                output = output.replace(&format!("${{{name}}}"), value);
            }
        }
    }
    output
}

fn clean_html(input: &str) -> String {
    input
        .replace("<b>", "")
        .replace("</b>", "")
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

fn friendly_button_label(label: &str) -> String {
    match label {
        "logs" => crate::tr("rust.crash.action.logs").to_owned(),
        "open-mods" => crate::tr("rust.crash.action.mods").to_owned(),
        "open-config" => crate::tr("rust.crash.action.config").to_owned(),
        "open-game-root" => crate::tr("rust.crash.action.game-folder").to_owned(),
        "jre-recommended" => crate::tr("rust.crash.action.recommended-java").to_owned(),
        "update-java" => crate::tr("rust.crash.action.update-java").to_owned(),
        "install-java64" => crate::tr("rust.crash.action.java-64-bit").to_owned(),
        "open-settings" => crate::tr("rust.crash.action.settings").to_owned(),
        "fix-stack-size" => crate::tr("rust.crash.action.stack-size").to_owned(),
        "force-update" => crate::tr("rust.crash.action.force-update").to_owned(),
        other => other.replace('-', " "),
    }
}
