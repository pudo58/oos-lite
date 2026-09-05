#[cfg(unix)]
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use std::thread;
use oos_lite_core::StorageEngine;

#[cfg(unix)]
pub mod fuse;

pub mod webdav;

#[cfg(windows)]
pub mod windows;

pub fn mount(
    engine: Arc<StorageEngine>,
    target: Option<&str>,
    force_webdav: bool,
    port: u16,
    cache_mb: usize,
) -> anyhow::Result<()> {
    let is_windows = cfg!(windows);
    let use_webdav = force_webdav || is_windows;

    if use_webdav {
        mount_webdav(engine, target, port, cache_mb)
    } else {
        #[cfg(unix)]
        {
            let path_str = target.unwrap_or("/tmp/oos-drive");
            fuse::mount_fuse(engine, Path::new(path_str), cache_mb)
        }
        #[cfg(not(unix))]
        {
            unreachable!()
        }
    }
}

pub fn mount_webdav(
    engine: Arc<StorageEngine>,
    target: Option<&str>,
    port: u16,
    cache_mb: usize,
) -> anyhow::Result<()> {
    let running = Arc::new(AtomicBool::new(true));
    let host = "127.0.0.1";

    println!("============================================================");
    println!("     OOS-Lite Read-Only WebDAV Virtual Filesystem           ");
    println!("============================================================");

    #[cfg(windows)]
    let mut mapped_drive = None;

    #[cfg(windows)]
    {
        // 1. Run Pre-flight Check
        println!("==> Running Windows Environment Pre-flight Check...");
        let report = windows::run_preflight_check();

        match report.service_status {
            windows::WebClientStatus::Running => {
                println!("  [?] Windows WebClient service: RUNNING");
            }
            windows::WebClientStatus::Stopped => {
                println!("  [!] Windows WebClient service: STOPPED");
                println!("      Notice: Windows Explorer drive mapping requires WebClient.");
                println!("      To enable drive mapping, run in an Administrator PowerShell:");
                println!("      Start-Service WebClient");
                println!("      (You can still access WebDAV via VLC, Cyberduck, or Web browser)");
            }
            windows::WebClientStatus::NotFound => {
                println!("  [!] Windows WebClient service: NOT INSTALLED (e.g. Windows Server)");
                println!("      Install 'WebDAV-Redirector' feature to map network drives.");
            }
            windows::WebClientStatus::Unknown(ref msg) => {
                println!("  [?] WebClient status: {}", msg);
            }
        }

        if let Some(limit_bytes) = report.file_size_limit_bytes {
            let limit_mb = limit_bytes as f64 / (1024.0 * 1024.0);
            println!("  [*] Client FileSizeLimitInBytes: {:.1} MB ({} bytes)", limit_mb, limit_bytes);
            if limit_bytes <= 50_000_000 {
                println!("      Tip: Files larger than {:.0} MB will fail in Windows Explorer.", limit_mb);
                println!("      To raise limit up to 4 GB, run in Admin PowerShell:");
                println!(r"      reg add HKLM\SYSTEM\CurrentControlSet\Services\WebClient\Parameters /v FileSizeLimitInBytes /t REG_DWORD /d 0xffffffff /f");
                println!("      Restart-Service WebClient");
            }
        }
        println!("------------------------------------------------------------");

        let drive_char = target.and_then(|t| {
            let clean = t.trim().trim_end_matches(':');
            if clean.len() == 1 {
                clean.chars().next()
            } else if clean.eq_ignore_ascii_case("webdav") {
                None
            } else {
                None
            }
        }).or(Some('Z'));

        // 2. Start WebDAV Server
        webdav::start_webdav_server(
            Arc::clone(&engine),
            host,
            port,
            cache_mb,
            8,
            Arc::clone(&running),
        )?;

        println!("  Server Address: http://{}:{}", host, port);
        println!("  Hierarchy     : /current, /snapshots, /history");

        // 3. Map drive if drive letter selected
        if let Some(ch) = drive_char {
            if report.service_status == windows::WebClientStatus::Running {
                match windows::map_drive(ch, port) {
                    Ok(()) => {
                        mapped_drive = Some(ch);
                    }
                    Err(e) => {
                        println!("  [!] Could not map drive {}: {}", ch, e);
                        println!("      WebDAV server is still running at http://{}:{}", host, port);
                    }
                }
            } else {
                println!("  [-] Skipping auto-mapping drive {}: (WebClient service is not running)", ch);
                println!("      Start WebClient service and run 'net use {}: \\\\{}@{}\\DavWWWRoot'", ch, host, port);
            }
        }

        // 4. Setup Ctrl+C handler
        windows::setup_ctrlc_cleanup(mapped_drive, Arc::clone(&running))?;
    }

    #[cfg(not(windows))]
    {
        webdav::start_webdav_server(
            Arc::clone(&engine),
            host,
            port,
            cache_mb,
            8,
            Arc::clone(&running),
        )?;
        println!("  Server Address: http://{}:{}", host, port);
        println!("  Hierarchy     : /current, /snapshots, /history");

        let r = Arc::clone(&running);
        ctrlc::set_handler(move || {
            println!("\n[!] Received Ctrl+C. Shutting down WebDAV server...");
            r.store(false, Ordering::SeqCst);
            std::process::exit(0);
        })?;
    }

    println!("============================================================");
    println!("  Press Ctrl+C to unmount and stop server.");
    println!("============================================================");

    while running.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(500));
    }

    Ok(())
}
