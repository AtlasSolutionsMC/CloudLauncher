pub async fn upload_diagnostic_report(report: String) -> Result<String, String> {
    if report.trim().is_empty() {
        return Err(crate::tr("rust.error.support.report-empty").to_owned());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| {
            crate::tr("rust.error.support.client").replace("%0", &error.to_string())
        })?;
    let response = client
        .post("https://pasta.llaun.ch/create/v1")
        .header(reqwest::header::CONTENT_TYPE, "text/x-ansi; charset=utf-8")
        .body(report)
        .send()
        .await
        .map_err(|error| {
            crate::tr("rust.error.support.upload").replace("%0", &error.to_string())
        })?;
    let status = response.status();
    let body = response.text().await.map_err(|error| {
        crate::tr("rust.error.support.response-read").replace("%0", &error.to_string())
    })?;
    if status.as_u16() == 413 {
        return Err(crate::tr("rust.error.support.report-large").to_owned());
    }
    if status.as_u16() == 429 {
        return Err(crate::tr("rust.error.support.rate-limited").to_owned());
    }
    if status.as_u16() != 201 {
        return Err(crate::tr("rust.error.support.http")
            .replace("%0", &status.to_string())
            .replace("%1", body.trim()));
    }
    let url = url::Url::parse(body.trim()).map_err(|error| {
        crate::tr("rust.error.support.link-invalid").replace("%0", &error.to_string())
    })?;
    if !matches!(url.scheme(), "https" | "http") {
        return Err(crate::tr("rust.error.support.link-unsafe").to_owned());
    }
    Ok(url.to_string())
}
