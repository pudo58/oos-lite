use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::path::{Path, PathBuf};
use tracing::{info, error};
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
                            // Check expiration
                            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                            if share.expires_at > 0 && share.expires_at < now {
                                let _ = request.respond(tiny_http::Response::from_string("Link expired").with_status_code(410));
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
                                    let _ = request.respond(tiny_http::Response::from_string("Unauthorized").with_status_code(401));
                                    continue;
                                }

                                // Stream the file via temp
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
                            } else {
                                // Serve the HTML page
                                let html = format!(r#"
                                    <!DOCTYPE html>
                                    <html>
                                    <head><title>OOS-Lite Secure Drop</title></head>
                                    <body style="font-family: monospace; text-align: center; padding: 50px; background: #0f172a; color: #fff;">
                                        <h2>OOS-Lite Secure Drop</h2>
                                        <p>File: {}</p>
                                        <p>Expires: {}</p>
                                        <form action="/s/{}/download" method="GET">
                                            <input type="password" name="token" placeholder="Enter Token" style="padding: 10px; border-radius: 5px;"/>
                                            <button type="submit" style="padding: 10px 20px; background: #6366f1; color: white; border: none; border-radius: 5px;">Download</button>
                                        </form>
                                    </body>
                                    </html>
                                "#, share.path, if share.expires_at == 0 { "Never".to_string() } else { "Soon".to_string() }, id);
                                let response = tiny_http::Response::from_string(html)
                                    .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/html"[..]).unwrap());
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
