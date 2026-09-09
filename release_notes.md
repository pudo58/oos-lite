## OOS-Lite v0.2.3 - Core Durability and Recovery Fixes

This patch release fixes several storage-engine failure modes and improves large-file ingestion without changing the public API or on-disk formats.

### Core fixes

- Fixed GC rollback after a crash during a partial multi-segment swap. Original segments that had not yet been renamed are now preserved.
- Fixed a race during encrypted-store initialization by acquiring the exclusive store lock before reading or creating `vault.key`.
- Changed new writes to stream chunks directly into durable segment storage. WAL records now contain metadata and manifests instead of retaining all new file data in memory.
- Added WAL recovery validation so metadata is never committed when a manifest references a missing durable chunk.
- Added checked WAL length conversions for names, manifests, chunk counts, chunk sizes, total payload size, and encryption overhead.
- Fixed watcher reconciliation so same-size content changes are detected with BLAKE3.
- Standardized the remaining core decryption error message in English.

Existing WAL records containing chunk data remain readable. Existing vaults and segment files require no migration.

### Windows installation

Download `OOS-Lite-Setup-v0.2.3.exe` and run the installer. The portable package `oos-lite-windows-x86_64-v0.2.3.zip` is also available for use without installation.

### Linux installation

OOS-Lite requires FUSE 3 for virtual filesystem mounting.

```bash
# Ubuntu / Debian dependencies
sudo apt update
sudo apt install -y libfuse3-3 fuse3

# Download and extract OOS-Lite
wget https://github.com/pudo58/oos-lite/releases/download/v0.2.3/oos-lite-linux-x86_64-v0.2.3.tar.gz
mkdir -p "$HOME/.local/share/oos-lite"
tar -xzf oos-lite-linux-x86_64-v0.2.3.tar.gz -C "$HOME/.local/share/oos-lite"

# Install the CLI and GUI launcher for the current user
mkdir -p "$HOME/.local/bin"
ln -sf "$HOME/.local/share/oos-lite/oos-lite" "$HOME/.local/bin/oos-lite"
ln -sf "$HOME/.local/share/oos-lite/oos-lite-gui" "$HOME/.local/bin/oos-lite-gui"

# Ensure ~/.local/bin is available in the current shell
export PATH="$HOME/.local/bin:$PATH"

# Verify and initialize a store
oos-lite --version
oos-lite --store-dir "$HOME/.oos-store" init
```

To keep `~/.local/bin` on `PATH`, add the following line to `~/.bashrc` or `~/.zshrc`:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

For an encrypted store, initialize with `oos-lite --store-dir "$HOME/.oos-store" --password YOUR_PASSWORD init`.

### Release assets

- `OOS-Lite-Setup-v0.2.3.exe` - Windows installer
- `oos-lite-windows-x86_64-v0.2.3.zip` - Windows portable binaries
- `oos-lite-linux-x86_64-v0.2.3.tar.gz` - Linux x86_64 binaries
- `SHA256SUMS.txt` - SHA-256 checksums for all packages
