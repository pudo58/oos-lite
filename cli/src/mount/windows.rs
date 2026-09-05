use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{info, warn};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebClientStatus {
    Running,
    Stopped,
    NotFound,
    Unknown(String),
}

pub struct WindowsPreflightReport {
    pub service_status: WebClientStatus,
    pub file_size_limit_bytes: Option<u64>,
}

pub fn run_preflight_check() -> WindowsPreflightReport {
    let service_status = check_webclient_service();
    let file_size_limit_bytes = query_file_size_limit_registry();

    WindowsPreflightReport {
        service_status,
        file_size_limit_bytes,
    }
}

fn check_webclient_service() -> WebClientStatus {
    let output = match Command::new("sc.exe").args(["query", "WebClient"]).output() {
        Ok(out) => out,
        Err(_) => return WebClientStatus::Unknown("Failed to execute sc.exe".to_string()),
    };

    let text = String::from_utf8_lossy(&output.stdout);
    if text.contains("STATE") {
        if text.contains("RUNNING") {
            WebClientStatus::Running
        } else if text.contains("STOPPED") {
            WebClientStatus::Stopped
        } else {
            WebClientStatus::Unknown(text.to_string())
        }
    } else {
        let err_text = String::from_utf8_lossy(&output.stderr);
        if text.contains("FAILED 1060") || err_text.contains("1060") {
            WebClientStatus::NotFound
        } else {
            WebClientStatus::Unknown(format!("{} {}", text, err_text))
        }
    }
}

fn query_file_size_limit_registry() -> Option<u64> {
    let output = Command::new("reg.exe")
        .args([
            "query",
            r"HKLM\SYSTEM\CurrentControlSet\Services\WebClient\Parameters",
            "/v",
            "FileSizeLimitInBytes",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if line.contains("FileSizeLimitInBytes") && line.contains("REG_DWORD") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(hex_val) = parts.last() {
                let clean_hex = hex_val.trim_start_matches("0x");
                if let Ok(bytes) = u64::from_str_radix(clean_hex, 16) {
                    return Some(bytes);
                }
            }
        }
    }

    None
}

pub fn is_drive_mapped(drive_letter: char) -> bool {
    let drive_str = format!("{}:", drive_letter.to_ascii_uppercase());
    if let Ok(output) = Command::new("net.exe").args(["use"]).output() {
        let text = String::from_utf8_lossy(&output.stdout);
        return text.contains(&drive_str);
    }
    false
}

pub fn map_drive(drive_letter: char, port: u16) -> anyhow::Result<()> {
    let drive_str = format!("{}:", drive_letter.to_ascii_uppercase());
    let unc_path = format!(r"\\127.0.0.1@{port}\DavWWWRoot");

    // Clean up any stale mapping first if it already exists
    let _ = unmap_drive(drive_letter);

    info!(
        drive = %drive_str,
        unc = %unc_path,
        "Mapping Windows network drive via WebDAV redirector..."
    );

    let output = Command::new("net.exe")
        .args(["use", &drive_str, &unc_path, "/persistent:no"])
        .output()?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let out = String::from_utf8_lossy(&output.stdout);
        anyhow::bail!(
            "Failed to map drive {} to {}.\nSTDOUT: {}\nSTDERR: {}",
            drive_str,
            unc_path,
            out.trim(),
            err.trim()
        );
    }

    println!("==> Successfully mapped OOS-Lite WebDAV drive to {}", drive_str);
    Ok(())
}

pub fn unmap_drive(drive_letter: char) -> anyhow::Result<()> {
    let drive_str = format!("{}:", drive_letter.to_ascii_uppercase());
    info!(drive = %drive_str, "Unmapping Windows network drive...");

    let output = Command::new("net.exe")
        .args(["use", &drive_str, "/delete", "/y"])
        .output()?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        warn!("Failed to cleanly unmap drive {}: {}", drive_str, err.trim());
    } else {
        println!("==> Cleanly unmapped drive {}", drive_str);
    }

    Ok(())
}

pub fn setup_ctrlc_cleanup(drive_letter: Option<char>, running: Arc<AtomicBool>) -> anyhow::Result<()> {
    ctrlc::set_handler(move || {
        println!("\n[!] Received Ctrl+C / Termination signal. Shutting down...");
        if let Some(c) = drive_letter {
            println!("    Cleaning up network drive {}:...", c.to_ascii_uppercase());
            let _ = unmap_drive(c);
        }
        running.store(false, Ordering::SeqCst);
        std::process::exit(0);
    })?;

    Ok(())
}
