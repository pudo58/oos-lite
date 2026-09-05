use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use tiny_http::{Header, Request, Response, Server, StatusCode};
use tracing::{error, info};

use oos_lite_core::vfs::{VfsNodeType, VfsTree};
use oos_lite_core::StorageEngine;

pub struct WebDavContext {
    engine: Arc<StorageEngine>,
    cache_mb: usize,
    vfs: std::sync::RwLock<VfsTree>,
    last_refresh: std::sync::Mutex<std::time::Instant>,
}

impl WebDavContext {
    pub fn new(engine: Arc<StorageEngine>, cache_mb: usize) -> anyhow::Result<Self> {
        let vfs = VfsTree::build(Arc::clone(&engine), cache_mb * 1024 * 1024)?;
        Ok(Self {
            engine,
            cache_mb,
            vfs: std::sync::RwLock::new(vfs),
            last_refresh: std::sync::Mutex::new(std::time::Instant::now()),
        })
    }

    pub fn refresh_if_needed(&self) {
        let mut last = self.last_refresh.lock().unwrap_or_else(|p| p.into_inner());
        if last.elapsed() > std::time::Duration::from_millis(500) {
            if let Ok(new_vfs) = VfsTree::build(Arc::clone(&self.engine), self.cache_mb * 1024 * 1024) {
                if let Ok(mut w) = self.vfs.write() {
                    *w = new_vfs;
                    *last = std::time::Instant::now();
                }
            }
        }
    }
}

pub fn start_webdav_server(
    engine: Arc<StorageEngine>,
    host: &str,
    port: u16,
    cache_mb: usize,
    workers: usize,
    running: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let addr = format!("{}:{}", host, port);
    let server = Arc::new(
        Server::http(&addr)
            .map_err(|e| anyhow::anyhow!("Failed to bind WebDAV server on {}: {}", addr, e))?,
    );

    let ctx = Arc::new(WebDavContext::new(engine, cache_mb)?);

    info!(
        address = %addr,
        workers = workers,
        cache_mb = cache_mb,
        "Starting multi-threaded read-only WebDAV server..."
    );

    // Spawn worker thread pool
    for worker_id in 0..workers {
        let server = Arc::clone(&server);
        let ctx = Arc::clone(&ctx);
        let running = Arc::clone(&running);

        thread::Builder::new()
            .name(format!("webdav-worker-{}", worker_id))
            .spawn(move || {
                while running.load(Ordering::Relaxed) {
                    match server.recv() {
                        Ok(request) => {
                            handle_request(request, &ctx);
                        }
                        Err(e) => {
                            if running.load(Ordering::Relaxed) {
                                error!("WebDAV server.recv() error: {}", e);
                            }
                            break;
                        }
                    }
                }
            })?;
    }

    Ok(())
}

fn handle_request(request: Request, ctx: &WebDavContext) {
    let raw_url = request.url().to_string();
    let path = decode_url_path(&raw_url);
    let method_str = request.method().as_str();

    if method_str == "PROPFIND" {
        ctx.refresh_if_needed();
    }

    let vfs = ctx.vfs.read().unwrap_or_else(|p| p.into_inner());

    match method_str {
        "OPTIONS" => handle_options(request),
        "PROPFIND" => handle_propfind(request, &vfs, &path),
        "LOCK" => handle_lock(request, &vfs, &path),
        "UNLOCK" => handle_unlock(request),
        "GET" => handle_get(request, &vfs, &path, false),
        "HEAD" => handle_get(request, &vfs, &path, true),
        "PUT" | "DELETE" | "MKCOL" | "MOVE" | "COPY" | "PATCH" | "POST" => {
            let resp = Response::from_string("Read-only OOS-Lite filesystem")
                .with_status_code(StatusCode(403))
                .with_header(Header::from_bytes(&b"Content-Type"[..], &b"text/plain"[..]).unwrap());
            let _ = request.respond(resp);
        }
        _ => {
            let resp = Response::empty(StatusCode(405));
            let _ = request.respond(resp);
        }
    }
}

fn handle_options(request: Request) {
    let dav_header = Header::from_bytes(&b"DAV"[..], &b"1, 2"[..]).unwrap();
    let ms_header = Header::from_bytes(&b"MS-Author-Via"[..], &b"DAV"[..]).unwrap();
    let allow_header = Header::from_bytes(
        &b"Allow"[..],
        &b"OPTIONS, GET, HEAD, PROPFIND, LOCK, UNLOCK"[..],
    )
    .unwrap();

    let resp = Response::empty(StatusCode(200))
        .with_header(dav_header)
        .with_header(ms_header)
        .with_header(allow_header);

    let _ = request.respond(resp);
}

fn handle_propfind(request: Request, vfs: &VfsTree, path: &str) {
    let depth = request
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case("Depth"))
        .map(|h| h.value.as_str().trim())
        .unwrap_or("1");

    let target_node = match vfs.resolve_path(path) {
        Some(node) => node,
        None => {
            let resp = Response::empty(StatusCode(404));
            let _ = request.respond(resp);
            return;
        }
    };

    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<D:multistatus xmlns:D=\"DAV:\">\n");

    let clean_path = path.trim_matches('/');
    let target_href = if clean_path.is_empty() {
        "/".to_string()
    } else if target_node.kind == VfsNodeType::Directory {
        format!("/{}/", clean_path)
    } else {
        format!("/{}", clean_path)
    };

    // 1. Response for the requested node
    xml.push_str(&format_prop_entry(
        &target_href,
        &target_node.name,
        target_node.kind == VfsNodeType::Directory,
        target_node.size,
        target_node.mtime,
    ));

    // 2. If Depth is 1 and target is Directory, include direct children
    if depth != "0" && target_node.kind == VfsNodeType::Directory {
        if let Some(children) = vfs.readdir(target_node.ino) {
            for child in children {
                let child_href = if clean_path.is_empty() {
                    if child.kind == VfsNodeType::Directory {
                        format!("/{}/", child.name)
                    } else {
                        format!("/{}", child.name)
                    }
                } else {
                    if child.kind == VfsNodeType::Directory {
                        format!("/{}/{}/", clean_path, child.name)
                    } else {
                        format!("/{}/{}", clean_path, child.name)
                    }
                };

                xml.push_str(&format_prop_entry(
                    &child_href,
                    &child.name,
                    child.kind == VfsNodeType::Directory,
                    child.size,
                    child.mtime,
                ));
            }
        }
    }

    xml.push_str("</D:multistatus>\n");

    let ct = Header::from_bytes(&b"Content-Type"[..], &b"application/xml; charset=utf-8"[..]).unwrap();
    let resp = Response::from_string(xml)
        .with_status_code(StatusCode(207))
        .with_header(ct);

    let _ = request.respond(resp);
}

fn format_prop_entry(href: &str, name: &str, is_dir: bool, size: u64, mtime: u64) -> String {
    let date_str = format_http_date(mtime);
    let mut s = format!(
        "  <D:response>\n    <D:href>{}</D:href>\n    <D:propstat>\n      <D:prop>\n        <D:displayname>{}</D:displayname>\n",
        escape_xml(href),
        escape_xml(name)
    );

    if is_dir {
        s.push_str("        <D:resourcetype><D:collection/></D:resourcetype>\n");
        s.push_str(&format!("        <D:getlastmodified>{}</D:getlastmodified>\n", date_str));
    } else {
        s.push_str("        <D:resourcetype/>\n");
        s.push_str(&format!("        <D:getcontentlength>{}</D:getcontentlength>\n", size));
        s.push_str("        <D:getcontenttype>application/octet-stream</D:getcontenttype>\n");
        s.push_str(&format!("        <D:getlastmodified>{}</D:getlastmodified>\n", date_str));
    }

    s.push_str("      </D:prop>\n      <D:status>HTTP/1.1 200 OK</D:status>\n    </D:propstat>\n  </D:response>\n");
    s
}

fn handle_lock(request: Request, vfs: &VfsTree, path: &str) {
    if vfs.resolve_path(path).is_none() {
        let resp = Response::empty(StatusCode(404));
        let _ = request.respond(resp);
        return;
    }

    // Windows Explorer expects a successful lock response with an opaque lock token.
    let lock_token = "opaquelocktoken:00000000-0000-0000-0000-000000000001";
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
        <D:prop xmlns:D=\"DAV:\">\n  \
          <D:lockdiscovery>\n    \
            <D:activelock>\n      \
              <D:locktype><D:write/></D:locktype>\n      \
              <D:lockscope><D:exclusive/></D:lockscope>\n      \
              <D:depth>0</D:depth>\n      \
              <D:timeout>Second-3600</D:timeout>\n      \
              <D:locktoken><D:href>{}</D:href></D:locktoken>\n    \
            </D:activelock>\n  \
          </D:lockdiscovery>\n\
        </D:prop>\n",
        lock_token
    );

    let ct = Header::from_bytes(&b"Content-Type"[..], &b"application/xml; charset=utf-8"[..]).unwrap();
    let token_hdr = Header::from_bytes(
        &b"Lock-Token"[..],
        format!("<{}>", lock_token).as_bytes(),
    )
    .unwrap();

    let resp = Response::from_string(body)
        .with_status_code(StatusCode(200))
        .with_header(ct)
        .with_header(token_hdr);

    let _ = request.respond(resp);
}

fn handle_unlock(request: Request) {
    let resp = Response::empty(StatusCode(204));
    let _ = request.respond(resp);
}

fn handle_get(request: Request, vfs: &VfsTree, path: &str, is_head: bool) {
    let node = match vfs.resolve_path(path) {
        Some(n) => n,
        None => {
            let resp = Response::empty(StatusCode(404));
            let _ = request.respond(resp);
            return;
        }
    };

    if node.kind == VfsNodeType::Directory {
        if is_head {
            let resp = Response::empty(StatusCode(200))
                .with_header(Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap());
            let _ = request.respond(resp);
            return;
        }

        // Render HTML directory view for browser inspection
        let mut html = format!(
            "<!DOCTYPE html><html><head><meta charset='utf-8'><title>OOS-Lite WebDAV: {}</title>\
             <style>body{{font-family:sans-serif;margin:2rem;}}a{{text-decoration:none;}}a:hover{{text-decoration:underline;}}ul{{line-height:1.8;}}</style>\
             </head><body><h1>Directory: /{}</h1><hr/><ul>",
            escape_xml(&node.name),
            escape_xml(path.trim_matches('/'))
        );

        if let Some(children) = vfs.readdir(node.ino) {
            for child in children {
                let href = format!("{}/", child.name);
                let suffix = if child.kind == VfsNodeType::Directory { "/" } else { "" };
                html.push_str(&format!(
                    "<li><a href=\"{}\">{}{}</a> ({} bytes)</li>",
                    escape_xml(&href),
                    escape_xml(&child.name),
                    suffix,
                    child.size
                ));
            }
        }
        html.push_str("</ul></body></html>");

        let resp = Response::from_string(html)
            .with_status_code(StatusCode(200))
            .with_header(Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap());
        let _ = request.respond(resp);
        return;
    }

    let manifest_id = match &node.manifest_id {
        Some(id) => id,
        None => {
            let resp = Response::empty(StatusCode(200));
            let _ = request.respond(resp);
            return;
        }
    };

    let total_size = node.size;
    let range_hdr = request
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case("Range"))
        .map(|h| h.value.as_str().trim());

    let (start, length, is_partial) = if let Some(range_val) = range_hdr {
        if let Some((s, len)) = parse_range_header(range_val, total_size) {
            (s, len, true)
        } else {
            let resp = Response::empty(StatusCode(416))
                .with_header(Header::from_bytes(&b"Content-Range"[..], format!("bytes */{}", total_size).as_bytes()).unwrap());
            let _ = request.respond(resp);
            return;
        }
    } else {
        (0, total_size, false)
    };

    let ct = Header::from_bytes(&b"Content-Type"[..], &b"application/octet-stream"[..]).unwrap();
    let accept_ranges = Header::from_bytes(&b"Accept-Ranges"[..], &b"bytes"[..]).unwrap();

    if is_head {
        let mut resp = Response::empty(if is_partial { StatusCode(206) } else { StatusCode(200) })
            .with_header(ct)
            .with_header(accept_ranges)
            .with_header(Header::from_bytes(&b"Content-Length"[..], length.to_string().as_bytes()).unwrap());

        if is_partial {
            let range_str = format!("bytes {}-{}/{}", start, start + length - 1, total_size);
            resp = resp.with_header(Header::from_bytes(&b"Content-Range"[..], range_str.as_bytes()).unwrap());
        }

        let _ = request.respond(resp);
        return;
    }

    match vfs.read_range(manifest_id, start, length) {
        Ok(data) => {
            let mut resp = Response::new(
                if is_partial { StatusCode(206) } else { StatusCode(200) },
                vec![
                    ct,
                    accept_ranges,
                    Header::from_bytes(&b"Content-Length"[..], data.len().to_string().as_bytes()).unwrap(),
                ],
                Cursor::new(data),
                Some(length as usize),
                None,
            );

            if is_partial {
                let range_str = format!("bytes {}-{}/{}", start, start + length - 1, total_size);
                resp = resp.with_header(Header::from_bytes(&b"Content-Range"[..], range_str.as_bytes()).unwrap());
            }

            let _ = request.respond(resp);
        }
        Err(e) => {
            error!(path = %path, offset = start, length = length, error = %e, "WebDAV read_range failed");
            let resp = Response::empty(StatusCode(500));
            let _ = request.respond(resp);
        }
    }
}

pub fn parse_range_header(header: &str, total_size: u64) -> Option<(u64, u64)> {
    if total_size == 0 {
        return Some((0, 0));
    }

    let prefix = "bytes=";
    if !header.starts_with(prefix) {
        return None;
    }

    let range_str = &header[prefix.len()..];
    let parts: Vec<&str> = range_str.split('-').collect();
    if parts.len() != 2 {
        return None;
    }

    let start_str = parts[0].trim();
    let end_str = parts[1].trim();

    if start_str.is_empty() {
        // Suffix range: "-500" -> last 500 bytes
        let suffix_len: u64 = end_str.parse().ok()?;
        let start = total_size.saturating_sub(suffix_len);
        let len = total_size - start;
        Some((start, len))
    } else {
        let start: u64 = start_str.parse().ok()?;
        if start >= total_size {
            return None;
        }

        let end = if end_str.is_empty() {
            total_size - 1
        } else {
            let e: u64 = end_str.parse().ok()?;
            e.min(total_size - 1)
        };

        if end < start {
            return None;
        }

        let len = end - start + 1;
        Some((start, len))
    }
}

fn decode_url_path(url: &str) -> String {
    let clean = url.split('?').next().unwrap_or(url);
    urlencoding_decode(clean)
}

fn urlencoding_decode(s: &str) -> String {
    let mut bytes = Vec::with_capacity(s.len());
    let mut chars = s.as_bytes().iter();
    while let Some(&b) = chars.next() {
        if b == b'%' {
            if let (Some(&h1), Some(&h2)) = (chars.next(), chars.next()) {
                if let Ok(val) = u8::from_str_radix(
                    std::str::from_utf8(&[h1, h2]).unwrap_or(""),
                    16,
                ) {
                    bytes.push(val);
                    continue;
                }
            }
        }
        bytes.push(b);
    }
    String::from_utf8_lossy(&bytes).to_string()
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn format_http_date(ts: u64) -> String {
    let days_since_epoch = (ts / 86400) as i64;
    let day_seconds = (ts % 86400) as u32;
    let hour = day_seconds / 3600;
    let minute = (day_seconds % 3600) / 60;
    let second = day_seconds % 60;

    let day_of_week = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][(days_since_epoch.rem_euclid(7)) as usize];

    let z = days_since_epoch + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun",
        "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let month_str = months[(m - 1) as usize];

    format!("{day_of_week}, {d:02} {month_str} {y} {hour:02}:{minute:02}:{second:02} GMT")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_http_date() {
        // 0 -> Thu, 01 Jan 1970 00:00:00 GMT
        assert_eq!(format_http_date(0), "Thu, 01 Jan 1970 00:00:00 GMT");
        // 1700000000 -> Tue, 14 Nov 2023 22:13:20 GMT
        assert_eq!(format_http_date(1700000000), "Tue, 14 Nov 2023 22:13:20 GMT");
    }

    #[test]
    fn test_parse_range_header() {
        let total = 1000u64;
        assert_eq!(parse_range_header("bytes=0-499", total), Some((0, 500)));
        assert_eq!(parse_range_header("bytes=500-999", total), Some((500, 500)));
        assert_eq!(parse_range_header("bytes=500-", total), Some((500, 500)));
        assert_eq!(parse_range_header("bytes=-100", total), Some((900, 100)));
        assert_eq!(parse_range_header("bytes=1000-", total), None);
        assert_eq!(parse_range_header("invalid", total), None);
    }

    #[test]
    fn test_escape_xml() {
        assert_eq!(escape_xml("cat & dog <'test'>"), "cat &amp; dog &lt;&apos;test&apos;&gt;");
    }
}
