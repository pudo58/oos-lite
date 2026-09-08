## OOS-Lite v0.2.2 - Cloudflare Tunnel Auto-Provisioning, Share UI Overhaul & Windows Shell Fixes

### 🚀 Key Improvements & Highlights

- **Seamless Internet Sharing with Cloudflare Quick Tunnels**:
  - **Zero-Configuration Auto-Download**: OOS-Lite now automatically provisions and runs `cloudflared` directly into the vault's `.oos-store/bin/` on demand. Users no longer need to manually install or configure any external tunneling tools.
  - **Hidden Background Daemon**: Spawns `cloudflared` with `CREATE_NO_WINDOW` on Windows, eliminating flashing command-line terminals when generating public links.
  - **Zombie Process Cleanup**: Proactively terminates orphaned `cloudflared` background instances on startup and share creation to prevent port contention and hanging tunnels.

- **Polished File Share Experience**:
  - **Security & Privacy**: Added password hide/reveal toggle and masked password inputs.
  - **One-Click Clipboard Actions**: Fast copy buttons for LAN address, Internet Public URL, and Passwords with visual checkmarks and instant toast notifications.
  - **Real-Time Polling**: Smart auto-polling UI that gracefully waits for Cloudflare's ephemeral URL assignment and displays the ready link without requiring popup reloads.
  - **Session Guard Warning**: Inlined persistent warnings alerting users to keep OOS-Lite active while sharing files.

- **Windows Shell & Context Menu Hardening**:
  - **Silent Routing**: Shell context menu commands ("Store in Vault", "View History", "Restore", "Snapshot") now route directly through `oos-lite-gui.exe` instead of popping console windows.
  - **Fixed "Select an app" File Association Glitch**: Corrected installer payload to register GUI targets in HKCU without prompting Windows file-open dialogs.
  - **Resilient URL Parsing**: Upgraded Cloudflare output scanner to handle ANSI terminal escape sequences and variable output buffering reliably.

---

### 🐧 Linux Quick Start & Installation

```bash
# 1. Download Linux binary bundle from GitHub Release v0.2.2
wget https://github.com/pudo58/oos-lite/releases/download/v0.2.2/oos-lite-linux-x86_64-v0.2.2.tar.gz

# 2. Extract the archive and set executable permissions
tar -xzf oos-lite-linux-x86_64-v0.2.2.tar.gz
chmod +x oos-lite oos-lite-gui

# 3. Install runtime dependency (for Ubuntu / Debian)
sudo apt update && sudo apt install -y libfuse2

# 4. Check version and view CLI help
./oos-lite --version
./oos-lite --help

# 5. Launch embedded Web UI Dashboard & Diff Studio
./oos-lite ui
```

---

### 📦 Download Assets

- **Windows Installer (Recommended)**: `OOS-Lite-Setup-v0.2.2.exe` (Complete setup wizard with desktop shortcut, context menu integration, and uninstaller)
- **Windows Portable Bundle**: `oos-lite-windows-x86_64-v0.2.2.zip` (Pre-compiled standalone Windows binaries)
- **Linux Standalone Bundle**: `oos-lite-linux-x86_64-v0.2.2.tar.gz` (Pre-compiled native 64-bit Linux binaries with FUSE support)
- **Integrity Checksums**: `SHA256SUMS.txt` (SHA-256 verification hash list)
