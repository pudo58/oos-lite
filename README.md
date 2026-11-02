# OOS-Lite

**English** | [简体中文](docs/i18n/README.zh-CN.md) | [日本語](docs/i18n/README.ja.md) | [Français](docs/i18n/README.fr.md) | [한국어](docs/i18n/README.ko.md) | [Tiếng Việt](docs/i18n/README.vi.md)

[![CI](https://github.com/pudo58/oos-lite/actions/workflows/ci.yml/badge.svg)](https://github.com/pudo58/oos-lite/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/pudo58/oos-lite)](https://github.com/pudo58/oos-lite/releases)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Support OOS-Lite](https://img.shields.io/badge/support-SOL%20%7C%20USDT-087f5b.svg)](https://pudo58.github.io/oos-lite/support/)

OOS-Lite is a local-first, content-addressed file vault written in Rust. It keeps every version of your files, deduplicates them at the chunk level, and takes snapshots without copying data. You can use it from the command line, a desktop app, a web dashboard, or as a read-only virtual drive.

---

## Contents

- [Features](#features)
- [Platform support](#platform-support)
- [Installation](#installation)
- [Quick start](#quick-start)
- [Command reference](#command-reference)
- [Architecture](#architecture)
- [Encryption](#encryption)
- [Web UI and public sharing](#web-ui-and-public-sharing)
- [On-disk layout](#on-disk-layout)
- [Project structure](#project-structure)
- [Development](#development)
- [Contributing](#contributing)
- [License](#license)

---

## Features

- **Chunk-level deduplication.** Files are split with FastCDC (16 KiB min, 64 KiB target, 256 KiB max) and each chunk is identified by its BLAKE3 hash. Editing part of a large file only stores the chunks that changed.
- **Version history.** Every `put` of an existing name adds a new version. You can read, roll back, or prune old versions.
- **Snapshots.** A snapshot records which version of every file was current. It does not copy file data.
- **Crash recovery.** Writes go through a redo-only write-ahead log (WAL) that is fsynced before metadata is updated. On restart, committed operations are replayed and a torn tail in the newest segment is truncated. Recovery is covered by kill-and-restart tests.
- **Compression.** Chunks are compressed with Zstandard when it saves at least 5%. Hashing is always done on the uncompressed bytes, so compression does not affect deduplication.
- **Encryption at rest (optional).** Chunk payloads and WAL records are encrypted with XChaCha20-Poly1305 under a master key protected by Argon2id. See [Encryption](#encryption) for exactly what is and is not covered.
- **Garbage collection.** Mark-and-sweep GC copies live chunks into new segments and swaps them in with a crash-safe rename protocol.
- **Integrity check.** `fsck` verifies segment records, checksums, manifests, and (for encrypted stores) authentication tags.
- **Auto-Vault watcher.** Watches a folder and commits new versions automatically, with debounce, cooldown, throttling, and `.oosignore` support.
- **Virtual drive.** Mount the store read-only as a WebDAV drive on Windows (no kernel driver) or via FUSE on Linux/macOS.
- **Desktop app and Web UI.** A dashboard for browsing files, versions, and snapshots, uploading, and running maintenance. English and Vietnamese UI.

## Platform support

| Platform | Prebuilt release | Mount backend | Tested in CI |
| :--- | :--- | :--- | :--- |
| Windows x86_64 | Installer and portable `.zip` | WebDAV | Yes |
| Linux x86_64 (glibc) | `.tar.gz` | FUSE 3 | Yes |
| macOS | Build from source | FUSE (macFUSE) | No |

Windows binaries are currently unsigned, so SmartScreen may show an "Unknown publisher" warning.

## Installation

### Windows

Download `OOS-Lite-Setup-<version>.exe` from the [releases page](https://github.com/pudo58/oos-lite/releases) and run it. The installer includes the CLI (`oos-lite.exe`) and the desktop app (`oos-lite-gui.exe`), and can optionally add OOS-Lite to your `PATH` and the Explorer context menu.

For a portable setup, extract `oos-lite-windows-x86_64-<version>.zip` and run `.\oos-lite.exe --version`.

### Linux

Install the runtime libraries (Ubuntu/Debian):

```bash
sudo apt update
sudo apt install -y libfuse3-3 libwayland-client0 libxkbcommon0 libdbus-1-3
```

Download, verify, and install for the current user (replace `v1.0.0` with the release you want):

```bash
VERSION=v1.0.0
wget https://github.com/pudo58/oos-lite/releases/download/$VERSION/oos-lite-linux-x86_64-$VERSION.tar.gz
wget https://github.com/pudo58/oos-lite/releases/download/$VERSION/SHA256SUMS.txt
sha256sum --check SHA256SUMS.txt --ignore-missing

mkdir -p "$HOME/.local/share/oos-lite" "$HOME/.local/bin"
tar -xzf oos-lite-linux-x86_64-$VERSION.tar.gz -C "$HOME/.local/share/oos-lite"
ln -sf "$HOME/.local/share/oos-lite/oos-lite" "$HOME/.local/bin/oos-lite"
ln -sf "$HOME/.local/share/oos-lite/oos-lite-gui" "$HOME/.local/bin/oos-lite-gui"
```

Make sure `$HOME/.local/bin` is on your `PATH`.

### From source

Requires a recent stable Rust toolchain and a C linker (MSVC or MinGW on Windows). On Linux, FUSE support also needs `libfuse3-dev` and `pkg-config`.

```bash
git clone https://github.com/pudo58/oos-lite.git
cd oos-lite
cargo build --release -p oos-lite
```

Binaries are written to `target/release/oos-lite` and `target/release/oos-lite-gui` (`.exe` on Windows).

## Quick start

```bash
# Create a store (defaults to ./.oos-store; use --store-dir to change it)
oos-lite init

# Store a file, then store it again after editing it
oos-lite put report.docx
oos-lite put report.docx

# See its versions and get the first one back
oos-lite versions report.docx
oos-lite get report.docx ./report_v1.docx --version 1

# Snapshot everything, then check how much space deduplication saved
oos-lite snapshot create before-cleanup
oos-lite stats
```

## Command reference

Global options: `--store-dir <path>` (alias `--store`, default `.oos-store`), `--password <pass>`, `--password-file <file>`. The passphrase can also be supplied with the `OOS_PASSWORD` environment variable.

### Files and versions

| Command | Description |
| :--- | :--- |
| `oos-lite init` | Initialize a store (encrypted if a passphrase is given) |
| `oos-lite put <path> [--name <name>]` | Store a file, or add a new version of it |
| `oos-lite get <name\|id> <out> [--version N]` | Write a file (latest or a given version) to disk |
| `oos-lite list` | List stored files |
| `oos-lite versions <name\|id>` | Show the version history of a file |
| `oos-lite rollback <name> --version N [--out <path>]` | Make an older version the latest again, optionally writing it to disk |
| `oos-lite rm <name>` | Remove a name from the store (history stays until pruned and collected) |
| `oos-lite prune [--keep N] [name]` | Keep only the latest N versions (default 10) |

### Snapshots

| Command | Description |
| :--- | :--- |
| `oos-lite snapshot create <label>` | Create a snapshot |
| `oos-lite snapshot list` | List snapshots |
| `oos-lite snapshot restore <label> <dir>` | Restore a snapshot into a directory |
| `oos-lite snapshot delete <label>` | Delete a snapshot |

### Maintenance

| Command | Description |
| :--- | :--- |
| `oos-lite stats` | Logical size, physical size, savings ratio, chunk counts |
| `oos-lite gc` | Reclaim space from unreferenced chunks |
| `oos-lite fsck` | Verify data integrity |

### Apps, mounting, and integration

| Command | Description |
| :--- | :--- |
| `oos-lite app` | Launch the desktop app (aliases `desktop`, `gui`) |
| `oos-lite ui [--host 127.0.0.1] [--port 3000] [--no-open]` | Start the Web UI |
| `oos-lite mount [Z: \| /mnt/dir] [--webdav] [--port 8080] [--cache-mb 128]` | Mount the store read-only |
| `oos-lite watch <dir> [--debounce-secs 3] [--cooldown-secs 60] [--throttle-ms 10]` | Auto-version a folder |
| `oos-lite shell-ext enable\|disable\|status` | Windows Explorer context menu |
| `oos-lite stop` | Stop background OOS-Lite processes and free ports 3000/8080 |
| `oos-lite uninstall` | Stop processes and remove installed components |

Run `oos-lite <command> --help` for the full list of options.

The mounted drive has three folders: `current/` (latest version of every file), `snapshots/<label>/`, and `history/<path>/<file>@v<N>` plus `@latest`. See the [mount guide](docs/MOUNT_GUIDE.md) (Vietnamese) for details.

## Architecture

### Write path

```
input file
   │
   ▼
FastCDC chunker (16 / 64 / 256 KiB)
   │
   ▼
ChunkId = BLAKE3(plaintext chunk)
   │
   ├── already stored? ──► reuse the ID, write nothing
   │
   └── new ──► zstd (if it helps) ──► encrypt (if enabled) ──► append to segment
   │
   ▼
manifest (ordered ChunkIds and lengths)
   │
   ▼
WAL record appended and fsynced
   │
   ▼
redb metadata updated: name → ObjectId, ObjectId → versions → manifest
```

### Components

- **Chunk:** an immutable byte range produced by FastCDC, identified by BLAKE3 and checked with CRC32C.
- **Segment store:** append-only files (`segment_00000001.seg`, ...) that roll over at about 256 MiB. Readers use a file-descriptor cache.
- **Manifest:** the ordered list of chunks that make up one version of a file.
- **ObjectId:** a 128-bit hybrid logical ID that stays the same across edits and renames, so history follows the file.
- **Metadata:** stored in [redb](https://crates.io/crates/redb): the name index, object records, manifests, snapshots, and watcher state.
- **WAL:** redo-only log. A record only carries chunk data for chunks that were not already stored, which avoids writing every chunk twice.

### Crash safety

- Metadata is updated only after the WAL record is durable. Recovery replays committed records idempotently and checks that every chunk in a manifest is durable before committing it.
- Recovery truncates only an incomplete tail write in the newest segment. Corruption anywhere else is reported as an error and data is left untouched.
- GC writes compacted segments to a staging directory, renames old segments to `.seg.old`, then moves the new ones in. If a crash interrupts this, startup restores from the `.seg.old` files.

## Encryption

```bash
# Create an encrypted store (passphrase of at least 8 characters)
oos-lite --password-file ~/.oos-password init

# Every later command needs the same passphrase
oos-lite --password-file ~/.oos-password put backup.tar.gz
export OOS_PASSWORD='...'   # alternative
```

Prefer `--password-file` (with `chmod 600`) over `--password`, which ends up in shell history.

How it works: a random 256-bit master key encrypts chunk payloads and WAL records with XChaCha20-Poly1305, using a random nonce for every record. The master key is wrapped in the 100-byte `vault.key` file using a key derived from your passphrase with Argon2id (64 MiB, 3 iterations). An existing unencrypted store cannot be switched to encrypted mode.

**Limitations you should know about:**

- **Metadata is not encrypted.** File names, sizes, timestamps, version history, snapshot labels, and manifests in the redb database are stored in plaintext.
- **Chunk IDs are plain BLAKE3 hashes of the content.** Someone with read access to the store can tell whether a file they already have is stored in it, by chunking it and comparing hashes.
- **Losing the passphrase or `vault.key` means losing the data.** There is no recovery mechanism.

Use encryption to protect file contents on a lost disk or an untrusted backup target, not to hide which files you have.

## Web UI and public sharing

```bash
oos-lite ui --port 3000   # opens http://127.0.0.1:3000
oos-lite app              # same dashboard in a desktop window
```

The dashboard includes a file explorer (list, tree, and grid views), previews, version history, snapshots, uploads (including folders), watcher control, and GC/fsck.

The UI server binds to `127.0.0.1` by default and checks the `Host`, `Origin`, and `Sec-Fetch-Site` headers to block DNS rebinding and cross-site requests. It has **no login**. If you bind it to `0.0.0.0`, anyone on your network can use it.

**Public sharing is on by default.** When the UI starts, OOS-Lite also:

1. starts a share server on port `3001`, listening on all network interfaces;
2. starts a Cloudflare quick tunnel to that port, downloading the latest `cloudflared` binary into the store directory if it is not already there.

Shared links are protected by a random token and an optional expiry. If you do not need sharing, turn it off:

```bash
OOS_DISABLE_PUBLIC_SHARING=1 oos-lite ui        # Linux / macOS
$env:OOS_DISABLE_PUBLIC_SHARING = "1"           # Windows PowerShell, then run oos-lite ui
```

## On-disk layout

```
.oos-store/
├── store.lock              # exclusive process lock
├── vault.key               # only for encrypted stores
├── segments/               # append-only chunk segments
│   └── segment_00000001.seg
├── wal/
│   └── wal.log
├── metadata.db.open.lock
└── metadata.db/
    ├── metadata.redb       # active metadata database
    ├── .redb-active        # migration completed marker (do not remove)
    ├── .redb-migrating     # present while a migration is in progress
    ├── conf                # blocks older sled-based binaries from opening the store
    ├── conf.sled-backup    # original sled settings (migrated stores only)
    └── db, snap.*          # original sled data (migrated stores only)
```

Versions before the redb migration stored metadata in sled. When a newer build opens such a store, it imports the sled data through `metadata.redb.staging`, keeps the original sled files in place, and installs a blocker so older binaries cannot open the migrated store. An interrupted migration resumes automatically. Once `.redb-active` exists, a missing or damaged database is reported as an error rather than silently re-imported from stale sled data.

When backing up a store, copy the whole directory and never delete the state markers.

## Project structure

```
oos-lite/
├── core/                 # storage engine library (oos-lite-core)
│   ├── src/
│   │   ├── engine.rs     # StorageEngine: ties everything together
│   │   ├── chunk/        # FastCDC, ChunkId, chunk store
│   │   ├── segment/      # segment format, writer, reader, store
│   │   ├── manifest/     # manifests
│   │   ├── object/       # ObjectId and version records
│   │   ├── index/        # redb metadata and sled migration
│   │   ├── wal/          # write-ahead log
│   │   ├── snapshot/     # snapshots
│   │   ├── gc/           # garbage collection
│   │   ├── fsck/         # integrity checking
│   │   ├── crypto/       # vault key and chunk encryption
│   │   ├── vfs/          # virtual tree used by mounts
│   │   └── watcher/      # Auto-Vault folder watcher
│   └── tests/            # integration, crash-recovery, encryption, watcher tests
├── cli/                  # oos-lite and oos-lite-gui binaries
│   └── src/
│       ├── main.rs       # CLI (clap)
│       ├── gui_main.rs   # desktop app entry point
│       ├── ui/           # embedded web server and dashboard
│       ├── mount/        # WebDAV and FUSE
│       ├── share.rs      # public sharing and tunnel
│       ├── tray.rs       # Windows system tray
│       └── shell_ext.rs  # Windows Explorer integration
├── benchmarks/           # Criterion benchmarks and comparison runner
├── installer/            # Inno Setup script and icons
└── docs/                 # mount guide, support page, translations
```

## Development

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all
cargo bench --workspace                # Criterion benchmarks
cargo run -p oos-lite-benchmarks       # comparison against plain copies
```

Release history is in [release_notes.md](release_notes.md) and on the [releases page](https://github.com/pudo58/oos-lite/releases).

## Contributing

Contributions are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) before opening an issue or pull request. Security problems should be reported privately, as described there.

If OOS-Lite is useful to you, you can also [support the project](https://pudo58.github.io/oos-lite/support/).

## License

Dual-licensed under either of

- MIT License ([LICENSE-MIT](LICENSE-MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

at your option.
