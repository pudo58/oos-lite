## OOS-Lite v1.0.0 - Reliable Local-First Storage

OOS-Lite 1.0 is the first stable release of the local-first, content-addressed file vault. It combines version history, FastCDC chunk deduplication, snapshots, encryption at rest, live folder synchronization, recovery tooling, a desktop-oriented Web UI, and native Windows/Linux executables.

Existing OOS-Lite stores, segment files, metadata, and WAL records remain compatible. No migration is required when upgrading from 0.2.x.

### Highlights

- Redesigned the dashboard as a compact, light desktop application with list, tree, and grid file views, a file inspector, responsive navigation, search, sorting, previews, version history, snapshots, and maintenance tools.
- Added polished public download and expiration pages for secure shares.
- Added folder sharing with on-demand ZIP creation while preserving single-file downloads.
- Improved watcher status reporting, directory selection, retry visibility, and English/Vietnamese localization. English is now the default UI language.

### Storage reliability

- Recovery now distinguishes an incomplete tail write from a complete but corrupted segment record. Only a torn tail in the newest segment is truncated; CRC, magic, and older-segment corruption return a precise error without modifying data.
- GC rollback preserves segments that were not renamed before a crash and restores only files with a matching `.seg.old` backup.
- GC and FSCK now retain and validate object history after a name is unbound, including history reachable only through `ObjectId` and snapshots.
- Encrypted-store initialization now acquires the exclusive store lock before reading or creating `vault.key`, preventing concurrent processes from generating different master keys.
- Large-file ingestion streams chunks directly into segment storage instead of buffering the full payload in WAL memory.
- WAL recovery verifies that every manifest chunk is durable before committing metadata, rejects values that exceed on-disk integer limits, and remains compatible with legacy WAL records containing chunk data.
- Error messages emitted by the core are standardized in English.

### Watcher correctness

- Reconciliation hashes same-size files with BLAKE3, so offline edits are detected even when file size does not change.
- Scan and metadata errors no longer look like deletions. An incomplete scan enters `Degraded` state and skips destructive reconciliation.
- Ordered rename handling preserves one object history across rename-then-modify and chained rename events.
- Directory rename and deletion update every managed child using path-component boundaries, including moves into or out of ignored paths.
- Watch roots are normalized to absolute paths and checked against store overlap.
- Files locked during startup are retried with bounded backoff until readable, removed, or the watcher stops.

### Windows installation

Download `OOS-Lite-Setup-v1.0.0.exe` and run it. The installer includes the CLI and desktop GUI, and can optionally add OOS-Lite to your user `PATH` and Windows Explorer context menu.

For a portable installation, download and extract `oos-lite-windows-x86_64-v1.0.0.zip`, then run:

```powershell
.\oos-lite.exe --version
.\oos-lite-gui.exe
```

The binaries are currently unsigned, so Microsoft Defender SmartScreen may show an "Unknown publisher" warning.

### Linux installation

The Linux build targets x86_64 glibc systems. On Ubuntu or Debian, install the runtime libraries first:

```bash
sudo apt update
sudo apt install -y libfuse3-3 libwayland-client0 libxkbcommon0 libdbus-1-3
```

Download, verify, and install OOS-Lite for the current user:

```bash
wget https://github.com/pudo58/oos-lite/releases/download/v1.0.0/oos-lite-linux-x86_64-v1.0.0.tar.gz
wget https://github.com/pudo58/oos-lite/releases/download/v1.0.0/SHA256SUMS.txt
sha256sum --check SHA256SUMS.txt --ignore-missing

mkdir -p "$HOME/.local/share/oos-lite" "$HOME/.local/bin"
tar -xzf oos-lite-linux-x86_64-v1.0.0.tar.gz -C "$HOME/.local/share/oos-lite"
ln -sf "$HOME/.local/share/oos-lite/oos-lite" "$HOME/.local/bin/oos-lite"
ln -sf "$HOME/.local/share/oos-lite/oos-lite-gui" "$HOME/.local/bin/oos-lite-gui"
export PATH="$HOME/.local/bin:$PATH"

oos-lite --version
oos-lite --store-dir "$HOME/.oos-store" init
oos-lite-gui
```

To keep the command available after restarting your shell, add this line to `~/.bashrc` or `~/.zshrc`:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

For encrypted initialization, avoid putting a passphrase directly in shell history. Store it in a protected file and use:

```bash
chmod 600 "$HOME/.oos-password"
oos-lite --store-dir "$HOME/.oos-store" --password-file "$HOME/.oos-password" init
```

### Release assets

- `OOS-Lite-Setup-v1.0.0.exe` - Windows installer
- `oos-lite-windows-x86_64-v1.0.0.zip` - portable Windows CLI and GUI
- `oos-lite-linux-x86_64-v1.0.0.tar.gz` - Linux x86_64 CLI and GUI
- `SHA256SUMS.txt` - SHA-256 checksums for all packages
