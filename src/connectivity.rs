use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

const MOJANG_VERSION_MANIFEST: &str =
    "https://launchermeta.mojang.com/mc/game/version_manifest.json";
const EXTRA_REPOSITORIES: [&str; 3] = [
    "https://repo.llaun.ch/versions/versions.json",
    "https://repo.legacylauncher.ru/versions/versions.json",
    "https://repo.legacylauncherservices.com/versions/versions.json",
];
const BOOTSTRAP_HOSTS: [&str; 2] = ["bootstrap.llaun.ch", "bootstrap.legacylauncher.ru"];
const PROXY_HOSTS: [&str; 2] = ["proxy.llaun.ch", "proxy.legacylauncher.ru"];
const ELY_PROFILE: &str =
    "https://account.ely.by/api/minecraft/session/profile/ffb3378cd561502fa78a08494be68811";
const DOOMSDAY_URLS: [&str; 2] = [
    "https://europe-west3-legacylauncher.cloudfunctions.net/doomsday",
    "https://functions.yandexcloud.net/d4e6s85mtp5rjisfrgil",
];

#[derive(Debug, Clone)]
pub struct ServiceCheck {
    pub id: &'static str,
    pub hosts: Vec<&'static str>,
    pub reachable: bool,
    pub critical: bool,
    pub failure_details: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub services: Vec<ServiceCheck>,
    pub doomsday_message: Option<String>,
}

impl Report {
    pub fn has_critical_failures(&self) -> bool {
        self.services
            .iter()
            .any(|service| service.critical && !service.reachable)
    }

    pub fn all_services_unreachable(&self) -> bool {
        !self.services.is_empty() && self.services.iter().all(|service| !service.reachable)
    }

    pub fn unavailable_hosts(&self) -> Vec<&'static str> {
        let mut hosts: Vec<_> = self
            .services
            .iter()
            .filter(|service| !service.reachable)
            .flat_map(|service| service.hosts.iter().copied())
            .collect();
        hosts.sort_unstable();
        hosts.dedup();
        hosts
    }
}

pub async fn check() -> Report {
    let client = match crate::http::client_builder()
        .timeout(Duration::from_secs(5))
        .user_agent(concat!("LegacyLauncher-Rust/", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return unavailable_report(&format!("Could not create HTTP client: {error}"));
        }
    };

    let brand = option_env!("LEGACYLAUNCHER_SHORT_BRAND").unwrap_or("develop");
    let bootstrap_urls =
        BOOTSTRAP_HOSTS.map(|host| format!("https://{host}/{brand}/bootstrap.json.mgz.signed"));
    let proxified_manifest_urls = PROXY_HOSTS.map(|host| {
        format!("https://{host}/launchermeta.mojang.com/mc/game/version_manifest.json")
    });
    let (
        mojang_result,
        repo_llaunch_result,
        repo_ru_result,
        repo_services_result,
        bootstrap_a_result,
        bootstrap_b_result,
        ely_result,
    ) = tokio::join!(
        check_json(&client, MOJANG_VERSION_MANIFEST),
        check_json(&client, EXTRA_REPOSITORIES[0]),
        check_json(&client, EXTRA_REPOSITORIES[1]),
        check_json(&client, EXTRA_REPOSITORIES[2]),
        check_http(&client, &bootstrap_urls[0]),
        check_http(&client, &bootstrap_urls[1]),
        check_json(&client, ELY_PROFILE),
    );
    let mojang_failure = failure_detail("launchermeta.mojang.com", mojang_result);
    let repo_llaunch_failure = failure_detail("repo.llaun.ch", repo_llaunch_result);
    let repo_ru_failure = failure_detail("repo.legacylauncher.ru", repo_ru_result);
    let repo_services_failure =
        failure_detail("repo.legacylauncherservices.com", repo_services_result);
    let bootstrap_a_failure = failure_detail(BOOTSTRAP_HOSTS[0], bootstrap_a_result);
    let bootstrap_b_failure = failure_detail(BOOTSTRAP_HOSTS[1], bootstrap_b_result);
    let ely_failure = failure_detail("account.ely.by", ely_result);
    let mojang = mojang_failure.is_none();
    let repo_llaunch = repo_llaunch_failure.is_none();
    let repo_ru = repo_ru_failure.is_none();
    let repo_services = repo_services_failure.is_none();
    let bootstrap_a = bootstrap_a_failure.is_none();
    let bootstrap_b = bootstrap_b_failure.is_none();
    let ely = ely_failure.is_none();
    let proxy_needed =
        !mojang || !(repo_llaunch || repo_ru || repo_services) || !(bootstrap_a || bootstrap_b);
    let (proxified_a_result, proxified_b_result) = if proxy_needed {
        tokio::join!(
            check_json(&client, &proxified_manifest_urls[0]),
            check_json(&client, &proxified_manifest_urls[1]),
        )
    } else {
        // Source queues proxy checks only after a normal-priority service fails.
        (Ok(()), Ok(()))
    };
    let proxified_a_failure = failure_detail(PROXY_HOSTS[0], proxified_a_result);
    let proxified_b_failure = failure_detail(PROXY_HOSTS[1], proxified_b_result);
    let proxified_a = proxified_a_failure.is_none();
    let proxified_b = proxified_b_failure.is_none();

    let mut report = Report {
        services: vec![
            ServiceCheck {
                id: "official_repo",
                hosts: vec!["launchermeta.mojang.com"],
                reachable: mojang,
                critical: true,
                failure_details: failures([mojang_failure]),
            },
            ServiceCheck {
                id: "official_repo_proxy",
                hosts: PROXY_HOSTS.to_vec(),
                reachable: proxified_a || proxified_b,
                critical: false,
                failure_details: failures([proxified_a_failure, proxified_b_failure]),
            },
            ServiceCheck {
                id: "repo",
                hosts: vec![
                    "repo.llaun.ch",
                    "repo.legacylauncher.ru",
                    "repo.legacylauncherservices.com",
                ],
                reachable: repo_llaunch || repo_ru || repo_services,
                critical: true,
                failure_details: failures([
                    repo_llaunch_failure,
                    repo_ru_failure,
                    repo_services_failure,
                ]),
            },
            ServiceCheck {
                id: "boot",
                hosts: BOOTSTRAP_HOSTS.to_vec(),
                reachable: bootstrap_a || bootstrap_b,
                critical: true,
                failure_details: failures([bootstrap_a_failure, bootstrap_b_failure]),
            },
            ServiceCheck {
                id: "account.ely.by",
                hosts: vec!["account.ely.by"],
                reachable: ely,
                critical: false,
                failure_details: failures([ely_failure]),
            },
        ],
        doomsday_message: None,
    };
    if report.has_critical_failures() {
        report.doomsday_message = fetch_doomsday_message(&client).await;
    }
    report
}

async fn fetch_doomsday_message(client: &Client) -> Option<String> {
    async fn fetch(client: &Client, url: &'static str) -> Option<String> {
        let response = client.get(url).send().await.ok()?;
        if !response.status().is_success() {
            return None;
        }
        let payload = response.json::<Value>().await.ok()?;
        let message = payload.get("messages")?.get("en_US")?.as_str()?.trim();
        (!message.is_empty()).then(|| message.to_owned())
    }

    let (primary, mirror) = tokio::join!(
        fetch(client, DOOMSDAY_URLS[0]),
        fetch(client, DOOMSDAY_URLS[1])
    );
    primary.or(mirror)
}

fn failure_detail(host: &str, result: Result<(), String>) -> Option<String> {
    result.err().map(|error| format!("{host}: {error}"))
}

fn failures<const N: usize>(details: [Option<String>; N]) -> Vec<String> {
    details.into_iter().flatten().collect()
}

async fn check_json(client: &Client, url: &str) -> Result<(), String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?;
    response
        .json::<Value>()
        .await
        .map(|_| ())
        .map_err(|error| format!("Invalid JSON: {error}"))
}

async fn check_http(client: &Client, url: &str) -> Result<(), String> {
    client
        .get(url)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub fn unavailable_report(reason: &str) -> Report {
    let mut report = Report {
        services: vec![
            ServiceCheck {
                id: "official_repo",
                hosts: vec!["launchermeta.mojang.com"],
                reachable: false,
                critical: true,
                failure_details: Vec::new(),
            },
            ServiceCheck {
                id: "official_repo_proxy",
                hosts: PROXY_HOSTS.to_vec(),
                reachable: false,
                critical: false,
                failure_details: Vec::new(),
            },
            ServiceCheck {
                id: "repo",
                hosts: vec![
                    "repo.llaun.ch",
                    "repo.legacylauncher.ru",
                    "repo.legacylauncherservices.com",
                ],
                reachable: false,
                critical: true,
                failure_details: Vec::new(),
            },
            ServiceCheck {
                id: "boot",
                hosts: BOOTSTRAP_HOSTS.to_vec(),
                reachable: false,
                critical: true,
                failure_details: Vec::new(),
            },
            ServiceCheck {
                id: "account.ely.by",
                hosts: vec!["account.ely.by"],
                reachable: false,
                critical: false,
                failure_details: Vec::new(),
            },
        ],
        doomsday_message: None,
    };
    if let Some(service) = report.services.first_mut() {
        service.failure_details.push(reason.to_owned());
    }
    report
}
