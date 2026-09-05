# OOS-Lite — Content-Addressed & Deduplicated File Storage Engine

[![Language](https://img.shields.io/badge/language-Rust%202021-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-49%20passed%20%7C%20100%25-brightgreen.svg)]()
[![Engine](https://img.shields.io/badge/storage-Append--Only%20Segments%20%2B%20Sled-success.svg)]()
[![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20Windows%20%7C%20macOS-lightgrey.svg)]()

> **OOS-Lite** is a high-performance, embedded, content-addressed file storage engine written in Rust. It implements core principles of modern distributed storage architectures—immutable chunks, content-defined chunking (FastCDC), BLAKE3 content hashing, multi-versioning, instant zero-copy snapshots, redo-only WAL crash consistency, and memory-bounded garbage collection—packaged as an easy-to-use CLI and pure Rust library with an embedded real-time Web UI dashboard.

---

## Table of Contents

- [1. Overview & Core Value](#1-overview--core-value)
- [2. System Architecture](#2-system-architecture)
  - [2.1 Write Path Dataflow](#21-write-path-dataflow)
  - [2.2 Core Storage Components](#22-core-storage-components)
  - [2.3 Crash Consistency & Safe Compaction](#23-crash-consistency--safe-compaction)
- [3. Embedded Web UI Dashboard](#3-embedded-web-ui-dashboard)
- [4. Command-Line Interface (CLI)](#4-command-line-interface-cli)
- [5. On-Disk Storage Layout](#5-on-disk-storage-layout)
- [6. Workspace & Codebase Structure](#6-workspace--codebase-structure)
- [7. Technology Stack & Dependencies](#7-technology-stack--dependencies)
- [8. Installation & Getting Started](#8-installation--getting-started)
- [9. Development Milestones](#9-development-milestones)
- [10. License](#10-license)

---

## 1. Overview & Core Value

Traditional file backups relying on `cp`, `rsync`, or basic archive utilities suffer from exponential storage growth when saving multiple iterations of large files or directory trees. OOS-Lite solves this by operating at the **sub-file chunk level**:

1. **Content-Defined Chunking (FastCDC):** Files are partitioned dynamically based on data content rather than fixed offsets. Editing a few bytes in the middle of a large file only produces new chunks for the modified region; all unchanged blocks are deduplicated.
2. **Instant Zero-Copy Snapshots:** Point-in-time snapshots capture the logical state of the entire store in sub-millisecond time and consume zero additional disk space for file payloads.
3. **Absolute Crash Consistency:** Utilizing append-only segment storage and a redo-only Write-Ahead Log (WAL) with CRC32C verification, the engine guarantees data integrity even during sudden power outages or `kill -9` process termination.
4. **Streaming Garbage Collection:** The mark-and-sweep compactor streams live chunks directly between segment files without buffering chunk payloads into system memory, preventing Out-Of-Memory (OOM) crashes on multi-gigabyte stores.
5. **User-Space Virtual Filesystem:** Mounts seamlessly in user-space via Windows Native WebDAV (zero kernel drivers required) and Linux/macOS POSIX FUSE.

---

## 2. System Architecture

### 2.1 Write Path Dataflow

```
                     ┌───────────────────────────┐
                     │     Input File Stream     │
                     └─────────────┬─────────────┘
                                   │
                                   ▼
                     ┌───────────────────────────┐
                     │ FastCDC Dynamic Chunker   │
                     │ (Min: 16 KiB, Target: 64) │
                     └─────────────┬─────────────┘
                                   │
                    ┌──────────────┴──────────────┐
                    ▼                             ▼
       ┌────────────────────────┐    ┌────────────────────────┐
       │ Chunk ID: BLAKE3(data) │    │ Checksum: CRC32C(data) │
       └────────────┬───────────┘    └────────────┬───────────┘
                    │                             │
                    ▼                             ▼
       ┌───────────────────────────────────────────────────────┐
       │ Check Existence in Segment Store (Deduplication)      │
       ├──────────────────────────────┬────────────────────────┤
       │ If New: Append to Segment    │ If Exists: Reuse ID    │
       │ (~256 MiB Sequential File)   │ (0 Bytes Disk Written) │
       └────────────┬─────────────────┴────────────────────────┘
                    │
                    ▼
       ┌───────────────────────────────────────────────────────┐
       │ Build Manifest (Ordered ChunkIDs + Lengths + Metas)   │
       └────────────┬──────────────────────────────────────────┘
                    │
                    ▼
       ┌───────────────────────────────────────────────────────┐
       │ Write-Ahead Log (Redo Log Append + fsync)             │
       └────────────┬──────────────────────────────────────────┘
                    │
                    ▼
       ┌───────────────────────────────────────────────────────┐
       │ Update Sled Embedded B-Trees:                         │
       │  • Name Index:   "docs/report.pdf" ──► ObjectID (HLID)│
       │  • Object Index: ObjectID ──► Latest Manifest + Vers  │
       └───────────────────────────────────────────────────────┘
```

### 2.2 Core Storage Components

* **Chunk:** An immutable byte sequence bounded by FastCDC. Identified uniquely by `BLAKE3(bytes)` (256-bit hash) with payload integrity validated via `CRC32C`.
* **Segment Store:** Large, sequential append-only container files (`segment_00000001.seg`) targeting ~256 MiB each. Sequential writes maximize disk throughput, while random reads benefit from an integrated File Descriptor Cache.
* **Manifest:** A version-specific blueprint listing the ordered array of `ChunkID`s that reconstruct the original logical file.
* **ObjectID (128-bit HLID):** A Hybrid Logical Identifier (Node ID / Type Tag / Timestamp / Entropy) providing permanent object identity across file edits, renames, and versions.
* **Decoupled Indexing (Sled B-Tree):**
  * **Name Index:** Maps human-readable logical paths (`"photos/summer.jpg"`) to `ObjectID`. File renames update only this lightweight mapping without modifying physical data chunks or version history.
  * **Object Index:** Tracks version lineage, creation timestamps, and points to the corresponding `Manifest` for each version of an `ObjectID`.
* **Write-Ahead Log (WAL):** Redo-only logging ensuring atomic transactions across crash boundaries. Uncommitted operations are replayed idempotently upon engine restart.

### 2.3 Crash Consistency & Safe Compaction

* **Two-Phase Safe Compaction:** During Garbage Collection, new compacted segments are written to a temporary staging area (`.compact_tmp_<pid>`). Old segments are renamed to `.seg.old` before the new segments take their place. If a crash occurs during compaction, startup recovery restores from `.seg.old` automatically—eliminating any risk of data loss.
* **WAL Write-Amplification Optimization:** The WAL records payloads exclusively for newly introduced chunks. Chunks already residing in the segment store are referenced by ID alone, eliminating 2x write amplification.

---

## 3. Embedded Web UI Dashboard

OOS-Lite includes a built-in, modern, responsive Web Dashboard served directly from the executable with zero external runtime dependencies.

```bash
oos-lite ui --port 3000
```

### Dashboard Features

1. **System Overview (Metrics & Analytics):**
   * Real-time metrics: Logical stored size, Physical disk footprint, Savings Ratio, Total chunks, and Active files.
   * Interactive **Chart.js** data visualization comparing logical vs. physical disk utilization.
2. **File Explorer:**
   * Toggle between responsive **Card Grid View** and detailed **Table View**.
   * Real-time search and filter by file name or extension.
   * Categorized file badge indicators (Images, Source Code, Archives, Documents).
   * **In-Browser File Preview Modal:** Inspect source code, Markdown, JSON, plain text, logs, and images directly without downloading.
3. **Snapshot Center:**
   * Inspect point-in-time snapshots with complete file inventories.
   * One-click snapshot restoration to designated destination folders.
4. **Upload Center:**
   * Interactive drag-and-drop zone supporting single files and **recursive directory folder uploads**.
   * Live upload progress tracking per file.
5. **Maintenance & Storage Health:**
   * Trigger Garbage Collection (Mark-and-Sweep Compaction) with safety confirmation modals.
   * Inspect segment fragmentation and storage diagnostics.

---

## 4. Command-Line Interface (CLI)

### Core File Operations

```bash
# Store a file (creates a new object or appends a new version)
oos-lite put path/to/document.pdf

# Store with an explicit logical name
oos-lite put path/to/archive.tar.gz --name backups/daily.tar.gz

# Retrieve a file by logical name or ObjectID
oos-lite get backups/daily.tar.gz ./restored.tar.gz

# Retrieve a specific historical version
oos-lite get backups/daily.tar.gz ./restored_v1.tar.gz --version 1

# List all stored files with latest version, size, and chunk counts
oos-lite list

# Inspect version history and timestamps for an object
oos-lite versions backups/daily.tar.gz

# Remove a logical file mapping
oos-lite rm backups/daily.tar.gz
```

### Snapshot Management

```bash
# Create an instant zero-copy snapshot
oos-lite snapshot create v1.0.0-release

# List existing snapshots
oos-lite snapshot list

# Restore a snapshot to a target directory
oos-lite snapshot restore v1.0.0-release ./release-export

# Delete a snapshot
oos-lite snapshot delete v1.0.0-release
```

### Maintenance & Diagnostics

```bash
# View storage statistics, deduplication ratio, and chunk metrics
oos-lite stats

# Reclaim space from orphaned chunks (Mark-and-Sweep)
oos-lite gc

# Verify cryptographic and structural integrity across all chunks
oos-lite fsck

# Start the Web UI Dashboard
oos-lite ui --host 127.0.0.1 --port 3000
```

### Virtual Filesystem (FUSE Mount — Linux / macOS / WSL2)

```bash
# Mount the store as a read-only POSIX directory with 128 MiB LRU chunk cache
oos-lite mount /mnt/oos-drive

# Custom chunk cache memory limit (e.g., 256 MiB)
oos-lite mount /mnt/oos-drive --cache-mb 256

# Explore the virtual directory hierarchy:
# /mnt/oos-drive/current/               -> Latest versions of all files
# /mnt/oos-drive/snapshots/<label>/     -> Complete state of snapshots
# /mnt/oos-drive/history/<path>/<file>@v<N> -> All historical versions
# /mnt/oos-drive/history/<path>/<file>@latest -> Virtual symlink to latest version
```

---

## 5. On-Disk Storage Layout

By default, OOS-Lite initializes its storage root in `.oos-store` (customizable via `-s, --store-dir`):

```
.oos-store/
├── segments/               # Append-only chunk storage
│   ├── segment_00000001.seg
│   └── segment_00000002.seg
├── wal/                    # Redo Write-Ahead Log
│   └── wal.log
├── sled/                   # Embedded B-Tree metadata database
│   ├── conf
│   ├── db
│   └── snap.*
└── snapshots/              # Snapshot manifests
    ├── v1.0.0.meta
    └── v1.0.0.data
```

---

## 6. Workspace & Codebase Structure

The project is structured as a modular Cargo workspace:

```
oos-lite/
├── Cargo.toml                  # Workspace manifest
├── LICENSE                     # Dual license terms (MIT / Apache-2.0)
├── LICENSE-MIT                 # MIT License
├── LICENSE-APACHE              # Apache License 2.0
├── README.md                   # Project documentation
├── core/                       # Storage Engine Core Library
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs              # Public library API & engine exports
│       ├── error.rs            # Typed error definitions (thiserror)
│       ├── engine.rs           # Orchestrator (StorageEngine coordination)
│       ├── chunk/              # BLAKE3 identity, CRC32C, FastCDC chunker
│       ├── segment/            # Sequential segment store, writer, cached reader
│       ├── manifest/           # Manifest records & chunk sequences
│       ├── object/             # 128-bit HLID & Object version records
│       ├── index/              # Sled B-Tree integration (Name & Object indices)
│       ├── wal/                # Redo-only Write-Ahead Log implementation
│       ├── gc/                 # Safe mark-and-sweep compaction
│       └── snapshot/           # Zero-copy snapshot engine
├── cli/                        # Command-Line Application & Web Server
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs             # Clap CLI entrypoint
│       └── ui/
│           ├── server.rs       # Embedded HTTP server (tiny_http)
│           └── index.html      # Responsive Single-Page Web Dashboard
└── benchmarks/                 # Criterion Performance Benchmarks
    ├── Cargo.toml
    ├── benches/                # Criterion benchmark suites
    └── src/
        └── main.rs             # CLI benchmark comparison runner
```

---

## 7. Technology Stack & Dependencies

| Component | Library / Crate | Purpose |
| :--- | :--- | :--- |
| **Content Addressing** | [`blake3`](https://crates.io/crates/blake3) | Cryptographic 256-bit chunk identification at hardware speeds |
| **Data Integrity** | [`crc32fast`](https://crates.io/crates/crc32fast) | SIMD-accelerated physical block verification |
| **CDC Chunking** | [`fastcdc`](https://crates.io/crates/fastcdc) | Fast Content-Defined Chunking with dynamic cut points |
| **Metadata Engine** | [`sled`](https://crates.io/crates/sled) | Embedded lock-free B-Tree database for names and manifests |
| **CLI Framework** | [`clap`](https://crates.io/crates/clap) | Declarative CLI argument parsing |
| **Embedded Web** | [`tiny_http`](https://crates.io/crates/tiny_http) | Lightweight, non-async embedded HTTP server |
| **Diagnostics** | [`tracing`](https://crates.io/crates/tracing) | High-performance structured logging |
| **Error Handling** | [`thiserror`](https://crates.io/crates/thiserror) / [`anyhow`](https://crates.io/crates/anyhow) | Idiomatic Rust error hierarchies |
| **Compression** | [`zstd`](https://crates.io/crates/zstd) | High-speed Zstandard chunk compression with adaptive threshold |
| **FUSE Driver** | [`fuser`](https://crates.io/crates/fuser) | Userspace POSIX filesystem driver (Linux / macOS / WSL2) |
| **Benchmarking** | [`criterion`](https://crates.io/crates/criterion) | Microbenchmarking and statistical regression analysis |

---

## 8. Installation & Getting Started

### Prerequisites

* **Rust Toolchain:** Version 1.75+ (Tested on Rust `1.98.1` stable).
* **C/C++ Linker:** MSVC or GCC / MinGW (`w64devkit` on Windows).

### Building from Source

```bash
# Clone repository
git clone https://github.com/pudo58/oos-lite.git
cd oos-lite

# Build debug workspace
cargo build --workspace

# Build optimized release binary
cargo build --release -p oos-lite
```

The resulting binary will be available at `./target/release/oos-lite` (or `./target/release/oos-lite.exe` on Windows).

### Running Tests

```bash
# Run all unit, integration, crash-recovery, and E2E tests (49 tests)
cargo test --workspace
```

### Running Benchmarks

```bash
# Run Criterion benchmarks
cargo bench --workspace

# Or execute the comparative benchmark runner
cargo run -p oos-lite-benchmarks
```

---

## 9. Development Milestones

All core milestones from the OOS-Lite engineering specification have been successfully implemented and verified:

- [x] **Milestone 0 — Bootstrap:** Workspace setup, central error types (`OosLiteError`), tracing integration, and testing harness.
- [x] **Milestone 1 — Chunk Engine:** Content-addressed chunking, BLAKE3 identification, CRC32C checksums, and storage deduplication.
- [x] **Milestone 2 — Segment Store:** 256 MiB append-only segments, file rotation, corrupted record detection, and File Descriptor Caching.
- [x] **Milestone 3 — FastCDC Integration:** Dynamic sub-file boundaries, byte-for-byte fidelity across engine restarts.
- [x] **Milestone 4 — Manifest & Index Trees:** Versioned manifest tracking, Sled Name Index and Object Index workflows.
- [x] **Milestone 5 — WAL & Crash Consistency:** Redo-only WAL write path, zero-write-amplification filtering, and multi-point kill recovery.
- [x] **Milestone 6 — Versioning & Zero-Copy Snapshots:** Sub-millisecond snapshot generation, version lineage queries, and full directory tree restoration.
- [x] **Milestone 7 — Mark-and-Sweep GC:** Streaming two-phase safe compaction with zero OOM risk and constant $O(1)$ memory usage.
- [x] **Milestone 8 — CLI & Diagnostics:** Complete `clap` interface, storage metrics (`stats`), integrity auditing (`fsck`), and Path Traversal sanitization.
- [x] **Milestone 9 — Real-World Benchmarks:** Comparative benchmark runner measuring deduplication savings and latency against traditional copy mechanisms.
- [x] **Milestone 10 — Embedded Web UI Dashboard:** Responsive single-page application with real-time metrics, interactive File Explorer, in-browser previews, and drag-and-drop uploads.
- [x] **Milestone 11 — Transparent Chunk Compression (Zstd):** Conditional per-chunk compression (<95% threshold) with physical CRC32C validation, keeping BLAKE3 hashing strictly on raw uncompressed bytes to preserve 100% deduplication.
- [x] **Milestone 12 — Read-Only FUSE Virtual Filesystem (Milestone 2A):** Zero-copy POSIX mounting exposing `/current`, `/snapshots`, and `/history` with virtual symlinks, on-demand FastCDC chunk reassembly, LRU bounded memory cache, and strict read-only syscall protections.
- [x] **Milestone 13 — Native Windows WebDAV Drive Mount (Milestone 2B):** Zero-driver Windows mounting via standard WebDAV (`\\127.0.0.1@<port>\DavWWWRoot`), Fake `LOCK`/`UNLOCK` Windows Explorer handshake, multi-threaded request processing, dynamic registry inspection for file size caps, and automated `Ctrl+C` drive unmapping.

---

## 10. License

This project is dual-licensed under either:

* **MIT License** ([LICENSE-MIT](LICENSE-MIT) or [http://opensource.org/licenses/MIT](http://opensource.org/licenses/MIT))
* **Apache License, Version 2.0** ([LICENSE-APACHE](LICENSE-APACHE) or [http://www.apache.org/licenses/LICENSE-2.0](http://www.apache.org/licenses/LICENSE-2.0))

at your option.
