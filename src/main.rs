mod auth;
mod bootstrap;
mod catalog;
mod connectivity;
mod crash;
mod diagnostics;
mod ely;
mod ely_legacy;
mod github_update;
mod http;
mod malware;
mod notices;
mod support;
mod vault;

mod app;

pub(crate) use app::{open_url, tr, tr_dynamic};

fn main() -> eframe::Result {
    app::run()
}
