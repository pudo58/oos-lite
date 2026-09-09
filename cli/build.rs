#[cfg(windows)]
fn main() {
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_string());
    let mut res = winres::WindowsResource::new();
    res.set_windres_path("C:\\Users\\ADMIN\\w64devkit\\bin\\windres.exe");
    res.set_icon("app.ico");
    res.set("CompanyName", "pudo58");
    res.set("FileDescription", "OOS-Lite Content-Addressed File Storage & Vault Drive");
    res.set("LegalCopyright", "Copyright (C) 2026 pudo58");
    res.set("ProductName", "OOS-Lite");
    res.set("ProductVersion", &version);
    res.set("FileVersion", &version);
    res.set("OriginalFilename", "oos-lite.exe");
    if let Err(e) = res.compile() {
        eprintln!("cargo:warning=Failed to compile Windows resource metadata: {}", e);
    }
}

#[cfg(not(windows))]
fn main() {}
