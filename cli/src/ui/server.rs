use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};
use tiny_http::{Header, Method, Response, Server, StatusCode};
use tracing::error;
use url::Url;

use oos_lite_core::StorageEngine;

const INDEX_HTML: &str = include_str!("index.html");

#[derive(Default)]
pub struct MountController {
    pub is_mounted: bool,
    pub drive_letter: Option<char>,
    pub port: u16,
    pub stop_flag: Option<Arc<AtomicBool>>,
}

#[derive(Default)]
pub struct WatcherController {
    pub is_running: bool,
    pub watch_dir: Option<PathBuf>,
    pub debounce_secs: u64,
    pub cooldown_secs: u64,
    pub throttle_ms: u64,
    pub handle: Option<oos_lite_core::watcher::WatcherHandle>,
}

#[derive(Serialize)]
struct ApiWatcherStatus {
    running: bool,
    watched_dir: Option<String>,
    debounce_secs: u64,
    cooldown_secs: u64,
    throttle_ms: u64,
    message: Option<String>,
}

#[derive(Deserialize)]
struct ApiWatcherStartRequest {
    dir: String,
    debounce_secs: Option<u64>,
    cooldown_secs: Option<u64>,
    throttle_ms: Option<u64>,
}

#[derive(Deserialize)]
struct ApiPruneRequest {
    keep: Option<usize>,
    name: Option<String>,
}

#[derive(Serialize)]
struct ApiPruneResponse {
    ok: bool,
    pruned_count: usize,
    message: String,
}

#[derive(Serialize)]
struct ApiMountStatus {
    mounted: bool,
    drive: Option<String>,
    drive_letter: String,
    port: u16,
    service_status: String,
    file_size_limit_bytes: Option<u64>,
    message: Option<String>,
}

#[derive(Serialize)]
struct ApiStats {
    total_chunks: usize,
    total_manifests: usize,
    total_objects: usize,
    total_snapshots: usize,
    logical_bytes: u64,
    latest_logical_bytes: u64,
    unique_chunks_bytes: u64,
    physical_disk_bytes: u64,
    dedup_ratio: f64,
    space_savings_pct: f64,
}

#[derive(Serialize)]
struct ApiFileItem {
    name: String,
    object_id: String,
    latest_version: u32,
    size_bytes: u64,
    created_at: u64,
}

#[derive(Serialize)]
struct ApiVersionItem {
    version: u32,
    manifest_id: String,
    size_bytes: u64,
    created_at: u64,
}

#[derive(Serialize)]
struct ApiSnapshotItem {
    label: String,
    created_time: String,
    entries_count: usize,
}

#[derive(Serialize)]
struct ApiUploadResponse {
    name: String,
    version: u32,
    manifest_id: String,
    total_bytes: u64,
    chunk_count: usize,
    new_chunks: usize,
    dedup_chunks: usize,
}

#[derive(Serialize)]
struct ApiGcResponse {
    live_roots: usize,
    reachable_chunks: usize,
    chunks_reclaimed: usize,
    manifests_reclaimed: usize,
    active_chunks_retained: usize,
}

#[derive(Serialize)]
struct ApiFsckResponse {
    is_healthy: bool,
    segments_checked: usize,
    chunks_checked: usize,
    manifests_checked: usize,
    objects_checked: usize,
    corrupted_chunks: usize,
    missing_chunks: usize,
    errors: Vec<String>,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Serialize)]
struct SuccessResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    count: Option<usize>,
}

#[derive(Deserialize)]
struct SnapshotCreateReq {
    label: String,
}

#[derive(Deserialize)]
struct SnapshotRestoreReq {
    label: String,
    dir: String,
}

#[derive(Deserialize)]
struct SnapshotDeleteReq {
    label: String,
}

#[derive(Deserialize)]
struct FileDeleteReq {
    name: String,
}

#[derive(Deserialize)]
struct FileRollbackReq {
    name: String,
    version: u32,
    disk_path: Option<String>,
}

fn json_response<T: Serialize>(data: &T) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::to_vec(data).unwrap_or_else(|_| b"{}".to_vec());
    let ct = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
    Response::from_data(body).with_header(ct)
}

fn error_response(status: u16, msg: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::to_vec(&ErrorResponse { error: msg.to_string() }).unwrap();
    let ct = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
    Response::from_data(body)
        .with_status_code(StatusCode(status))
        .with_header(ct)
}

fn format_relative_time(ts_secs: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let diff = now.saturating_sub(ts_secs);
    if diff < 60 {
        format!("{}s ago", diff)
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}

#[cfg(windows)]
pub fn open_desktop_window(url: &str) {
    let candidate_browsers = [
        // Microsoft Edge (Pre-installed on Windows 10 & 11)
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        // Google Chrome
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        // Brave
        r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe",
    ];

    let app_arg = format!("--app={}", url);
    let size_arg = "--window-size=1200,820";

    // 1. Try explicit browser installation paths
    for path in &candidate_browsers {
        if std::path::Path::new(path).exists() {
            if let Ok(_) = std::process::Command::new(path)
                .args([&app_arg, size_arg, "--no-first-run", "--no-default-browser-check"])
                .spawn()
            {
                return;
            }
        }
    }

    // 2. Try looking up in PATH
    for cmd in &["msedge.exe", "msedge", "chrome.exe", "chrome", "brave.exe"] {
        if let Ok(_) = std::process::Command::new(cmd)
            .args([&app_arg, size_arg, "--no-first-run", "--no-default-browser-check"])
            .spawn()
        {
            return;
        }
    }

    // 3. Fallback: ShellExecuteW if no Chromium app mode is available
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            hwnd: *mut std::ffi::c_void,
            lpOperation: *const u16,
            lpFile: *const u16,
            lpParameters: *const u16,
            lpDirectory: *const u16,
            nShowCmd: i32,
        ) -> *mut std::ffi::c_void;
    }

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    let op = to_wide("open");
    let target = to_wide(url);
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1, // SW_SHOWNORMAL
        );
    }
}

#[cfg(not(windows))]
pub fn open_desktop_window(url: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

pub fn start_ui_server(
    engine: Arc<StorageEngine>,
    host: &str,
    port: u16,
    no_open: bool,
    is_desktop: bool,
) -> anyhow::Result<()> {
    let addr = format!("{}:{}", host, port);
    let server = Server::http(&addr)
        .map_err(|e| anyhow::anyhow!("Failed to bind UI server on {}: {}", addr, e))?;

    let local_url = if host == "0.0.0.0" {
        println!("⚠️  SECURITY WARNING: Bound to 0.0.0.0 - Web UI is exposed to LAN without auth!");
        format!("http://localhost:{}", port)
    } else {
        format!("http://{}:{}", host, port)
    };

    println!("============================================================");
    if is_desktop {
        println!("       OOS-Lite Desktop Application running at:");
    } else {
        println!("       OOS-Lite Web UI Dashboard running at:");
    }
    println!("       {}", local_url);
    println!("       Listening on: {}", addr);
    println!("       Press Ctrl+C to exit.");
    println!("============================================================");

    let mount_ctrl = Arc::new(Mutex::new(MountController::default()));
    let watcher_ctrl = Arc::new(Mutex::new(WatcherController::default()));

    // Clean up legacy .bat shortcuts on Windows Desktop if present
    #[cfg(windows)]
    if is_desktop {
        let mut candidates = Vec::new();
        if let Ok(od) = std::env::var("OneDrive") {
            candidates.push(std::path::PathBuf::from(od).join("Desktop"));
        }
        if let Ok(profile) = std::env::var("USERPROFILE") {
            let p = std::path::PathBuf::from(&profile);
            candidates.push(p.join("OneDrive").join("Desktop"));
            candidates.push(p.join("Desktop"));
        }

        for desktop_dir in candidates {
            let legacy_bat = desktop_dir.join("OOS-Lite.bat");
            if legacy_bat.exists() {
                let _ = std::fs::remove_file(&legacy_bat);
            }
        }
    }

    // Auto-mount in desktop mode for instant gratification
    if is_desktop {
        let mut ctrl = mount_ctrl.lock().unwrap_or_else(|p| p.into_inner());
        let mount_port = 8080u16;
        let stop_flag = Arc::new(AtomicBool::new(true));
        let r = Arc::clone(&stop_flag);

        if crate::mount::webdav::start_webdav_server(
            Arc::clone(&engine),
            "127.0.0.1",
            mount_port,
            128,
            8,
            r,
        ).is_ok() {
            #[cfg(windows)]
            let mut drive = None;
            #[cfg(windows)]
            {
                let preflight = crate::mount::windows::run_preflight_check();
                if preflight.service_status == crate::mount::windows::WebClientStatus::Running {
                    if crate::mount::windows::map_drive('Z', mount_port).is_ok() {
                        drive = Some('Z');
                    }
                }
            }

            ctrl.is_mounted = true;
            ctrl.port = mount_port;
            ctrl.stop_flag = Some(stop_flag);
            #[cfg(windows)]
            {
                ctrl.drive_letter = drive;
            }
        }
    }

    // Setup cleanup handler on Ctrl+C
    let cleanup_ctrl = Arc::clone(&mount_ctrl);
    let _ = ctrlc::set_handler(move || {
        println!("\n[!] Exiting OOS-Lite...");
        let ctrl = cleanup_ctrl.lock().unwrap();
        #[cfg(windows)]
        if let Some(ch) = ctrl.drive_letter {
            let _ = crate::mount::windows::unmap_drive(ch);
        }
        std::process::exit(0);
    });

    if !no_open {
        let open_url = local_url.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            open_desktop_window(&open_url);
        });
    }

    let server = Arc::new(server);

    for request in server.incoming_requests() {
        let engine_clone = Arc::clone(&engine);
        let mount_ctrl_clone = Arc::clone(&mount_ctrl);
        let watcher_ctrl_clone = Arc::clone(&watcher_ctrl);
        std::thread::spawn(move || {
            if let Err(e) = handle_request(engine_clone, mount_ctrl_clone, watcher_ctrl_clone, request) {
                error!("Request error: {:?}", e);
            }
        });
    }

    Ok(())
}

pub fn is_host_allowed(host_val: &str) -> bool {
    let host_domain = host_val.split(':').next().unwrap_or("").trim();
    matches!(host_domain, "127.0.0.1" | "localhost" | "")
}

pub fn is_origin_allowed(origin: &str) -> bool {
    match Url::parse(origin) {
        Ok(url) => {
            (url.scheme() == "http" || url.scheme() == "https")
                && matches!(url.host_str(), Some("127.0.0.1") | Some("localhost"))
        }
        Err(_) => false,
    }
}

fn handle_request(
    engine: Arc<StorageEngine>,
    mount_ctrl: Arc<Mutex<MountController>>,
    watcher_ctrl: Arc<Mutex<WatcherController>>,
    mut request: tiny_http::Request,
) -> anyhow::Result<()> {
    let parsed_url = Url::parse(&format!("http://localhost{}", request.url()))?;
    let path = parsed_url.path().to_string();
    let method = request.method().clone();

    // 1. DNS Rebinding & Host header validation
    if let Some(host_header) = request.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case("Host")) {
        if !is_host_allowed(host_header.value.as_str()) {
            let _ = request.respond(error_response(403, "Invalid Host header (DNS Rebinding protection)"));
            return Ok(());
        }
    }

    // 2. Fetch Metadata (Sec-Fetch-Site): reject cross-site requests
    if let Some(sec_site) = request.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case("Sec-Fetch-Site")) {
        let val = sec_site.value.as_str();
        if val.eq_ignore_ascii_case("cross-site") {
            let _ = request.respond(error_response(403, "Cross-Site Requests Forbidden"));
            return Ok(());
        }
    }

    // 3. CSRF Protection: Strict Origin validation
    if let Some(origin_header) = request.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case("Origin")) {
        if !is_origin_allowed(origin_header.value.as_str()) {
            let _ = request.respond(error_response(403, "Cross-Origin Requests (CORS/CSRF) Forbidden"));
            return Ok(());
        }
    }

    // CORS preflight
    if method == Method::Options {
        let resp = Response::empty(StatusCode(204));
        let _ = request.respond(resp);
        return Ok(());
    }

    match (method, path.as_str()) {
        (Method::Get, "/") | (Method::Get, "/index.html") => {
            let ct = Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap();
            let resp = Response::from_string(INDEX_HTML).with_header(ct);
            let _ = request.respond(resp);
        }

        (Method::Get, "/api/stats") => {
            let s = engine.stats();
            let resp_data = ApiStats {
                total_chunks: s.total_chunks,
                total_manifests: s.total_manifests,
                total_objects: s.total_objects,
                total_snapshots: s.total_snapshots,
                logical_bytes: s.logical_bytes,
                latest_logical_bytes: s.latest_logical_bytes,
                unique_chunks_bytes: s.unique_chunks_bytes,
                physical_disk_bytes: s.physical_disk_bytes,
                dedup_ratio: s.dedup_ratio,
                space_savings_pct: s.space_savings_pct,
            };
            let _ = request.respond(json_response(&resp_data));
        }

        (Method::Get, "/api/files") => {
            match engine.list_files() {
                Ok(files) => {
                    let items: Vec<ApiFileItem> = files
                        .into_iter()
                        .map(|(name, id, record)| {
                            let (latest_ver, size, created) = if let Some(latest) = record.latest() {
                                (latest.version, latest.size_bytes, latest.created_at)
                            } else {
                                (0, 0, 0)
                            };
                            ApiFileItem {
                                name,
                                object_id: id.to_string(),
                                latest_version: latest_ver,
                                size_bytes: size,
                                created_at: created,
                            }
                        })
                        .collect();
                    let _ = request.respond(json_response(&items));
                }
                Err(e) => {
                    let _ = request.respond(error_response(500, &e.to_string()));
                }
            }
        }

        (Method::Get, "/api/versions") => {
            let name_query = parsed_url.query_pairs().find(|(k, _)| k == "name");
            if let Some((_, name)) = name_query {
                match engine.get_versions(&name) {
                    Ok(versions) => {
                        let items: Vec<ApiVersionItem> = versions
                            .into_iter()
                            .map(|v| ApiVersionItem {
                                version: v.version,
                                manifest_id: v.manifest_id,
                                size_bytes: v.size_bytes,
                                created_at: v.created_at,
                            })
                            .collect();
                        let _ = request.respond(json_response(&items));
                    }
                    Err(e) => {
                        let _ = request.respond(error_response(404, &e.to_string()));
                    }
                }
            } else {
                let _ = request.respond(error_response(400, "Missing 'name' parameter"));
            }
        }

        (Method::Get, "/api/snapshots") => {
            match engine.list_snapshots() {
                Ok(snapshots) => {
                    let items: Vec<ApiSnapshotItem> = snapshots
                        .into_iter()
                        .map(|s| ApiSnapshotItem {
                            label: s.label,
                            created_time: format_relative_time(s.created_at),
                            entries_count: s.entries.len(),
                        })
                        .collect();
                    let _ = request.respond(json_response(&items));
                }
                Err(e) => {
                    let _ = request.respond(error_response(500, &e.to_string()));
                }
            }
        }

        (Method::Post, "/api/upload") => {
            let name_query = parsed_url.query_pairs().find(|(k, _)| k == "name");
            let raw_name = name_query
                .map(|(_, v)| v.replace('\\', "/"))
                .unwrap_or_else(|| "unnamed_file".to_string());

            let safe_name = Path::new(&raw_name)
                .components()
                .filter_map(|c| match c {
                    std::path::Component::Normal(p) => p.to_str(),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("/");
            let file_name = if safe_name.is_empty() { "unnamed_file".to_string() } else { safe_name };

            let tmp_dir = std::env::temp_dir();
            let now_ns = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let tmp_path = tmp_dir.join(format!("oos_up_{}_{}.tmp", std::process::id(), now_ns));

            {
                let mut tmp_file = match std::fs::File::create(&tmp_path) {
                    Ok(f) => f,
                    Err(e) => {
                        let _ = request.respond(error_response(500, &format!("Failed to create temp file: {}", e)));
                        return Ok(());
                    }
                };

                let reader = request.as_reader();
                if let Err(e) = std::io::copy(reader, &mut tmp_file) {
                    let _ = std::fs::remove_file(&tmp_path);
                    let _ = request.respond(error_response(500, &format!("Failed to stream body: {}", e)));
                    return Ok(());
                }
            }

            match engine.put_file_named(&file_name, &tmp_path) {
                Ok(summary) => {
                    let _ = std::fs::remove_file(&tmp_path);
                    let resp = ApiUploadResponse {
                        name: file_name,
                        version: summary.version,
                        manifest_id: summary.manifest_id,
                        total_bytes: summary.total_bytes,
                        chunk_count: summary.chunk_count,
                        new_chunks: summary.new_chunks,
                        dedup_chunks: summary.dedup_chunks,
                    };
                    let _ = request.respond(json_response(&resp));
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&tmp_path);
                    let _ = request.respond(error_response(500, &e.to_string()));
                }
            }
        }

        (Method::Post, "/api/file/store-path") => {
            #[derive(serde::Deserialize)]
            struct StorePathReq {
                path: String,
                name: Option<String>,
            }
            let mut body_str = String::new();
            let _ = request.as_reader().read_to_string(&mut body_str);
            let req_data: StorePathReq = match serde_json::from_str(&body_str) {
                Ok(d) => d,
                Err(e) => {
                    let _ = request.respond(error_response(400, &format!("Invalid JSON: {}", e)));
                    return Ok(());
                }
            };
            let target_path = PathBuf::from(&req_data.path);
            if !target_path.exists() {
                let _ = request.respond(error_response(404, "Target file does not exist on disk"));
                return Ok(());
            }
            if target_path.is_dir() {
                let _ = request.respond(error_response(400, "Selected path is a folder. Please choose individual files, or use Auto-Vault to sync folders."));
                return Ok(());
            }
            let fallback_name = target_path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("unnamed_file")
                .to_string();
            let file_name = req_data.name.as_deref().unwrap_or(&fallback_name);
            match engine.put_file_named(file_name, &target_path) {
                Ok(summary) => {
                    let resp = serde_json::json!({
                        "ok": true,
                        "name": file_name,
                        "version": summary.version,
                        "size": summary.total_bytes,
                        "chunks": summary.chunk_count,
                        "dedup_chunks": summary.dedup_chunks,
                        "message": format!("Successfully stored '{}' into Vault as version #{}", file_name, summary.version),
                    });
                    let _ = request.respond(json_response(&resp));
                }
                Err(e) => {
                    let _ = request.respond(error_response(500, &format!("Failed to store file: {}", e)));
                }
            }
        }

        (Method::Post, "/api/folder/ingest") => {
            #[derive(serde::Deserialize)]
            struct FolderIngestReq {
                path: String,
                create_snapshot: Option<bool>,
                snapshot_label: Option<String>,
            }
            let mut body_str = String::new();
            let _ = request.as_reader().read_to_string(&mut body_str);
            let req_data: FolderIngestReq = match serde_json::from_str(&body_str) {
                Ok(d) => d,
                Err(e) => {
                    let _ = request.respond(error_response(400, &format!("Invalid JSON: {}", e)));
                    return Ok(());
                }
            };
            let folder_path = PathBuf::from(&req_data.path);
            if !folder_path.is_dir() {
                let _ = request.respond(error_response(400, "Selected path is not a valid directory"));
                return Ok(());
            }

            let base_name = folder_path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("folder")
                .to_string();

            fn walk_folder(dir: &Path, root: &Path, prefix: &str, out: &mut Vec<(PathBuf, String)>) {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        let name = entry.file_name();
                        let name_str = name.to_string_lossy();
                        // Skip system and heavy build artifacts
                        if name_str.starts_with('.') || name_str == "node_modules" || name_str == "target" || name_str == "dist" {
                            continue;
                        }
                        if p.is_dir() {
                            walk_folder(&p, root, prefix, out);
                        } else if p.is_file() {
                            if let Ok(rel) = p.strip_prefix(root) {
                                let logical = format!("{}/{}", prefix, rel.to_string_lossy().replace('\\', "/"));
                                out.push((p, logical));
                            }
                        }
                    }
                }
            }

            let mut files_to_ingest = Vec::new();
            walk_folder(&folder_path, &folder_path, &base_name, &mut files_to_ingest);

            let mut stored_count = 0usize;
            let mut total_bytes = 0u64;
            for (p, logical) in &files_to_ingest {
                if let Ok(summary) = engine.put_file_named(logical, p) {
                    stored_count += 1;
                    total_bytes += summary.total_bytes;
                }
            }

            let now_secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let snap_label = req_data.snapshot_label.unwrap_or_else(|| {
                format!("snapshot_{}_{}", base_name, now_secs)
            });

            let mut snapshot_ok = false;
            if req_data.create_snapshot.unwrap_or(true) {
                if engine.create_snapshot(&snap_label).is_ok() {
                    snapshot_ok = true;
                }
            }

            let resp = serde_json::json!({
                "ok": true,
                "folder": base_name,
                "files_stored": stored_count,
                "total_bytes": total_bytes,
                "snapshot_created": snapshot_ok,
                "snapshot_label": snap_label,
            });
            let _ = request.respond(json_response(&resp));
        }

        (Method::Get, "/api/download") => {
            let target = parsed_url.query_pairs().find(|(k, _)| k == "target").map(|(_, v)| v.replace('\\', "/"));
            let version = parsed_url
                .query_pairs()
                .find(|(k, _)| k == "version")
                .and_then(|(_, v)| v.parse::<u32>().ok());

            if let Some(target_str) = target {
                let tmp_dir = std::env::temp_dir();
                let now_ns = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos();
                let tmp_path = tmp_dir.join(format!("oos_down_{}_{}.tmp", std::process::id(), now_ns));

                match engine.get_file_version(&target_str, version, &tmp_path) {
                    Ok(_) => {
                        match std::fs::File::open(&tmp_path) {
                            Ok(file) => {
                                let download_name = Path::new(&target_str)
                                    .file_name()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("download.bin");
                                let disp_val = format!("attachment; filename=\"{}\"", download_name);
                                let disp = Header::from_bytes(&b"Content-Disposition"[..], disp_val.as_bytes()).unwrap();
                                let ct = Header::from_bytes(&b"Content-Type"[..], &b"application/octet-stream"[..]).unwrap();
                                let resp = Response::from_file(file).with_header(disp).with_header(ct);
                                let _ = request.respond(resp);
                                let _ = std::fs::remove_file(&tmp_path);
                            }
                            Err(e) => {
                                let _ = std::fs::remove_file(&tmp_path);
                                let _ = request.respond(error_response(500, &format!("Open error: {}", e)));
                            }
                        }
                    }
                    Err(e) => {
                        let _ = std::fs::remove_file(&tmp_path);
                        let _ = request.respond(error_response(404, &e.to_string()));
                    }
                }
            } else {
                let _ = request.respond(error_response(400, "Missing 'target' query parameter"));
            }
        }

        (Method::Post, "/api/snapshot/create") => {
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);
            match serde_json::from_str::<SnapshotCreateReq>(&body) {
                Ok(req) => match engine.create_snapshot(&req.label) {
                    Ok(_) => {
                        let resp = SuccessResponse {
                            ok: true,
                            message: Some(format!("Snapshot '{}' created", req.label)),
                            count: None,
                        };
                        let _ = request.respond(json_response(&resp));
                    }
                    Err(e) => {
                        let _ = request.respond(error_response(400, &e.to_string()));
                    }
                },
                Err(e) => {
                    let _ = request.respond(error_response(400, &format!("Invalid JSON: {}", e)));
                }
            }
        }

        (Method::Post, "/api/snapshot/restore") => {
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);
            match serde_json::from_str::<SnapshotRestoreReq>(&body) {
                Ok(req) => {
                    let dir_str = req.dir.trim();
                    let dir_path = Path::new(dir_str);
                    let has_parent_traversal = dir_path.components().any(|c| matches!(c, std::path::Component::ParentDir));
                    let is_absolute_or_root = dir_path.is_absolute()
                        || dir_path.components().any(|c| matches!(c, std::path::Component::RootDir | std::path::Component::Prefix(_)));
                    let normal_count = dir_path.components().filter(|c| matches!(c, std::path::Component::Normal(_))).count();

                    if dir_str.is_empty() || has_parent_traversal || is_absolute_or_root || normal_count == 0 {
                        let _ = request.respond(error_response(400, "Invalid restore directory: must be a relative subdirectory without parent traversal"));
                        return Ok(());
                    }
                    match engine.restore_snapshot(&req.label, Path::new(dir_str)) {
                        Ok(count) => {
                            let resp = SuccessResponse {
                                ok: true,
                                message: Some(format!("Restored {} files", count)),
                                count: Some(count),
                            };
                            let _ = request.respond(json_response(&resp));
                        }
                        Err(e) => {
                            let _ = request.respond(error_response(400, &e.to_string()));
                        }
                    }
                }
                Err(e) => {
                    let _ = request.respond(error_response(400, &format!("Invalid JSON: {}", e)));
                }
            }
        }

        (Method::Post, "/api/snapshot/delete") => {
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);
            match serde_json::from_str::<SnapshotDeleteReq>(&body) {
                Ok(req) => match engine.delete_snapshot(&req.label) {
                    Ok(deleted) => {
                        let resp = SuccessResponse {
                            ok: deleted,
                            message: if deleted { Some("Deleted".into()) } else { Some("Not found".into()) },
                            count: None,
                        };
                        let _ = request.respond(json_response(&resp));
                    }
                    Err(e) => {
                        let _ = request.respond(error_response(400, &e.to_string()));
                    }
                },
                Err(e) => {
                    let _ = request.respond(error_response(400, &format!("Invalid JSON: {}", e)));
                }
            }
        }

        (Method::Post, "/api/file/delete") => {
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);
            match serde_json::from_str::<FileDeleteReq>(&body) {
                Ok(req) => match engine.delete_file(&req.name) {
                    Ok(deleted) => {
                        let resp = SuccessResponse {
                            ok: deleted,
                            message: if deleted { Some("File unlinked".into()) } else { Some("File not found".into()) },
                            count: None,
                        };
                        let _ = request.respond(json_response(&resp));
                    }
                    Err(e) => {
                        let _ = request.respond(error_response(400, &e.to_string()));
                    }
                },
                Err(e) => {
                    let _ = request.respond(error_response(400, &format!("Invalid JSON: {}", e)));
                }
            }
        }

        (Method::Post, "/api/file/rollback") => {
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);
            match serde_json::from_str::<FileRollbackReq>(&body) {
                Ok(req) => {
                    let disk_target: Option<PathBuf> = if let Some(ref dp) = req.disk_path {
                        let p = PathBuf::from(dp);
                        if !p.as_os_str().is_empty() {
                            Some(p)
                        } else {
                            None
                        }
                    } else {
                        // Try resolving via watcher directory
                        let w_dir = watcher_ctrl.lock().ok().and_then(|w| w.watch_dir.clone());
                        if let Some(ref wd) = w_dir {
                            let candidate = wd.join(&req.name);
                            if candidate.exists() || candidate.parent().map(|p| p.exists()).unwrap_or(false) {
                                Some(candidate)
                            } else {
                                None
                            }
                        } else {
                            let local = PathBuf::from(&req.name);
                            if local.exists() {
                                Some(local)
                            } else {
                                None
                            }
                        }
                    };

                    match engine.rollback_file(&req.name, req.version, disk_target.as_ref()) {
                        Ok((new_version, written_bytes)) => {
                            let applied_path_str = disk_target.map(|p| p.to_string_lossy().to_string());
                            let resp = serde_json::json!({
                                "ok": true,
                                "name": req.name,
                                "target_version": req.version,
                                "new_version": new_version,
                                "written_bytes": written_bytes,
                                "applied_disk_path": applied_path_str,
                                "message": format!("Successfully rolled back '{}' to version #{} (recorded as version #{})", req.name, req.version, new_version),
                            });
                            let _ = request.respond(json_response(&resp));
                        }
                        Err(e) => {
                            let _ = request.respond(error_response(500, &format!("Rollback failed: {}", e)));
                        }
                    }
                }
                Err(e) => {
                    let _ = request.respond(error_response(400, &format!("Invalid JSON: {}", e)));
                }
            }
        }

        (Method::Post, "/api/gc") => {
            match engine.gc() {
                Ok(stats) => {
                    let resp = ApiGcResponse {
                        live_roots: stats.live_roots,
                        reachable_chunks: stats.reachable_chunks,
                        chunks_reclaimed: stats.chunks_reclaimed,
                        manifests_reclaimed: stats.manifests_reclaimed,
                        active_chunks_retained: stats.active_chunks_retained,
                    };
                    let _ = request.respond(json_response(&resp));
                }
                Err(e) => {
                    let _ = request.respond(error_response(500, &e.to_string()));
                }
            }
        }

        (Method::Post, "/api/fsck") => {
            match engine.fsck() {
                Ok(rep) => {
                    let resp = ApiFsckResponse {
                        is_healthy: rep.is_healthy,
                        segments_checked: rep.segments_checked,
                        chunks_checked: rep.chunks_checked,
                        manifests_checked: rep.manifests_checked,
                        objects_checked: rep.objects_checked,
                        corrupted_chunks: rep.corrupted_chunks,
                        missing_chunks: rep.missing_chunks,
                        errors: rep.errors,
                    };
                    let _ = request.respond(json_response(&resp));
                }
                Err(e) => {
                    let _ = request.respond(error_response(500, &e.to_string()));
                }
            }
        }

        (Method::Get, "/api/mount/status") => {
            let mut ctrl = mount_ctrl.lock().unwrap_or_else(|p| p.into_inner());
            #[cfg(windows)]
            {
                let mapped = crate::mount::windows::is_drive_mapped('Z');
                ctrl.is_mounted = mapped;
                ctrl.drive_letter = if mapped { Some('Z') } else { None };
            }

            #[cfg(windows)]
            let report = crate::mount::windows::run_preflight_check();

            #[cfg(windows)]
            let status_str = match report.service_status {
                crate::mount::windows::WebClientStatus::Running => "RUNNING".to_string(),
                crate::mount::windows::WebClientStatus::Stopped => "STOPPED".to_string(),
                crate::mount::windows::WebClientStatus::NotFound => "NOT_FOUND".to_string(),
                crate::mount::windows::WebClientStatus::Unknown(ref s) => s.clone(),
            };
            #[cfg(not(windows))]
            let status_str = "POSIX_FUSE".to_string();

            #[cfg(windows)]
            let limit_bytes = report.file_size_limit_bytes;
            #[cfg(not(windows))]
            let limit_bytes = None;

            let resp = ApiMountStatus {
                mounted: ctrl.is_mounted,
                drive: ctrl.drive_letter.map(|c| format!("{}:", c)),
                drive_letter: "Z".to_string(),
                port: if ctrl.port == 0 { 8080 } else { ctrl.port },
                service_status: status_str,
                file_size_limit_bytes: limit_bytes,
                message: None,
            };
            let _ = request.respond(json_response(&resp));
        }

        (Method::Post, "/api/mount/toggle") => {
            let mut ctrl = mount_ctrl.lock().unwrap_or_else(|p| p.into_inner());
            let port = if ctrl.port == 0 { 8080 } else { ctrl.port };

            // Ensure WebDAV server is active in background
            if ctrl.stop_flag.is_none() {
                let stop_flag = Arc::new(AtomicBool::new(true));
                let r = Arc::clone(&stop_flag);
                if let Err(e) = crate::mount::webdav::start_webdav_server(
                    Arc::clone(&engine),
                    "127.0.0.1",
                    port,
                    128,
                    8,
                    r,
                ) {
                    let _ = request.respond(error_response(500, &format!("Không thể khởi động WebDAV server tại cổng {}: {}", port, e)));
                    return Ok(());
                }
                ctrl.stop_flag = Some(stop_flag);
                ctrl.port = port;
            }

            #[cfg(windows)]
            {
                let is_mapped = crate::mount::windows::is_drive_mapped('Z');
                if is_mapped {
                    let _ = crate::mount::windows::unmap_drive('Z');
                    ctrl.is_mounted = false;
                    ctrl.drive_letter = None;
                } else {
                    let preflight = crate::mount::windows::run_preflight_check();
                    if preflight.service_status != crate::mount::windows::WebClientStatus::Running {
                        let _ = request.respond(error_response(400, "Dịch vụ WebClient của Windows đang tắt. Hãy mở CMD (Admin) gõ: net start WebClient"));
                        return Ok(());
                    }
                    match crate::mount::windows::map_drive('Z', port) {
                        Ok(()) => {
                            ctrl.is_mounted = true;
                            ctrl.drive_letter = Some('Z');
                        }
                        Err(e) => {
                            let _ = request.respond(error_response(500, &format!("Không thể gắn ổ Z: {}", e)));
                            return Ok(());
                        }
                    }
                }
            }

            #[cfg(windows)]
            let report = crate::mount::windows::run_preflight_check();
            #[cfg(windows)]
            let status_str = match report.service_status {
                crate::mount::windows::WebClientStatus::Running => "RUNNING".to_string(),
                crate::mount::windows::WebClientStatus::Stopped => "STOPPED".to_string(),
                crate::mount::windows::WebClientStatus::NotFound => "NOT_FOUND".to_string(),
                crate::mount::windows::WebClientStatus::Unknown(ref s) => s.clone(),
            };
            #[cfg(not(windows))]
            let status_str = "POSIX_FUSE".to_string();

            let resp = ApiMountStatus {
                mounted: ctrl.is_mounted,
                drive: ctrl.drive_letter.map(|c| format!("{}:", c)),
                drive_letter: "Z".to_string(),
                port: ctrl.port,
                service_status: status_str,
                file_size_limit_bytes: None,
                message: if ctrl.is_mounted {
                    Some("Đã kết nối ổ Z:\\ thành công".to_string())
                } else {
                    Some("Đã ngắt kết nối ổ Z:\\ an toàn".to_string())
                },
            };
            let _ = request.respond(json_response(&resp));
        }

        (Method::Post, "/api/mount/open") => {
            let _ctrl = mount_ctrl.lock().unwrap_or_else(|p| p.into_inner());
            #[cfg(windows)]
            {
                let is_mapped = crate::mount::windows::is_drive_mapped('Z');
                if is_mapped {
                    let _ = std::process::Command::new("explorer.exe")
                        .arg(r"Z:\")
                        .spawn();
                    let resp = SuccessResponse {
                        ok: true,
                        message: Some("Đã mở ổ Z:\\ trong Windows Explorer".to_string()),
                        count: None,
                    };
                    let _ = request.respond(json_response(&resp));
                } else {
                    let _ = request.respond(error_response(400, "Ổ Z:\\ chưa được kết nối. Hãy bấm nút 'Kết Nối Ổ Đĩa' trước."));
                }
            }
            #[cfg(not(windows))]
            {
                let _ = &_ctrl;
                let resp = SuccessResponse {
                    ok: true,
                    message: Some("Explorer not supported on POSIX".to_string()),
                    count: None,
                };
                let _ = request.respond(json_response(&resp));
            }
        }

        (Method::Get, "/api/watcher/status") => {
            let mut ctrl = watcher_ctrl.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(ref h) = ctrl.handle {
                ctrl.is_running = h.is_running();
            } else {
                ctrl.is_running = false;
            }
            let resp = ApiWatcherStatus {
                running: ctrl.is_running,
                watched_dir: ctrl.watch_dir.as_ref().map(|p| p.display().to_string()),
                debounce_secs: if ctrl.debounce_secs == 0 { 3 } else { ctrl.debounce_secs },
                cooldown_secs: if ctrl.cooldown_secs == 0 { 60 } else { ctrl.cooldown_secs },
                throttle_ms: if ctrl.throttle_ms == 0 { 10 } else { ctrl.throttle_ms },
                message: None,
            };
            let _ = request.respond(json_response(&resp));
        }

        (Method::Post, "/api/watcher/start") => {
            let mut body = Vec::new();
            let _ = request.as_reader().read_to_end(&mut body);
            let req_data: ApiWatcherStartRequest = match serde_json::from_slice(&body) {
                Ok(d) => d,
                Err(_) => {
                    let _ = request.respond(error_response(400, "Invalid JSON body for watcher start"));
                    return Ok(());
                }
            };

            let target_dir = PathBuf::from(&req_data.dir);
            if !target_dir.is_dir() {
                let _ = request.respond(error_response(400, "Thư mục không tồn tại / Directory does not exist"));
                return Ok(());
            }

            let debounce = req_data.debounce_secs.unwrap_or(3);
            let cooldown = req_data.cooldown_secs.unwrap_or(60);
            let throttle = req_data.throttle_ms.unwrap_or(10);

            let mut ctrl = watcher_ctrl.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(h) = ctrl.handle.take() {
                h.stop();
            }

            let config = oos_lite_core::watcher::WatcherConfig::new(&target_dir)
                .with_debounce(std::time::Duration::from_secs(debounce))
                .with_cooldown(std::time::Duration::from_secs(cooldown))
                .with_throttle_ms(throttle);

            let service = oos_lite_core::watcher::WatcherService::new(Arc::clone(&engine), config);
            match service.start() {
                Ok(handle) => {
                    ctrl.is_running = true;
                    ctrl.watch_dir = Some(target_dir.clone());
                    ctrl.debounce_secs = debounce;
                    ctrl.cooldown_secs = cooldown;
                    ctrl.throttle_ms = throttle;
                    ctrl.handle = Some(handle);

                    let resp = ApiWatcherStatus {
                        running: true,
                        watched_dir: Some(target_dir.display().to_string()),
                        debounce_secs: debounce,
                        cooldown_secs: cooldown,
                        throttle_ms: throttle,
                        message: Some("Đã kích hoạt Auto-Vault Watcher thành công".to_string()),
                    };
                    let _ = request.respond(json_response(&resp));
                }
                Err(e) => {
                    let _ = request.respond(error_response(500, &format!("Không thể khởi động Watcher: {}", e)));
                }
            }
        }

        (Method::Post, "/api/watcher/stop") => {
            let mut ctrl = watcher_ctrl.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(h) = ctrl.handle.take() {
                h.stop();
            }
            ctrl.is_running = false;
            let resp = ApiWatcherStatus {
                running: false,
                watched_dir: ctrl.watch_dir.as_ref().map(|p| p.display().to_string()),
                debounce_secs: ctrl.debounce_secs,
                cooldown_secs: ctrl.cooldown_secs,
                throttle_ms: ctrl.throttle_ms,
                message: Some("Đã tạm dừng Auto-Vault Watcher".to_string()),
            };
            let _ = request.respond(json_response(&resp));
        }

        (Method::Post, "/api/prune") => {
            let mut body = Vec::new();
            let _ = request.as_reader().read_to_end(&mut body);
            let req_data: ApiPruneRequest = serde_json::from_slice(&body).unwrap_or(ApiPruneRequest {
                keep: Some(10),
                name: None,
            });

            let keep = req_data.keep.unwrap_or(10).max(1);
            let pruned_res = if let Some(ref target_name) = req_data.name {
                engine.prune_file_versions(target_name, keep)
            } else {
                engine.prune_all(keep)
            };

            match pruned_res {
                Ok(count) => {
                    let resp = ApiPruneResponse {
                        ok: true,
                        pruned_count: count,
                        message: format!("Đã dọn dẹp {} phiên bản cũ (giữ lại {} bản gần nhất)", count, keep),
                    };
                    let _ = request.respond(json_response(&resp));
                }
                Err(e) => {
                    let _ = request.respond(error_response(500, &format!("Lỗi khi dọn dẹp phiên bản: {}", e)));
                }
            }
        }

        // ── Shell Extension (Context Menu) ─────────────────────────────────
        (Method::Get, "/api/shell-ext/status") => {
            #[cfg(windows)]
            {
                let enabled = crate::shell_ext::windows::is_registered();
                let body = serde_json::json!({ "enabled": enabled });
                let _ = request.respond(json_response(&body));
            }
            #[cfg(not(windows))]
            {
                let body = serde_json::json!({ "enabled": false, "unsupported": true });
                let _ = request.respond(json_response(&body));
            }
        }
        (Method::Post, "/api/shell-ext/enable") => {
            #[cfg(windows)]
            {
                match crate::shell_ext::windows::register() {
                    Ok(()) => {
                        let body = serde_json::json!({ "ok": true, "enabled": true });
                        let _ = request.respond(json_response(&body));
                    }
                    Err(e) => {
                        let _ = request.respond(error_response(500, &format!("Failed to register context menu: {}", e)));
                    }
                }
            }
            #[cfg(not(windows))]
            {
                let _ = request.respond(error_response(400, "Context menu only supported on Windows"));
            }
        }
        (Method::Post, "/api/shell-ext/disable") => {
            #[cfg(windows)]
            {
                match crate::shell_ext::windows::unregister() {
                    Ok(()) => {
                        let body = serde_json::json!({ "ok": true, "enabled": false });
                        let _ = request.respond(json_response(&body));
                    }
                    Err(e) => {
                        let _ = request.respond(error_response(500, &format!("Failed to remove context menu: {}", e)));
                    }
                }
            }
            #[cfg(not(windows))]
            {
                let _ = request.respond(error_response(400, "Context menu only supported on Windows"));
            }
        }

        _ => {
            let _ = request.respond(error_response(404, "Endpoint not found"));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_csrf_origin_validation() {
        // Valid Origins
        assert!(is_origin_allowed("http://127.0.0.1:3000"));
        assert!(is_origin_allowed("http://localhost:3000"));
        assert!(is_origin_allowed("http://localhost"));
        assert!(is_origin_allowed("http://127.0.0.1"));
        assert!(is_origin_allowed("https://localhost:8080"));

        // Attackers / Bypasses
        assert!(!is_origin_allowed("http://127.0.0.1.evil.com"));
        assert!(!is_origin_allowed("http://localhost.evil.com"));
        assert!(!is_origin_allowed("http://evil.com"));
        assert!(!is_origin_allowed("null"));
        assert!(!is_origin_allowed(""));
        assert!(!is_origin_allowed("file:///etc/passwd"));
        assert!(!is_origin_allowed("javascript:alert(1)"));
    }

    #[test]
    fn test_host_header_validation() {
        assert!(is_host_allowed("localhost:3000"));
        assert!(is_host_allowed("127.0.0.1:3000"));
        assert!(is_host_allowed("localhost"));
        assert!(is_host_allowed("127.0.0.1"));

        assert!(!is_host_allowed("evil.com"));
        assert!(!is_host_allowed("evil.com:3000"));
        assert!(!is_host_allowed("localhost.attacker.com"));
    }
}
