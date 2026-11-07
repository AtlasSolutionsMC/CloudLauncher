use super::super::*;

pub(in crate::app) fn open_path(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let result = Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = Command::new("xdg-open").arg(path).spawn();
    result.map(|_| ()).map_err(|error| {
        tr("rust.status.folder.open-error")
            .replace("%0", &path.display().to_string())
            .replace("%1", &error.to_string())
    })
}

#[cfg(target_os = "windows")]
pub(in crate::app) fn apply_gpu_preference(
    java_path: &str,
    preference: &str,
) -> Result<(), String> {
    let value = match preference {
        "integrated" => "GpuPreference=1;",
        "discrete" => "GpuPreference=2;",
        _ => return Ok(()),
    };
    let java_path = PathBuf::from(java_path).canonicalize().map_err(|error| {
        tr("rust.status.java.path-unavailable").replace("%0", &error.to_string())
    })?;
    let current_version = Command::new("reg.exe")
        .args([
            "query",
            r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion",
            "/v",
            "CurrentBuildNumber",
        ])
        .output()
        .map_err(|error| {
            tr("rust.status.game.gpu-windows-build-read").replace("%0", &error.to_string())
        })?;
    if !current_version.status.success() {
        return Err(tr("rust.status.game.gpu-windows-build-read-failed").to_owned());
    }
    let build = String::from_utf8_lossy(&current_version.stdout)
        .split_whitespace()
        .filter_map(|field| field.parse::<u32>().ok())
        .next_back()
        .ok_or_else(|| tr("rust.status.game.gpu-windows-build-parse").to_owned())?;
    if build < 20_190 {
        return Err(tr("rust.status.game.gpu-windows-build-old").to_owned());
    }
    let java_path = java_path.to_string_lossy().into_owned();
    let output = Command::new("reg.exe")
        .args([
            "add",
            r"HKCU\Software\Microsoft\DirectX\UserGpuPreferences",
            "/v",
            &java_path,
            "/t",
            "REG_SZ",
            "/d",
            value,
            "/f",
        ])
        .output()
        .map_err(|error| {
            tr("rust.status.game.gpu-registry-update").replace("%0", &error.to_string())
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

#[cfg(not(target_os = "windows"))]
pub(in crate::app) fn apply_gpu_preference(
    _java_path: &str,
    preference: &str,
) -> Result<(), String> {
    if preference == "system" {
        Ok(())
    } else {
        Err(tr("rust.status.game.gpu-windows-only").to_owned())
    }
}

pub(in crate::app) fn open_crash_folder(
    game_directory: &std::path::Path,
    folder: &str,
) -> Result<(), String> {
    let relative = folder.strip_prefix("./").unwrap_or(folder);
    let relative_path = std::path::Path::new(relative);
    if relative_path.is_absolute()
        || relative_path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(tr("rust.status.crash.folder-outside-game").to_owned());
    }
    let base = match game_directory.canonicalize() {
        Ok(base) => base,
        Err(error) => {
            return Err(tr("rust.status.crash.game-folder-error").replace("%0", &error.to_string()));
        }
    };
    let target = base.join(relative_path);
    if relative_path == std::path::Path::new("mods") && !target.exists() {
        if let Err(error) = fs::create_dir_all(&target) {
            return Err(tr("rust.status.crash.mods-create-error").replace("%0", &error.to_string()));
        }
    }
    let target = match target.canonicalize() {
        Ok(target) if target.starts_with(&base) => target,
        _ => return Err(tr("rust.status.crash.folder-missing").to_owned()),
    };
    open_path(&target)
}

pub(crate) fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let result = open_url_windows(url);
    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = Command::new("xdg-open").arg(url).spawn();
    result.map(|_| ()).map_err(|error| {
        tr("rust.status.url.open-error")
            .replace("%0", url)
            .replace("%1", &error.to_string())
    })
}

#[cfg(target_os = "windows")]
fn open_url_windows(url: &str) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};

    let operation = "open".encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let url = std::ffi::OsStr::new(url)
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            url.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    let result_code = result as isize;
    if result_code <= 32 {
        return Err(format!("Windows URL handler returned {result_code}"));
    }
    Ok(())
}
