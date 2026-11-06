use std::{collections::HashMap, fs, io, path::PathBuf};

fn vault_path() -> PathBuf {
    let root = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    root.join("LegacyLauncher-Rust").join("credentials.dpapi")
}

pub fn load_refresh_token(account_id: &str) -> io::Result<Option<String>> {
    let path = vault_path();
    let encrypted = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let plaintext = dpapi::unprotect(&encrypted)?;
    let tokens: HashMap<String, String> = serde_json::from_slice(&plaintext)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(tokens.get(account_id).cloned())
}

pub fn store_refresh_token(account_id: &str, refresh_token: &str) -> io::Result<()> {
    let mut tokens = load_all()?;
    tokens.insert(account_id.to_owned(), refresh_token.to_owned());
    save_all(&tokens)
}

pub fn store_refresh_token_replacing_accounts(
    account_id: &str,
    refresh_token: &str,
    replaced_account_ids: &[String],
) -> io::Result<()> {
    let mut tokens = load_all()?;
    for replaced_account_id in replaced_account_ids {
        tokens.remove(replaced_account_id);
    }
    tokens.insert(account_id.to_owned(), refresh_token.to_owned());
    save_all(&tokens)
}

pub fn remove_account(account_id: &str) -> io::Result<()> {
    let mut tokens = load_all()?;
    if tokens.remove(account_id).is_some() {
        if tokens.is_empty() {
            match fs::remove_file(vault_path()) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        } else {
            save_all(&tokens)?;
        }
    }
    Ok(())
}

fn load_all() -> io::Result<HashMap<String, String>> {
    let path = vault_path();
    let encrypted = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => return Err(error),
    };
    let plaintext = dpapi::unprotect(&encrypted)?;
    serde_json::from_slice(&plaintext)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn save_all(tokens: &HashMap<String, String>) -> io::Result<()> {
    let path = vault_path();
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "vault path has no parent"))?;
    fs::create_dir_all(parent)?;
    let plaintext = serde_json::to_vec(tokens)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let encrypted = dpapi::protect(&plaintext)?;
    fs::write(path, encrypted)
}

#[cfg(windows)]
mod dpapi {
    use std::{io, ptr};

    #[repr(C)]
    struct DataBlob {
        size: u32,
        data: *mut u8,
    }

    #[link(name = "crypt32")]
    unsafe extern "system" {
        fn CryptProtectData(
            input: *const DataBlob,
            description: *const u16,
            entropy: *const DataBlob,
            reserved: *const std::ffi::c_void,
            prompt: *const std::ffi::c_void,
            flags: u32,
            output: *mut DataBlob,
        ) -> i32;
        fn CryptUnprotectData(
            input: *const DataBlob,
            description: *mut *mut u16,
            entropy: *const DataBlob,
            reserved: *const std::ffi::c_void,
            prompt: *const std::ffi::c_void,
            flags: u32,
            output: *mut DataBlob,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(memory: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    }

    pub fn protect(bytes: &[u8]) -> io::Result<Vec<u8>> {
        transform(bytes, true)
    }

    pub fn unprotect(bytes: &[u8]) -> io::Result<Vec<u8>> {
        transform(bytes, false)
    }

    fn transform(bytes: &[u8], protect: bool) -> io::Result<Vec<u8>> {
        let size = u32::try_from(bytes.len()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "credential data is too large")
        })?;
        let input = DataBlob {
            size,
            data: bytes.as_ptr().cast_mut(),
        };
        let mut output = DataBlob {
            size: 0,
            data: ptr::null_mut(),
        };
        let flags = 0x1; // CRYPTPROTECT_UI_FORBIDDEN
        let success = unsafe {
            if protect {
                CryptProtectData(
                    &input,
                    ptr::null(),
                    ptr::null(),
                    ptr::null(),
                    ptr::null(),
                    flags,
                    &mut output,
                )
            } else {
                CryptUnprotectData(
                    &input,
                    ptr::null_mut(),
                    ptr::null(),
                    ptr::null(),
                    ptr::null(),
                    flags,
                    &mut output,
                )
            }
        };
        if success == 0 {
            return Err(io::Error::last_os_error());
        }
        if output.data.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Windows credential protection returned no data",
            ));
        }

        let result =
            unsafe { std::slice::from_raw_parts(output.data, output.size as usize).to_vec() };
        unsafe {
            LocalFree(output.data.cast());
        }
        Ok(result)
    }
}

#[cfg(not(windows))]
mod dpapi {
    use std::io;

    pub fn protect(_bytes: &[u8]) -> io::Result<Vec<u8>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Microsoft credentials require Windows DPAPI",
        ))
    }

    pub fn unprotect(_bytes: &[u8]) -> io::Result<Vec<u8>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Microsoft credentials require Windows DPAPI",
        ))
    }
}
