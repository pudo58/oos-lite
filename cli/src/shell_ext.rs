/// Windows Explorer Context Menu Integration via HKCU Registry
///
/// Implements standard Windows Explorer static cascading context menus using
/// ExtendedSubCommandsKey with distinct menus for Files vs Directories:
///   - Files       : HKCU\Software\Classes\OOSLite.FileMenu\shell
///   - Directories : HKCU\Software\Classes\OOSLite.DirMenu\shell
///   - Background  : HKCU\Software\Classes\OOSLite.DirMenuBg\shell
///
/// Anchors:
///   - Files     : HKCU\Software\Classes\*\shell\OOSLite
///   - Folders   : HKCU\Software\Classes\Directory\shell\OOSLite
///   - Bg Folder : HKCU\Software\Classes\Directory\Background\shell\OOSLite
///
/// All entries live under HKCU — zero admin elevation required.

#[cfg(windows)]
#[allow(dead_code)]
pub mod windows {
    use std::os::windows::process::CommandExt;
    use winreg::enums::*;
    use winreg::RegKey;

    const CREATE_NO_WINDOW: u32 = 0x08000000;
    const MENU_KEY: &str = "OOSLite";
    const MENU_LABEL: &str = "OOS-Lite";

    // ── Helpers ──────────────────────────────────────────────────────────────

    pub fn gui_exe() -> String {
        std::env::current_exe()
            .ok()
            .and_then(|p| {
                let gui = p.parent()?.join("oos-lite-gui.exe");
                Some(gui.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "oos-lite-gui.exe".to_owned())
    }

    pub fn cli_exe() -> String {
        std::env::current_exe()
            .ok()
            .and_then(|p| {
                let cli = p.parent()?.join("oos-lite.exe");
                Some(cli.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "oos-lite.exe".to_owned())
    }

    pub fn icon_path() -> String {
        // 1. Try next to current running executable
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(parent) = exe_path.parent() {
                let ico = parent.join("app.ico");
                if ico.exists() {
                    return ico.to_string_lossy().into_owned();
                }
            }
        }
        // 2. Try installed directory in LocalAppData
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            let p = std::path::PathBuf::from(local_app_data).join(r"Programs\OOS-Lite\app.ico");
            if p.exists() {
                return p.to_string_lossy().into_owned();
            }
        }
        // 3. Try relative to current working directory (development)
        if let Ok(cwd) = std::env::current_dir() {
            let p = cwd.join(r"cli\app.ico");
            if p.exists() {
                return p.to_string_lossy().into_owned();
            }
        }
        // 4. Fallback to GUI executable
        gui_exe()
    }

    // ── Registration ─────────────────────────────────────────────────────────

    /// Returns true when our context menu keys exist.
    pub fn is_registered() -> bool {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        hkcu.open_subkey(format!(r"Software\Classes\*\shell\{}", MENU_KEY))
            .is_ok()
            && hkcu.open_subkey(r"Software\Classes\OOSLite.FileMenu").is_ok()
    }

    /// Write all registry keys that add the OOS-Lite cascade submenu.
    pub fn register() -> anyhow::Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let cli = cli_exe();
        let icon = icon_path();

        // 1. Register dedicated submenus
        register_file_menu(&hkcu, r"Software\Classes\OOSLite.FileMenu", &cli, &icon)?;
        register_dir_menu(&hkcu, r"Software\Classes\OOSLite.DirMenu", &cli, &icon, "%1")?;
        register_dir_menu(&hkcu, r"Software\Classes\OOSLite.DirMenuBg", &cli, &icon, "%V")?;

        // 2. Create the anchor keys
        register_anchor(
            &hkcu,
            r"Software\Classes\*\shell\OOSLite",
            "OOSLite.FileMenu",
            &icon,
        )?;
        register_anchor(
            &hkcu,
            r"Software\Classes\Directory\shell\OOSLite",
            "OOSLite.DirMenu",
            &icon,
        )?;
        register_anchor(
            &hkcu,
            r"Software\Classes\Directory\Background\shell\OOSLite",
            "OOSLite.DirMenuBg",
            &icon,
        )?;

        Ok(())
    }

    /// Remove all OOS-Lite context menu registry keys.
    pub fn unregister() -> anyhow::Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        for path in &[
            r"Software\Classes\*\shell\OOSLite",
            r"Software\Classes\Directory\shell\OOSLite",
            r"Software\Classes\Directory\Background\shell\OOSLite",
            r"Software\Classes\OOSLite.Menu",
            r"Software\Classes\OOSLite.MenuBg",
            r"Software\Classes\OOSLite.FileMenu",
            r"Software\Classes\OOSLite.DirMenu",
            r"Software\Classes\OOSLite.DirMenuBg",
        ] {
            let _ = hkcu.delete_subkey_all(path);
        }
        Ok(())
    }

    // ── Per-target registration ───────────────────────────────────────────────

    fn register_anchor(
        hkcu: &RegKey,
        path: &str,
        extended_key: &str,
        icon: &str,
    ) -> anyhow::Result<()> {
        // Clean up any legacy nested shell subkey under the anchor so it doesn't conflict
        let _ = hkcu.delete_subkey_all(format!(r"{}\shell", path));

        let (key, _) = hkcu.create_subkey(path)?;
        let _ = key.delete_value("SubCommands");
        let _ = key.delete_value(""); // Make sure (Default) is not set
        key.set_value("MUIVerb", &MENU_LABEL)?;
        key.set_value("ExtendedSubCommandsKey", &extended_key)?;
        key.set_value("Icon", &icon)?;

        Ok(())
    }

    /// Menu for Files: Store, View History, Restore
    fn register_file_menu(
        hkcu: &RegKey,
        base_path: &str,
        cli: &str,
        icon: &str,
    ) -> anyhow::Result<()> {
        let shell_path = format!(r"{}\shell", base_path);

        // ── 1. Store in Vault ─────────────────────────────────────────────
        {
            let key_path = format!(r"{}\01_StoreFile", shell_path);
            let (k, _) = hkcu.create_subkey(&key_path)?;
            k.set_value("", &"Store in Vault")?;
            k.set_value("MUIVerb", &"Store in Vault")?;
            k.set_value("Icon", &icon)?;
            let (cmd, _) = hkcu.create_subkey(format!(r"{}\command", key_path))?;
            cmd.set_value(
                "",
                &format!(
                    "\"{}\" context-menu store-file \"%1\"",
                    cli
                ),
            )?;
        }

        // ── 2. View Version History ───────────────────────────────────────
        {
            let key_path = format!(r"{}\02_ViewHistory", shell_path);
            let (k, _) = hkcu.create_subkey(&key_path)?;
            k.set_value("", &"View Version History")?;
            k.set_value("MUIVerb", &"View Version History")?;
            k.set_value("Icon", &icon)?;
            let (cmd, _) = hkcu.create_subkey(format!(r"{}\command", key_path))?;
            cmd.set_value(
                "",
                &format!(
                    "\"{}\" context-menu view-history \"%1\"",
                    cli
                ),
            )?;
        }

        // ── 3. Restore Version ───────────────────────────────────────────
        {
            let key_path = format!(r"{}\03_Restore", shell_path);
            let (k, _) = hkcu.create_subkey(&key_path)?;
            k.set_value("", &"Restore Version...")?;
            k.set_value("MUIVerb", &"Restore Version...")?;
            k.set_value("Icon", &icon)?;
            let (cmd, _) = hkcu.create_subkey(format!(r"{}\command", key_path))?;
            cmd.set_value(
                "",
                &format!(
                    "\"{}\" context-menu restore \"%1\"",
                    cli
                ),
            )?;
        }

        Ok(())
    }

    /// Menu for Folders: Snapshot, Auto-Vault, Browse
    fn register_dir_menu(
        hkcu: &RegKey,
        base_path: &str,
        cli: &str,
        icon: &str,
        path_var: &str,
    ) -> anyhow::Result<()> {
        let shell_path = format!(r"{}\shell", base_path);

        // ── 1. Snapshot Folder ────────────────────────────────────────────
        {
            let key_path = format!(r"{}\01_Snapshot", shell_path);
            let (k, _) = hkcu.create_subkey(&key_path)?;
            k.set_value("", &"Snapshot Folder")?;
            k.set_value("MUIVerb", &"Snapshot Folder")?;
            k.set_value("Icon", &icon)?;
            let (cmd, _) = hkcu.create_subkey(format!(r"{}\command", key_path))?;
            cmd.set_value(
                "",
                &format!(
                    "\"{}\" context-menu snapshot \"{}\"",
                    cli, path_var
                ),
            )?;
        }

        // ── 2. Watch with Auto-Vault ──────────────────────────────────────
        {
            let key_path = format!(r"{}\02_Watch", shell_path);
            let (k, _) = hkcu.create_subkey(&key_path)?;
            k.set_value("", &"Watch with Auto-Vault")?;
            k.set_value("MUIVerb", &"Watch with Auto-Vault")?;
            k.set_value("Icon", &icon)?;
            let (cmd, _) = hkcu.create_subkey(format!(r"{}\command", key_path))?;
            cmd.set_value(
                "",
                &format!(
                    "\"{}\" context-menu watch \"{}\"",
                    cli, path_var
                ),
            )?;
        }

        // ── 3. Browse in OOS-Lite ─────────────────────────────────────────
        {
            let key_path = format!(r"{}\03_Browse", shell_path);
            let (k, _) = hkcu.create_subkey(&key_path)?;
            k.set_value("", &"Browse in OOS-Lite")?;
            k.set_value("MUIVerb", &"Browse in OOS-Lite")?;
            k.set_value("Icon", &icon)?;
            let (cmd, _) = hkcu.create_subkey(format!(r"{}\command", key_path))?;
            cmd.set_value(
                "",
                &format!(
                    "\"{}\" context-menu browse \"{}\"",
                    cli, path_var
                ),
            )?;
        }

        Ok(())
    }

    // ── Context menu command handlers ─────────────────────────────────────────

    /// Try to connect to the running GUI server (port 3000).
    fn gui_is_running() -> bool {
        std::net::TcpStream::connect("127.0.0.1:3000").is_ok()
    }

    /// Ensure the desktop GUI is running; start it if not.
    fn ensure_gui_running() {
        if !gui_is_running() {
            let exe = gui_exe();
            let _ = std::process::Command::new(&exe)
                .arg("--no-open")
                .creation_flags(CREATE_NO_WINDOW)
                .spawn();
            // Wait until port is ready (up to 3 seconds)
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(150));
                if gui_is_running() {
                    break;
                }
            }
        }
    }

    /// Open OOS-Lite window (Edge/Chrome app mode or default system browser).
    fn open_gui_window(query: &str) {
        ensure_gui_running();
        let url = if query.is_empty() {
            "http://127.0.0.1:3000".to_string()
        } else {
            format!("http://127.0.0.1:3000/{}", query)
        };
        crate::ui::open_desktop_window(&url);
    }

    /// Handle `context-menu store-file <path>` — prompts or stores file in vault.
    pub fn handle_store_file(path: &str) {
        let encoded = url_encode(path);
        open_gui_window(&format!("?action=store&target={}", encoded));
    }

    /// Handle `context-menu view-history <path>` — opens Files Explorer filtered to the file.
    pub fn handle_view_history(path: &str) {
        let encoded = url_encode(path);
        open_gui_window(&format!("?action=view-history&target={}", encoded));
    }

    /// Handle `context-menu snapshot <path>` — opens Snapshots tab / folder hub in GUI.
    pub fn handle_snapshot(path: &str) {
        let encoded = url_encode(path);
        open_gui_window(&format!("?action=snapshot&target={}", encoded));
    }

    /// Handle `context-menu restore <path>` — opens Files Explorer → Versions panel for the file.
    pub fn handle_restore(path: &str) {
        let encoded = url_encode(path);
        open_gui_window(&format!("?action=restore&target={}", encoded));
    }

    /// Handle `context-menu watch <path>` — configures watcher for the directory.
    pub fn handle_watch(path: &str) {
        let encoded = url_encode(path);
        open_gui_window(&format!("?action=watch&target={}", encoded));
    }

    /// Handle `context-menu browse <path>` — opens Files Explorer filtered to this directory.
    pub fn handle_browse(path: &str) {
        let encoded = url_encode(path);
        open_gui_window(&format!("?action=browse&target={}", encoded));
    }

    fn url_encode(s: &str) -> String {
        let mut out = String::new();
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(b as char);
                }
                _ => {
                    out.push_str(&format!("%{:02X}", b));
                }
            }
        }
        out
    }
}
