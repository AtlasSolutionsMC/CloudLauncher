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
const ELY_PROFILE: &str =
    "https://account.ely.by/api/minecraft/session/profile/ffb3378cd561502fa78a08494be68811";

#[derive(Debug, Clone)]
pub struct ServiceCheck {
    pub id: &'static str,
    pub hosts: Vec<&'static str>,
    pub reachable: bool,
    pub critical: bool,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub services: Vec<ServiceCheck>,
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
        Err(_) => return unavailable_report(),
    };

    let brand = option_env!("LEGACYLAUNCHER_SHORT_BRAND").unwrap_or("develop");
    let bootstrap_urls =
        BOOTSTRAP_HOSTS.map(|host| format!("https://{host}/{brand}/bootstrap.json.mgz.signed"));
    let (mojang, repo_llaunch, repo_ru, repo_services, bootstrap_a, bootstrap_b, ely) = tokio::join!(
        check_json(&client, MOJANG_VERSION_MANIFEST),
        check_json(&client, EXTRA_REPOSITORIES[0]),
        check_json(&client, EXTRA_REPOSITORIES[1]),
        check_json(&client, EXTRA_REPOSITORIES[2]),
        check_http(&client, &bootstrap_urls[0]),
        check_http(&client, &bootstrap_urls[1]),
        check_json(&client, ELY_PROFILE),
    );

    Report {
        services: vec![
            ServiceCheck {
                id: "official_repo",
                hosts: vec!["launchermeta.mojang.com"],
                reachable: mojang,
                critical: true,
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
            },
            ServiceCheck {
                id: "boot",
                hosts: BOOTSTRAP_HOSTS.to_vec(),
                reachable: bootstrap_a || bootstrap_b,
                critical: true,
            },
            ServiceCheck {
                id: "account.ely.by",
                hosts: vec!["account.ely.by"],
                reachable: ely,
                critical: false,
            },
        ],
    }
}

async fn check_json(client: &Client, url: &str) -> bool {
    let Ok(response) = client.get(url).send().await else {
        return false;
    };
    if !response.status().is_success() {
        return false;
    }
    response.json::<Value>().await.is_ok()
}

async fn check_http(client: &Client, url: &str) -> bool {
    client
        .get(url)
        .send()
        .await
        .is_ok_and(|response| response.status().is_success())
}

pub fn unavailable_report() -> Report {
    Report {
        services: vec![
            ServiceCheck {
                id: "official_repo",
                hosts: vec!["launchermeta.mojang.com"],
                reachable: false,
                critical: true,
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
            },
            ServiceCheck {
                id: "boot",
                hosts: BOOTSTRAP_HOSTS.to_vec(),
                reachable: false,
                critical: true,
            },
            ServiceCheck {
                id: "account.ely.by",
                hosts: vec!["account.ely.by"],
                reachable: false,
                critical: false,
            },
        ],
    }
}
