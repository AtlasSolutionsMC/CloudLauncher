#[cfg(windows)]
use std::{collections::BTreeMap, process::Command};

pub fn system_info() -> String {
    let mut lines = vec!["System information".to_owned()];
    lines.push(format!("Architecture: {}", std::env::consts::ARCH));

    #[cfg(windows)]
    lines.extend(windows_system_info());
    #[cfg(not(windows))]
    lines.push(format!("Operating system: {}", std::env::consts::OS));

    lines.join("\n")
}

#[cfg(windows)]
fn windows_system_info() -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(version) = query_windows_version() {
        lines.push(format!("Operating system: {version}"));
    } else {
        lines.push("Operating system: Windows (version unavailable)".to_owned());
    }

    if let Ok(processor) = std::env::var("PROCESSOR_IDENTIFIER") {
        let processor = processor.trim();
        if !processor.is_empty() {
            lines.push(format!("Processor: {processor}"));
        }
    }
    if let Ok(processors) = std::thread::available_parallelism() {
        lines.push(format!("Logical processors: {}", processors.get()));
    }
    if let Some(memory) = total_physical_memory() {
        lines.push(format!(
            "Physical memory: {:.1} GiB",
            memory as f64 / 1_073_741_824.0
        ));
    }

    let adapters = display_adapters();
    if adapters.is_empty() {
        lines.push("Graphics adapters: unavailable".to_owned());
    } else {
        lines.extend(
            adapters
                .into_iter()
                .enumerate()
                .map(|(index, adapter)| format!("Graphics adapter {}: {adapter}", index + 1)),
        );
    }
    lines
}

#[cfg(windows)]
fn query_windows_version() -> Option<String> {
    let output = Command::new("reg.exe")
        .args([
            "query",
            r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let values = parse_registry_values(&String::from_utf8_lossy(&output.stdout));
    let product = values.get("ProductName")?;
    let display_version = values.get("DisplayVersion");
    let build = values.get("CurrentBuild");
    let mut version = product.clone();
    if let Some(display_version) = display_version.filter(|value| !value.is_empty()) {
        version.push_str(&format!(" ({display_version}"));
        if let Some(build) = build.filter(|value| !value.is_empty()) {
            version.push_str(&format!(", build {build}"));
        }
        version.push(')');
    } else if let Some(build) = build.filter(|value| !value.is_empty()) {
        version.push_str(&format!(" (build {build})"));
    }
    Some(version)
}

#[cfg(windows)]
fn display_adapters() -> Vec<String> {
    const DISPLAY_CLASS_KEY: &str =
        r"HKLM\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
    let Ok(output) = Command::new("reg.exe")
        .args(["query", DISPLAY_CLASS_KEY, "/s"])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut current_key = String::new();
    let mut values_by_key = BTreeMap::<String, (Option<String>, Option<String>)>::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with("HKEY_LOCAL_MACHINE\\") {
            current_key.clear();
            current_key.push_str(line);
            continue;
        }
        let mut fields = line.split_whitespace();
        let Some(name) = fields.next() else { continue };
        let Some(_) = fields.next() else { continue };
        let value = fields.collect::<Vec<_>>().join(" ");
        let entry = values_by_key.entry(current_key.clone()).or_default();
        match name {
            "DriverDesc" if !value.is_empty() => entry.0 = Some(value),
            "DriverVersion" if !value.is_empty() => entry.1 = Some(value),
            _ => {}
        }
    }

    values_by_key
        .into_values()
        .filter_map(|(description, driver)| {
            let description = description?;
            Some(match driver {
                Some(driver) => format!("{description} (driver {driver})"),
                None => description,
            })
        })
        .collect()
}

#[cfg(windows)]
fn parse_registry_values(output: &str) -> BTreeMap<String, String> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let name = fields.next()?;
            let _kind = fields.next()?;
            let value = fields.collect::<Vec<_>>().join(" ");
            (!value.is_empty()).then(|| (name.to_owned(), value))
        })
        .collect()
}

#[cfg(windows)]
fn total_physical_memory() -> Option<u64> {
    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_physical: u64,
        available_physical: u64,
        total_page_file: u64,
        available_page_file: u64,
        total_virtual: u64,
        available_virtual: u64,
        available_extended_virtual: u64,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
    }

    let mut status = MemoryStatusEx {
        length: std::mem::size_of::<MemoryStatusEx>() as u32,
        memory_load: 0,
        total_physical: 0,
        available_physical: 0,
        total_page_file: 0,
        available_page_file: 0,
        total_virtual: 0,
        available_virtual: 0,
        available_extended_virtual: 0,
    };
    let succeeded = unsafe { GlobalMemoryStatusEx(&mut status) } != 0;
    (succeeded && status.total_physical > 0).then_some(status.total_physical)
}
