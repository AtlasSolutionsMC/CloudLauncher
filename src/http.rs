use std::sync::atomic::{AtomicBool, Ordering};

static VERIFY_CERTIFICATES: AtomicBool = AtomicBool::new(true);

pub fn set_certificate_validation(verify: bool) {
    VERIFY_CERTIFICATES.store(verify, Ordering::Release);
    minecraft_java_rs_core::net::client::set_certificate_validation(verify);
}

pub fn client_builder() -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder();
    if VERIFY_CERTIFICATES.load(Ordering::Acquire) {
        builder
    } else {
        builder
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true)
    }
}
