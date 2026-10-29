pub async fn upload_diagnostic_report(report: String) -> Result<String, String> {
    if report.trim().is_empty() {
        return Err("The diagnostic report is empty.".to_owned());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| format!("Could not create diagnostic upload request: {error}"))?;
    let response = client
        .post("https://pasta.llaun.ch/create/v1")
        .header(reqwest::header::CONTENT_TYPE, "text/x-ansi; charset=utf-8")
        .body(report)
        .send()
        .await
        .map_err(|error| format!("Could not upload diagnostics: {error}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("Could not read diagnostic upload response: {error}"))?;
    if status.as_u16() == 413 {
        return Err("The diagnostic report is too large to upload.".to_owned());
    }
    if status.as_u16() == 429 {
        return Err("The diagnostic service is rate limiting uploads. Try again later.".to_owned());
    }
    if status.as_u16() != 201 {
        return Err(format!(
            "The diagnostic service returned HTTP {status}: {}",
            body.trim()
        ));
    }
    let url = url::Url::parse(body.trim())
        .map_err(|error| format!("The diagnostic service returned an invalid link: {error}"))?;
    if !matches!(url.scheme(), "https" | "http") {
        return Err("The diagnostic service returned an unsafe link.".to_owned());
    }
    Ok(url.to_string())
}
