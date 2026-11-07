use super::super::*;

pub(in crate::app) fn record_game_log_line(buffer: &Arc<Mutex<VecDeque<String>>>, line: String) {
    if let Ok(mut lines) = buffer.lock() {
        if lines.len() >= 2500 {
            lines.pop_front();
        }
        lines.push_back(line);
    }
}

pub(in crate::app) fn record_launcher_log_line(
    buffer: &Arc<Mutex<VecDeque<LogRecord>>>,
    message: &str,
) {
    record_launcher_log_event(buffer, LogLevel::Info, LogTarget::Launcher, message);
}

pub(in crate::app) fn record_launcher_log_event(
    buffer: &Arc<Mutex<VecDeque<LogRecord>>>,
    level: LogLevel,
    target: LogTarget,
    message: &str,
) {
    const BUFFER_CAPACITY: usize = 1024 * 1024;
    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = now.as_secs() % 86_400;
    let timestamp = format!(
        "{:02}:{:02}:{:02}.{:03}",
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60,
        now.subsec_millis()
    );
    let redacted = redact_sensitive_log_values(message);
    let mut records_to_write = Vec::new();
    for message_line in redacted.lines().take(256) {
        let mut record = LogRecord {
            timestamp: timestamp.clone(),
            level,
            target,
            message: message_line.to_owned(),
        };
        if record.rendered_len() > 32 * 1024 {
            let fixed_length = record.rendered_len() - record.message.len();
            record.message = truncate_utf8_to_bytes(&record.message, 32 * 1024 - fixed_length);
        }
        let line = record.render();
        if let Ok(mut lines) = buffer.lock() {
            let mut current_size = lines.iter().map(LogRecord::rendered_len).sum::<usize>();
            while current_size + line.len() > BUFFER_CAPACITY {
                let Some(removed) = lines.pop_front() else {
                    break;
                };
                current_size = current_size.saturating_sub(removed.rendered_len());
            }
            lines.push_back(record.clone());
        }
        records_to_write.push(format!("{line}\n"));
    }
    for line in records_to_write {
        append_launcher_log_file(line.as_bytes());
    }
}

pub(in crate::app) fn truncate_utf8_to_bytes(value: &str, limit: usize) -> String {
    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

pub(in crate::app) fn redact_sensitive_log_values(message: &str) -> String {
    use std::sync::OnceLock;

    static KEY_VALUE: OnceLock<regex::Regex> = OnceLock::new();
    static BEARER: OnceLock<regex::Regex> = OnceLock::new();
    let key_value = KEY_VALUE.get_or_init(|| {
        regex::Regex::new(
            r#"(?i)((?:access|refresh|id|client)[_-]?(?:token|secret)|authorization|password|secret|token)(["']?\s*[:=]\s*["']?)[^&\s,;"']+"#,
        )
        .expect("static sensitive-value expression is valid")
    });
    let bearer = BEARER.get_or_init(|| {
        regex::Regex::new(r"(?i)(bearer\s+)[A-Za-z0-9._~+/-]+=*")
            .expect("static bearer-token expression is valid")
    });
    let without_bearer = bearer.replace_all(message, "$1[REDACTED]");
    key_value
        .replace_all(&without_bearer, "$1$2[REDACTED]")
        .into_owned()
}

pub(in crate::app) fn append_launcher_log_file(line: &[u8]) {
    const FILE_CAPACITY: u64 = 12 * 1024 * 1024;
    let path = launcher_log_path();
    let Some(directory) = path.parent() else {
        return;
    };
    if fs::create_dir_all(directory).is_err() {
        return;
    }
    let current_size = fs::metadata(&path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if current_size.saturating_add(line.len() as u64) > FILE_CAPACITY {
        for index in (1..10).rev() {
            let source = path.with_file_name(format!("launcher.log.{index}"));
            let target = path.with_file_name(format!("launcher.log.{}", index + 1));
            if source.exists() {
                let _ = fs::remove_file(&target);
                let _ = fs::rename(source, target);
            }
        }
        let first_backup = path.with_file_name("launcher.log.1");
        let _ = fs::remove_file(&first_backup);
        let _ = fs::rename(&path, first_backup);
    }
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line);
    }
}

pub(in crate::app) fn append_launcher_diagnostic(
    level: LogLevel,
    target: LogTarget,
    message: &str,
) {
    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = now.as_secs() % 86_400;
    let timestamp = format!(
        "{:02}:{:02}:{:02}.{:03}",
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60,
        now.subsec_millis()
    );
    let redacted = redact_sensitive_log_values(message);
    for line in redacted.lines().take(256) {
        let mut record = LogRecord {
            timestamp: timestamp.clone(),
            level,
            target,
            message: line.to_owned(),
        };
        if record.rendered_len() > 32 * 1024 {
            let fixed_length = record.rendered_len() - record.message.len();
            record.message = truncate_utf8_to_bytes(&record.message, 32 * 1024 - fixed_length);
        }
        append_launcher_log_file(format!("{}\n", record.render()).as_bytes());
    }
}

pub(in crate::app) fn install_process_panic_logger() {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        previous_hook(panic_info);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let thread_name = std::thread::current()
                .name()
                .unwrap_or("<unnamed>")
                .to_owned();
            let payload = panic_info
                .payload()
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| {
                    panic_info
                        .payload()
                        .downcast_ref::<String>()
                        .map(String::as_str)
                })
                .unwrap_or("non-string panic payload");
            let location = panic_info
                .location()
                .map(|location| {
                    format!(
                        "{}:{}:{}",
                        location.file(),
                        location.line(),
                        location.column()
                    )
                })
                .unwrap_or_else(|| "unknown location".to_owned());
            append_launcher_diagnostic(
                LogLevel::Error,
                LogTarget::Launcher,
                &format!("Unhandled panic in thread {thread_name} at {location}: {payload}"),
            );
            append_launcher_diagnostic(
                LogLevel::Error,
                LogTarget::Launcher,
                &format!(
                    "Panic backtrace:\n{}",
                    std::backtrace::Backtrace::force_capture()
                ),
            );
        }));
    }));
}

pub(in crate::app) fn collect_crash_report(
    game_dir: &std::path::Path,
    game_log: &Arc<Mutex<VecDeque<String>>>,
    launch_started: SystemTime,
    version: &str,
    loader: &str,
    exit_code: i32,
    runtime: &crash::CrashRuntimeInfo,
) -> Option<CrashReport> {
    let cutoff = launch_started
        .checked_sub(Duration::from_secs(3))
        .unwrap_or(std::time::UNIX_EPOCH);
    let report_path = newest_recent_log(&game_dir.join("crash-reports"), cutoff, "txt")
        .or_else(|| newest_recent_log(&game_dir.join("logs"), cutoff, "log"));
    let disk_log = report_path.as_ref().and_then(|path| read_log_tail(path));
    let log = disk_log.unwrap_or_else(|| {
        game_log
            .lock()
            .map(|lines| lines.iter().cloned().collect::<Vec<_>>().join("\n"))
            .unwrap_or_default()
    });
    let mut suggestions = match crash::diagnose(&log, version, loader, exit_code, runtime) {
        crash::CrashDiagnosis::Fake => return None,
        crash::CrashDiagnosis::Known(suggestion) => vec![suggestion],
        crash::CrashDiagnosis::Unknown => Vec::new(),
    };
    if suggestions.is_empty() {
        suggestions = if log.trim().is_empty() {
            vec![crash::CrashSuggestion {
                title: tr("crash.unknown.title").to_owned(),
                body: tr("crash.unknown.body").to_owned(),
                image: None,
                permit_help: true,
                actions: Vec::new(),
            }]
        } else {
            diagnose_crash_fallback(&log)
                .into_iter()
                .map(|body| crash::CrashSuggestion {
                    title: tr("rust.ui.crash.fallback-title").to_owned(),
                    body,
                    image: None,
                    permit_help: true,
                    actions: Vec::new(),
                })
                .collect()
        };
    }
    let summary = if let Some(first) = suggestions.first() {
        tr("rust.crash.summary-with-suggestion").replace("%0", &first.title)
    } else {
        tr("rust.crash.summary-no-suggestion").to_owned()
    };
    Some(CrashReport {
        path: report_path,
        game_directory: game_dir.to_path_buf(),
        version: version.to_owned(),
        loader: loader.to_owned(),
        summary,
        suggestions,
        log,
    })
}

pub(in crate::app) fn newest_recent_log(
    directory: &std::path::Path,
    cutoff: SystemTime,
    extension: &str,
) -> Option<PathBuf> {
    fs::read_dir(directory)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file()
                || !path
                    .extension()
                    .is_some_and(|value| value.to_string_lossy().eq_ignore_ascii_case(extension))
            {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            (modified >= cutoff).then_some((path, modified))
        })
        .max_by_key(|(_, modified)| *modified)
        .map(|(path, _)| path)
}

pub(in crate::app) fn process_exit_code(status: &std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        return status.signal().map(|signal| 128 + signal).unwrap_or(-1);
    }
    #[cfg(not(unix))]
    {
        -1
    }
}

pub(in crate::app) fn list_managed_java_runtimes(root: &std::path::Path) -> Vec<PathBuf> {
    let mut found = HashSet::new();
    let mut pending = vec![(root.to_path_buf(), 0_u8)];
    while let Some((directory, depth)) = pending.pop() {
        if depth > 6 {
            continue;
        }
        if directory
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("bin"))
        {
            for executable in ["javaw.exe", "java.exe", "java"] {
                let candidate = directory.join(executable);
                if candidate.is_file() {
                    found.insert(candidate);
                    break;
                }
            }
        }
        if let Ok(entries) = fs::read_dir(&directory) {
            for entry in entries.flatten() {
                if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                    pending.push((entry.path(), depth.saturating_add(1)));
                }
            }
        }
    }
    let mut runtimes = found.into_iter().collect::<Vec<_>>();
    runtimes.sort();
    runtimes
}

pub(in crate::app) fn list_recent_logs(game_dir: &std::path::Path) -> Vec<(PathBuf, SystemTime)> {
    let mut files = [game_dir.join("logs"), game_dir.join("crash-reports")]
        .into_iter()
        .filter_map(|directory| fs::read_dir(directory).ok())
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let extension = path.extension()?.to_string_lossy();
            if !path.is_file()
                || !(extension.eq_ignore_ascii_case("log") || extension.eq_ignore_ascii_case("txt"))
            {
                return None;
            }
            Some((path, entry.metadata().ok()?.modified().ok()?))
        })
        .collect::<Vec<_>>();
    let launcher_log = launcher_log_path();
    for path in std::iter::once(launcher_log.clone())
        .chain((1..=10).map(|index| launcher_log.with_file_name(format!("launcher.log.{index}"))))
    {
        if let Ok(metadata) = fs::metadata(&path) {
            if metadata.is_file() {
                if let Ok(modified) = metadata.modified() {
                    files.push((path, modified));
                }
            }
        }
    }
    files.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
    files.truncate(100);
    files
}

pub(in crate::app) fn read_log_tail(path: &std::path::Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let start = file.metadata().ok()?.len().saturating_sub(64 * 1024);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::with_capacity(64 * 1024);
    file.read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

pub(in crate::app) fn diagnose_crash_fallback(log: &str) -> Vec<String> {
    let lower = log.to_ascii_lowercase();
    let mut suggestions = Vec::new();
    if lower.contains("outofmemoryerror")
        || lower.contains("could not reserve enough space for object heap")
    {
        suggestions.push(tr("rust.crash.fallback.out-of-memory").to_owned());
    }
    if lower.contains("unsupportedclassversionerror")
        || lower.contains("unsupported major.minor version")
    {
        suggestions.push(tr("rust.crash.fallback.java-incompatible").to_owned());
    }
    if lower.contains("could not find or load main class")
        || lower.contains("noclassdeffounderror")
        || lower.contains("classnotfoundexception")
    {
        suggestions.push(tr("rust.crash.fallback.game-file-missing").to_owned());
    }
    if lower.contains("glfw error") || lower.contains("failed to create window") {
        suggestions.push(tr("rust.crash.fallback.graphics-window").to_owned());
    }
    if lower.contains("duplicate mods") || lower.contains("duplicate mod") {
        suggestions.push(tr("rust.crash.fallback.mod-conflict").to_owned());
    }
    suggestions
}
