#![windows_subsystem = "windows"]

use std::path::PathBuf;
use std::sync::Arc;
use oos_lite_core::StorageEngine;

mod mount;
mod ui;
mod tray;

fn show_alert(title: &str, message: &str) {
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        #[link(name = "user32")]
        extern "system" {
            fn MessageBoxW(hwnd: *mut std::ffi::c_void, text: *const u16, caption: *const u16, utype: u32) -> i32;
        }

        fn to_wide(s: &str) -> Vec<u16> {
            OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
        }

        let text = to_wide(message);
        let caption = to_wide(title);
        unsafe {
            MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), 0x00000040 /* MB_ICONINFORMATION | MB_OK */);
        }
    }
    #[cfg(not(windows))]
    {
        eprintln!("{}: {}", title, message);
    }
}

pub fn open_browser_window(url: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        let edge_paths = [
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
            r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        ];
        let profile_dir = std::env::temp_dir().join("oos_lite_desktop_profile");
        let profile_arg = format!("--user-data-dir={}", profile_dir.display());
        for p in &edge_paths {
            if std::path::Path::new(p).exists() {
                let _ = std::process::Command::new(p)
                    .args([
                        &format!("--app={}", url),
                        &profile_arg,
                        "--no-first-run",
                        "--no-default-browser-check",
                    ])
                    .creation_flags(CREATE_NO_WINDOW)
                    .spawn();
                return;
            }
        }
        let _ = std::process::Command::new("explorer.exe")
            .arg(url)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

fn resolve_store_dir() -> PathBuf {
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if (args[i] == "--store" || args[i] == "-s") && i + 1 < args.len() {
            return PathBuf::from(&args[i + 1]);
        }
    }
    if let Ok(val) = std::env::var("OOS_STORE_DIR") {
        if !val.is_empty() {
            return PathBuf::from(val);
        }
    }

    // Check if .oos-store exists in current working directory
    let cur_store = PathBuf::from(".oos-store");
    if cur_store.is_dir() {
        return cur_store;
    }

    // Default to %USERPROFILE%\.oos-store
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let p = PathBuf::from(profile).join(".oos-store");
        return p;
    }

    if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(home).join(".oos-store");
        return p;
    }

    cur_store
}

fn resolve_password(store_dir: &std::path::Path) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if args[i] == "--password" && i + 1 < args.len() {
            return Some(args[i + 1].clone());
        }
        if args[i] == "--password-file" && i + 1 < args.len() {
            if let Ok(content) = std::fs::read_to_string(&args[i + 1]) {
                return Some(content.trim_end_matches(&['\r', '\n'][..]).to_string());
            }
        }
    }

    if let Ok(val) = std::env::var("OOS_PASSWORD") {
        if !val.is_empty() {
            return Some(val);
        }
    }

    // Check %LOCALAPPDATA%\oos-lite\vault.pass
    if let Ok(local_appdata) = std::env::var("LOCALAPPDATA") {
        let pass_file = PathBuf::from(local_appdata).join("oos-lite").join("vault.pass");
        if pass_file.is_file() {
            if let Ok(content) = std::fs::read_to_string(pass_file) {
                let trimmed = content.trim_end_matches(&['\r', '\n'][..]).to_string();
                if !trimmed.is_empty() {
                    return Some(trimmed);
                }
            }
        }
    }

    // Check <store_dir>\vault.pass
    let local_pass = store_dir.join("vault.pass");
    if local_pass.is_file() {
        if let Ok(content) = std::fs::read_to_string(local_pass) {
            let trimmed = content.trim_end_matches(&['\r', '\n'][..]).to_string();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }

    None
}

fn main() {
    let store_dir = resolve_store_dir();
    let password = resolve_password(&store_dir);

    // If port 3000 is already active, try to just focus the UI
    let test_stream = std::net::TcpStream::connect("127.0.0.1:3000");
    if test_stream.is_ok() {
        // App is already running, open browser window directly
        open_browser_window("http://127.0.0.1:3000");
        return;
    }

    // Ensure store directory exists
    if !store_dir.exists() {
        let _ = std::fs::create_dir_all(&store_dir);
    }

    let engine = if let Some(ref pwd) = password {
        let vault_key_file = store_dir.join("vault.key");
        if vault_key_file.exists() {
            // Store is encrypted — unlock it with the provided password
            match StorageEngine::open_with_password(&store_dir, pwd) {
                Ok(eng) => eng,
                Err(e) => {
                    show_alert("OOS-Lite - Wrong Password", &format!("Failed to unlock encrypted vault: {}", e));
                    return;
                }
            }
        } else if store_dir.exists() && store_dir.read_dir().map(|mut d| d.next().is_some()).unwrap_or(false) {
            // Store directory exists and has content but NO vault.key → plaintext store
            // Ignore the saved password and open unencrypted (user had data before encryption was added)
            match StorageEngine::open(&store_dir) {
                Ok(eng) => eng,
                Err(e) => {
                    show_alert("OOS-Lite Error", &format!("Failed to open store: {}", e));
                    return;
                }
            }
        } else {
            // Store is empty or doesn't exist → create new encrypted store
            match StorageEngine::open_with_password(&store_dir, pwd) {
                Ok(eng) => eng,
                Err(e) => {
                    show_alert("OOS-Lite Error", &format!("Failed to initialize encrypted vault: {}", e));
                    return;
                }
            }
        }
    } else {
        match StorageEngine::open(&store_dir) {
            Ok(eng) => eng,
            Err(oos_lite_core::error::OosLiteError::PasswordRequired) => {
                show_alert(
                    "OOS-Lite - Yêu Cầu Mật Khẩu",
                    "Kho lưu trữ OOS-Lite này đã được mã hóa. Vui lòng thiết lập mật khẩu trong file vault.pass hoặc biến môi trường OOS_PASSWORD.",
                );
                return;
            }
            Err(e) => {
                show_alert("OOS-Lite Error", &format!("Lỗi khởi động storage engine: {}", e));
                return;
            }
        }
    };

    // Spawn native Windows System Tray icon
    tray::windows::spawn_system_tray();

    let _ = ui::start_ui_server(Arc::new(engine), "127.0.0.1", 3000, false, true);
}
