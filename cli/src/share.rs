use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Serialize, Deserialize)]
pub struct ShareInfo {
    pub id: String,
    pub path: String,
    pub token: String,
    pub expires_at: u64,
}

#[derive(Clone)]
pub struct ShareManager {
    shares: Arc<RwLock<HashMap<String, ShareInfo>>>,
    store_path: PathBuf,
    pub public_url: Arc<RwLock<Option<String>>>,
}

impl ShareManager {
    pub fn new(vault_dir: &Path) -> Self {
        let store_path = vault_dir.join("shares.json");
        let mut shares = HashMap::new();
        if store_path.exists() {
            if let Ok(data) = std::fs::read_to_string(&store_path) {
                if let Ok(parsed) = serde_json::from_str::<HashMap<String, ShareInfo>>(&data) {
                    shares = parsed;
                }
            }
        }
        Self {
            shares: Arc::new(RwLock::new(shares)),
            store_path,
            public_url: Arc::new(RwLock::new(None)),
        }
    }

    pub fn save(&self) {
        let shares = self.shares.read().unwrap();
        if let Ok(json) = serde_json::to_string_pretty(&*shares) {
            let _ = std::fs::write(&self.store_path, json);
        }
    }

    pub fn add_share(&self, path: String, expires_in_sec: u64) -> ShareInfo {
        let id = format!("{:016x}", rand::random::<u64>());
        let token = format!("{:016x}", rand::random::<u64>());
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let expires_at = if expires_in_sec > 0 { now + expires_in_sec } else { 0 };

        let info = ShareInfo { id: id.clone(), path, token, expires_at };
        self.shares.write().unwrap().insert(id, info.clone());
        self.save();
        info
    }

    pub fn get_share(&self, id: &str) -> Option<ShareInfo> {
        self.shares.read().unwrap().get(id).cloned()
    }

    pub fn list_shares(&self) -> Vec<ShareInfo> {
        let shares = self.shares.read().unwrap();
        let mut list: Vec<_> = shares.values().cloned().collect();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        list.retain(|s| s.expires_at == 0 || s.expires_at > now);
        list
    }

    pub fn revoke_share(&self, id: &str) -> bool {
        let removed = self.shares.write().unwrap().remove(id).is_some();
        if removed { self.save(); }
        removed
    }
}

use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};

const SHARE_DOWNLOAD_HTML: &str = include_str!("ui/share_download.html");
const SHARE_EXPIRED_HTML: &str = include_str!("ui/share_expired.html");

fn get_file_icon_svg(ext: &str) -> &'static str {
    match ext {
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" => {
            r#"<svg fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 8h14M5 8a2 2 0 110-4h14a2 2 0 110 4M5 8v10a2 2 0 002 2h10a2 2 0 002-2V8m-9 4h4"/></svg>"#
        },
        "pdf" | "doc" | "docx" | "txt" | "md" => {
            r#"<svg fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z"/></svg>"#
        },
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" => {
            r#"<svg fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 16l4.586-4.586a2 2 0 012.828 0L16 16m-2-2l1.586-1.586a2 2 0 012.828 0L20 14m-6-6h.01M6 20h12a2 2 0 002-2V6a2 2 0 00-2-2H6a2 2 0 00-2 2v12a2 2 0 002 2z"/></svg>"#
        },
        "rs" | "js" | "ts" | "py" | "c" | "cpp" | "go" | "html" | "css" | "json" => {
            r#"<svg fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 20l4-16m4 4l4 4-4 4M6 16l-4-4 4-4"/></svg>"#
        },
        _ => {
            r#"<svg fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M7 21h10a2 2 0 002-2V9.414a1 1 0 00-.293-.707l-5.414-5.414A1 1 0 0012.586 3H7a2 2 0 00-2 2v14a2 2 0 002 2z"/></svg>"#
        }
    }
}

fn get_file_icon_class(ext: &str) -> &'static str {
    match ext {
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" => "archive",
        "pdf" | "doc" | "docx" | "txt" | "md" => "document",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" => "image",
        "rs" | "js" | "ts" | "py" | "c" | "cpp" | "go" | "html" | "css" | "json" => "code",
        _ => "generic",
    }
}

fn get_folder_icon_svg() -> &'static str {
    r#"<svg fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z"/></svg>"#
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

impl ShareManager {
    pub fn spawn_tunnel(port: u16, public_url: Arc<RwLock<Option<String>>>, vault_dir: PathBuf) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                let _ = std::process::Command::new("taskkill")
                    .args(&["/F", "/IM", "cloudflared.exe", "/T"])
                    .creation_flags(0x08000000)
                    .output();
            }
            #[cfg(unix)]
            {
                let _ = std::process::Command::new("pkill")
                    .arg("-f")
                    .arg("cloudflared")
                    .output();
            }

            let exe_name = if cfg!(windows) { "cloudflared.exe" } else { "cloudflared" };
            let bin_dir = vault_dir.join("bin");
            let exe_path = bin_dir.join(exe_name);

            if !bin_dir.exists() {
                let _ = std::fs::create_dir_all(&bin_dir);
            }

            if !exe_path.exists() {
                tracing::info!("cloudflared not found locally. Downloading...");
                let url = if cfg!(windows) {
                    "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-windows-amd64.exe"
                } else if cfg!(target_os = "macos") {
                    "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-darwin-amd64.tgz" 
                    // Note: for macos it's a tgz, we would need extraction. But keeping it simple for windows/linux.
                } else {
                    "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64"
                };

                if let Ok(response) = ureq::get(url).call() {
                    let mut reader = response.into_body().into_reader();
                    if let Ok(mut file) = std::fs::File::create(&exe_path) {
                        let _ = std::io::copy(&mut reader, &mut file);
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            if let Ok(metadata) = std::fs::metadata(&exe_path) {
                                let mut perms = metadata.permissions();
                                perms.set_mode(0o755);
                                let _ = std::fs::set_permissions(&exe_path, perms);
                            }
                        }
                    }
                }
            }

            // Fallback to searching PATH if local download somehow failed or wasn't attempted (macOS)
            let final_exe = if exe_path.exists() {
                exe_path.to_string_lossy().to_string()
            } else {
                exe_name.to_string()
            };

            let mut cmd = Command::new(&final_exe);
            cmd.arg("tunnel")
               .arg("--url")
               .arg(format!("http://127.0.0.1:{}", port))
               .stdout(Stdio::piped())
               .stderr(Stdio::piped());

            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
            }
            if let Ok(mut child) = cmd.spawn() {
                tracing::info!("Spawned cloudflared tunnel for port {}", port);
                if let Some(stderr) = child.stderr.take() {
                    let reader = BufReader::new(stderr);
                    for line in reader.lines() {
                        if let Ok(l) = line {
                            if l.contains("trycloudflare.com") {
                                tracing::info!("Cloudflare Tunnel: {}", l);
                                if let Some(start_idx) = l.find("https://") {
                                    let remainder = &l[start_idx..];
                                    if let Some(end_idx) = remainder.find(".trycloudflare.com") {
                                        let url = &remainder[..end_idx + 18];
                                        if let Ok(mut p) = public_url.write() {
                                            *p = Some(url.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                let _ = child.wait();
            } else {
                tracing::error!("Failed to spawn cloudflared. Make sure it is installed or bundled.");
            }
        })
    }

    pub fn start_public_server(engine: std::sync::Arc<oos_lite_core::StorageEngine>, manager: ShareManager, port: u16) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let server = match tiny_http::Server::http(format!("0.0.0.0:{}", port)) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("Failed to bind public drop server on port {}: {}", port, e);
                    return;
                }
            };
            tracing::info!("Public Drop Server listening on 0.0.0.0:{}", port);

            for request in server.incoming_requests() {
                let url = request.url().to_string();
                if url.starts_with("/s/") {
                    // Extract ID and Token
                    let parts: Vec<&str> = url.split('?').collect();
                    let path_parts: Vec<&str> = parts[0].split('/').collect();
                    if path_parts.len() >= 3 {
                        let id = path_parts[2];
                        let is_download = path_parts.len() > 3 && path_parts[3] == "download";
                        
                        if let Some(share) = manager.get_share(id) {
                            // Normalize share.path
                            let clean_path = share.path.replace('\\', "/").trim_matches('/').to_string();
                            let folder_prefix = format!("{}/", clean_path);

                            // Inspect vault to determine if share.path is a folder or single file
                            let all_files = engine.list_files().unwrap_or_default();
                            let mut folder_files: Vec<(String, u64)> = Vec::new();

                            for (name, _, record) in &all_files {
                                let norm_name = name.replace('\\', "/");
                                if norm_name.starts_with(&folder_prefix) {
                                    let size = record.versions.last().map(|v| v.size_bytes).unwrap_or(0);
                                    folder_files.push((name.clone(), size));
                                }
                            }

                            let is_folder = !folder_files.is_empty();
                            let folder_name = clean_path.rsplit('/').next().unwrap_or(&clean_path);
                            let folder_name = if folder_name.is_empty() { "folder" } else { folder_name };
                            let total_folder_bytes: u64 = folder_files.iter().map(|(_, sz)| *sz).sum();

                            // Check expiration
                            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                            if share.expires_at > 0 && share.expires_at < now {
                                let display_title = if is_folder {
                                    format!("{} (Thư mục)", folder_name)
                                } else {
                                    std::path::Path::new(&share.path)
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or(&share.path)
                                        .to_string()
                                };
                                let html = SHARE_EXPIRED_HTML.replace("{{FILE_NAME}}", &display_title);
                                let response = tiny_http::Response::from_string(html)
                                    .with_status_code(410)
                                    .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap());
                                let _ = request.respond(response);
                                continue;
                            }

                            if is_download {
                                // Extract token from query params
                                let mut token_match = false;
                                if parts.len() > 1 {
                                    for param in parts[1].split('&') {
                                        let kv: Vec<&str> = param.split('=').collect();
                                        if kv.len() == 2 && kv[0] == "token" && kv[1] == share.token {
                                            token_match = true;
                                        }
                                    }
                                }
                                if !token_match {
                                    let redirect_url = format!("/s/{}?error=invalid_token", id);
                                    let response = tiny_http::Response::from_string("Redirecting...")
                                        .with_status_code(302)
                                        .with_header(tiny_http::Header::from_bytes(&b"Location"[..], redirect_url.as_bytes()).unwrap());
                                    let _ = request.respond(response);
                                    continue;
                                }

                                if is_folder {
                                    // Package folder into .zip archive on-the-fly
                                    let tmp_dir = std::env::temp_dir();
                                    let now_ns = SystemTime::now()
                                        .duration_since(UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_nanos();
                                    let zip_tmp_path = tmp_dir.join(format!(
                                        "oos_share_dir_{}_{}_{}.zip",
                                        std::process::id(),
                                        id,
                                        now_ns
                                    ));

                                    let zip_created = (|| -> anyhow::Result<()> {
                                        let zip_file = std::fs::File::create(&zip_tmp_path)?;
                                        let mut zip = zip::ZipWriter::new(zip_file);
                                        let options = zip::write::SimpleFileOptions::default()
                                            .compression_method(zip::CompressionMethod::Deflated)
                                            .unix_permissions(0o644);

                                        for (idx, (file_name, _)) in folder_files.iter().enumerate() {
                                            let clean_name = file_name.replace('\\', "/");
                                            let rel_path = if let Some(stripped) = clean_name.strip_prefix(&folder_prefix) {
                                                stripped
                                            } else if let Some(stripped) = clean_name.strip_prefix(&clean_path) {
                                                stripped.trim_start_matches('/')
                                            } else {
                                                &clean_name
                                            };
                                            let zip_entry_name = format!("{}/{}", folder_name, rel_path);

                                            let file_tmp_path = tmp_dir.join(format!(
                                                "oos_f_extract_{}_{}_{}_{}.tmp",
                                                std::process::id(),
                                                id,
                                                now_ns,
                                                idx
                                            ));

                                            if engine.get_file(file_name, &file_tmp_path).is_ok() {
                                                if let Ok(mut src) = std::fs::File::open(&file_tmp_path) {
                                                    if zip.start_file(&zip_entry_name, options).is_ok() {
                                                        let _ = std::io::copy(&mut src, &mut zip);
                                                    }
                                                }
                                                let _ = std::fs::remove_file(&file_tmp_path);
                                            }
                                        }

                                        zip.finish()?;
                                        Ok(())
                                    })();

                                    match zip_created {
                                        Ok(()) => {
                                            if let Ok(file) = std::fs::File::open(&zip_tmp_path) {
                                                let download_name = format!("{}.zip", folder_name);
                                                let response = tiny_http::Response::from_file(file)
                                                    .with_header(
                                                        tiny_http::Header::from_bytes(
                                                            &b"Content-Disposition"[..],
                                                            format!("attachment; filename=\"{}\"", download_name).as_bytes(),
                                                        )
                                                        .unwrap(),
                                                    )
                                                    .with_header(
                                                        tiny_http::Header::from_bytes(
                                                            &b"Content-Type"[..],
                                                            &b"application/zip"[..],
                                                        )
                                                        .unwrap(),
                                                    );
                                                let _ = request.respond(response);
                                                let _ = std::fs::remove_file(&zip_tmp_path);
                                            } else {
                                                let _ = std::fs::remove_file(&zip_tmp_path);
                                                let _ = request.respond(
                                                    tiny_http::Response::from_string("Failed to open generated zip")
                                                        .with_status_code(500),
                                                );
                                            }
                                        }
                                        Err(e) => {
                                            let _ = std::fs::remove_file(&zip_tmp_path);
                                            let _ = request.respond(
                                                tiny_http::Response::from_string(format!("Error generating archive: {}", e))
                                                    .with_status_code(500),
                                            );
                                        }
                                    }
                                } else {
                                    // Stream single file via temp
                                    let tmp_dir = std::env::temp_dir();
                                    let tmp_path = tmp_dir.join(format!("oos_share_{}_{}.tmp", std::process::id(), id));
                                    match engine.get_file(&share.path, &tmp_path) {
                                        Ok(_) => {
                                            if let Ok(file) = std::fs::File::open(&tmp_path) {
                                                let download_name = std::path::Path::new(&share.path)
                                                    .file_name()
                                                    .and_then(|n| n.to_str())
                                                    .unwrap_or("download.bin");
                                                let response = tiny_http::Response::from_file(file)
                                                    .with_header(tiny_http::Header::from_bytes(&b"Content-Disposition"[..], format!("attachment; filename=\"{}\"", download_name).as_bytes()).unwrap());
                                                let _ = request.respond(response);
                                                let _ = std::fs::remove_file(&tmp_path);
                                            } else {
                                                let _ = request.respond(tiny_http::Response::from_string("File read error").with_status_code(500));
                                            }
                                        }
                                        Err(_) => {
                                            let _ = request.respond(tiny_http::Response::from_string("File not found in vault").with_status_code(404));
                                        }
                                    }
                                }
                            } else {
                                // Extract query params
                                let query_str = if parts.len() > 1 { parts[1] } else { "" };
                                let mut has_error = false;
                                let mut prefill_token = "";
                                for param in query_str.split('&') {
                                    let kv: Vec<&str> = param.split('=').collect();
                                    if kv.len() == 2 {
                                        if kv[0] == "error" && kv[1] == "invalid_token" {
                                            has_error = true;
                                        } else if kv[0] == "token" {
                                            prefill_token = kv[1];
                                        }
                                    }
                                }

                                let error_html = if has_error {
                                    r#"<div class="error-alert">
                                        <svg fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"/>
                                        </svg>
                                        <span>Mật khẩu tải tệp không chính xác. Vui lòng kiểm tra lại!</span>
                                      </div>"#
                                } else {
                                    ""
                                };

                                let expires_badge = if share.expires_at == 0 {
                                    "Không giới hạn".to_string()
                                } else if share.expires_at > now {
                                    let diff = share.expires_at - now;
                                    if diff >= 86400 {
                                        format!("Còn {} ngày", diff / 86400)
                                    } else if diff >= 3600 {
                                        format!("Còn {} giờ", diff / 3600)
                                    } else if diff >= 60 {
                                        format!("Còn {} phút", diff / 60)
                                    } else {
                                        format!("Còn {} giây", diff)
                                    }
                                } else {
                                    "Đã hết hạn".to_string()
                                };

                                let (display_name, display_path, icon_svg, icon_class, extra_badges, download_btn_text, payload_desc) = if is_folder {
                                    let badges = format!(
                                        r#"<span class="version-tag">
                                            <svg fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z"/>
                                            </svg>
                                            {} tệp · {}
                                          </span>
                                          <span class="version-tag">
                                            <svg fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z"/>
                                            </svg>
                                            Kho cục bộ đã mã hóa
                                          </span>"#,
                                        folder_files.len(),
                                        format_bytes(total_folder_bytes)
                                    );
                                    (
                                        format!("{} (Thư mục)", folder_name),
                                        format!("{}/", clean_path),
                                        get_folder_icon_svg(),
                                        "archive",
                                        badges,
                                        "Tải thư mục (.zip)",
                                        "Toàn bộ thư mục được tự động nén thành file .zip và truyền trực tiếp từ kho lưu trữ OOS-Lite của người gửi qua Cloudflare Encrypted Tunnel."
                                    )
                                } else {
                                    let file_name = std::path::Path::new(&share.path)
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or(&share.path);
                                    let ext = std::path::Path::new(file_name)
                                        .extension()
                                        .and_then(|e| e.to_str())
                                        .unwrap_or("")
                                        .to_lowercase();
                                    let badges = r#"<span class="version-tag">
                                        <svg fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z"/>
                                        </svg>
                                        Kho cục bộ đã mã hóa
                                      </span>"#.to_string();
                                    (
                                        file_name.to_string(),
                                        share.path.clone(),
                                        get_file_icon_svg(&ext),
                                        get_file_icon_class(&ext),
                                        badges,
                                        "Tải xuống tập tin",
                                        "Tệp tin được truyền trực tiếp từ kho lưu trữ OOS-Lite của người gửi qua Cloudflare Encrypted Tunnel, không lưu trữ qua máy chủ trung gian."
                                    )
                                };

                                let html = SHARE_DOWNLOAD_HTML
                                    .replace("{{FILE_NAME}}", &display_name)
                                    .replace("{{FILE_PATH}}", &display_path)
                                    .replace("{{FILE_ICON_SVG}}", icon_svg)
                                    .replace("{{FILE_ICON_CLASS}}", icon_class)
                                    .replace("{{EXPIRES_BADGE}}", &expires_badge)
                                    .replace("{{EXTRA_BADGES}}", &extra_badges)
                                    .replace("{{DOWNLOAD_BTN_TEXT}}", download_btn_text)
                                    .replace("{{PAYLOAD_DESC}}", payload_desc)
                                    .replace("{{SHARE_ID}}", id)
                                    .replace("{{ERROR_HTML}}", error_html)
                                    .replace("{{TOKEN_VALUE}}", prefill_token);

                                let response = tiny_http::Response::from_string(html)
                                    .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap());
                                let _ = request.respond(response);
                            }
                            continue;
                        }
                    }
                }
                let _ = request.respond(tiny_http::Response::from_string("Not Found").with_status_code(404));
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn test_format_bytes_display() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1 KB");
        assert_eq!(format_bytes(1024 * 500), "500 KB");
        assert_eq!(format_bytes(1024 * 1024 * 3), "3.0 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024 * 2), "2.00 GB");
    }

    #[test]
    fn test_folder_zip_archive_roundtrip() {
        let temp_dir = std::env::temp_dir().join(format!("oos_test_zip_{}", rand::random::<u64>()));
        let _ = std::fs::create_dir_all(&temp_dir);

        let store_dir = temp_dir.join("store");
        let engine = oos_lite_core::StorageEngine::open(&store_dir).expect("Engine open failed");

        // Write 2 test files under a folder prefix
        let f1_path = temp_dir.join("f1.txt");
        std::fs::write(&f1_path, b"Hello from file 1").unwrap();
        engine.put_file_named("my-project/f1.txt", &f1_path).unwrap();

        let f2_path = temp_dir.join("f2.txt");
        std::fs::write(&f2_path, b"Hello from nested file 2").unwrap();
        engine.put_file_named("my-project/sub/f2.txt", &f2_path).unwrap();

        // Query files with folder prefix
        let clean_path = "my-project";
        let folder_prefix = format!("{}/", clean_path);
        let all_files = engine.list_files().unwrap();
        let mut folder_files = Vec::new();
        for (name, _, record) in &all_files {
            let norm = name.replace('\\', "/");
            if norm.starts_with(&folder_prefix) {
                let sz = record.versions.last().map(|v| v.size_bytes).unwrap_or(0);
                folder_files.push((name.clone(), sz));
            }
        }
        assert_eq!(folder_files.len(), 2);

        // Create zip archive
        let zip_path = temp_dir.join("test_out.zip");
        let zip_file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(zip_file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        for (idx, (file_name, _)) in folder_files.iter().enumerate() {
            let clean_name = file_name.replace('\\', "/");
            let rel_path = clean_name.strip_prefix(&folder_prefix).unwrap_or(&clean_name);
            let zip_entry_name = format!("{}/{}", clean_path, rel_path);

            let tmp_extract = temp_dir.join(format!("tmp_ext_{}.tmp", idx));
            engine.get_file(file_name, &tmp_extract).unwrap();
            let mut src = std::fs::File::open(&tmp_extract).unwrap();
            zip.start_file(&zip_entry_name, options).unwrap();
            std::io::copy(&mut src, &mut zip).unwrap();
            let _ = std::fs::remove_file(&tmp_extract);
        }
        zip.finish().unwrap();

        // Validate zip file contents
        let zip_read_file = std::fs::File::open(&zip_path).unwrap();
        let mut archive = zip::ZipArchive::new(zip_read_file).unwrap();
        assert_eq!(archive.len(), 2);

        {
            let mut entry1 = archive.by_name("my-project/f1.txt").unwrap();
            let mut content1 = String::new();
            entry1.read_to_string(&mut content1).unwrap();
            assert_eq!(content1, "Hello from file 1");
        }

        {
            let mut entry2 = archive.by_name("my-project/sub/f2.txt").unwrap();
            let mut content2 = String::new();
            entry2.read_to_string(&mut content2).unwrap();
            assert_eq!(content2, "Hello from nested file 2");
        }

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
