use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};
use tiny_http::{Header, Method, Response, Server, StatusCode};
use tracing::error;
use url::Url;

use oos_lite_core::StorageEngine;

const INDEX_HTML: &str = include_str!("index.html");

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

fn json_response<T: Serialize>(data: &T) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::to_vec(data).unwrap_or_else(|_| b"{}".to_vec());
    let ct = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
    let cors = Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap();
    Response::from_data(body).with_header(ct).with_header(cors)
}

fn error_response(status: u16, msg: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::to_vec(&ErrorResponse { error: msg.to_string() }).unwrap();
    let ct = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
    let cors = Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap();
    Response::from_data(body)
        .with_status_code(StatusCode(status))
        .with_header(ct)
        .with_header(cors)
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

pub fn start_ui_server(engine: Arc<StorageEngine>, host: &str, port: u16, no_open: bool) -> anyhow::Result<()> {
    let addr = format!("{}:{}", host, port);
    let server = Server::http(&addr)
        .map_err(|e| anyhow::anyhow!("Failed to bind UI server on {}: {}", addr, e))?;

    let local_url = if host == "0.0.0.0" {
        format!("http://localhost:{}", port)
    } else {
        format!("http://{}:{}", host, port)
    };

    println!("============================================================");
    println!("       OOS-Lite Web UI Dashboard running at:");
    println!("       {}", local_url);
    println!("       Listening on: {}", addr);
    println!("       Press Ctrl+C to stop the dashboard server.");
    println!("============================================================");

    if !no_open {
        let open_url = local_url.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            #[cfg(target_os = "windows")]
            {
                let _ = std::process::Command::new("cmd")
                    .args(["/C", "start", &open_url])
                    .spawn();
            }
            #[cfg(target_os = "macos")]
            {
                let _ = std::process::Command::new("open").arg(&open_url).spawn();
            }
            #[cfg(target_os = "linux")]
            {
                let _ = std::process::Command::new("xdg-open").arg(&open_url).spawn();
            }
        });
    }

    let server = Arc::new(server);

    for request in server.incoming_requests() {
        let engine_clone = Arc::clone(&engine);
        std::thread::spawn(move || {
            if let Err(e) = handle_request(engine_clone, request) {
                error!("Request error: {:?}", e);
            }
        });
    }

    Ok(())
}

fn handle_request(engine: Arc<StorageEngine>, mut request: tiny_http::Request) -> anyhow::Result<()> {
    let parsed_url = Url::parse(&format!("http://localhost{}", request.url()))?;
    let path = parsed_url.path().to_string();
    let method = request.method().clone();

    // CORS preflight
    if method == Method::Options {
        let cors1 = Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap();
        let cors2 = Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"GET, POST, OPTIONS"[..]).unwrap();
        let cors3 = Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"Content-Type"[..]).unwrap();
        let resp = Response::empty(StatusCode(204))
            .with_header(cors1)
            .with_header(cors2)
            .with_header(cors3);
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
                        match std::fs::read(&tmp_path) {
                            Ok(bytes) => {
                                let _ = std::fs::remove_file(&tmp_path);
                                let download_name = Path::new(&target_str)
                                    .file_name()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("download.bin");
                                let disp_val = format!("attachment; filename=\"{}\"", download_name);
                                let disp = Header::from_bytes(&b"Content-Disposition"[..], disp_val.as_bytes()).unwrap();
                                let ct = Header::from_bytes(&b"Content-Type"[..], &b"application/octet-stream"[..]).unwrap();
                                let resp = Response::from_data(bytes).with_header(disp).with_header(ct);
                                let _ = request.respond(resp);
                            }
                            Err(e) => {
                                let _ = std::fs::remove_file(&tmp_path);
                                let _ = request.respond(error_response(500, &format!("Read error: {}", e)));
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
                    if dir_str.is_empty() || dir_str.contains("..") {
                        let _ = request.respond(error_response(400, "Invalid or disallowed restore directory path"));
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

        _ => {
            let _ = request.respond(error_response(404, "Endpoint not found"));
        }
    }

    Ok(())
}
