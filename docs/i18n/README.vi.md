# OOS-Lite

[English](../../README.md) | [简体中文](README.zh-CN.md) | [日本語](README.ja.md) | [Français](README.fr.md) | [한국어](README.ko.md) | **Tiếng Việt**

[![CI](https://github.com/pudo58/oos-lite/actions/workflows/ci.yml/badge.svg)](https://github.com/pudo58/oos-lite/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/pudo58/oos-lite)](https://github.com/pudo58/oos-lite/releases)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE)
[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Support OOS-Lite](https://img.shields.io/badge/support-SOL%20%7C%20USDT-087f5b.svg)](https://pudo58.github.io/oos-lite/support/)

> Bản dịch này dựa trên [README tiếng Anh](../../README.md). Nếu hai bản khác nhau, hãy theo bản tiếng Anh.

OOS-Lite là kho lưu file cục bộ, đánh địa chỉ theo nội dung, viết bằng Rust. Nó giữ mọi phiên bản của file, khử trùng lặp ở mức chunk và chụp snapshot mà không sao chép dữ liệu. Bạn có thể dùng qua dòng lệnh, ứng dụng desktop, dashboard web hoặc mount thành ổ đĩa ảo chỉ đọc.

---

## Mục lục

- [Tính năng](#tính-năng)
- [Nền tảng hỗ trợ](#nền-tảng-hỗ-trợ)
- [Cài đặt](#cài-đặt)
- [Bắt đầu nhanh](#bắt-đầu-nhanh)
- [Danh sách lệnh](#danh-sách-lệnh)
- [Kiến trúc](#kiến-trúc)
- [Mã hóa](#mã-hóa)
- [Web UI và chia sẻ công khai](#web-ui-và-chia-sẻ-công-khai)
- [Cấu trúc thư mục lưu trữ](#cấu-trúc-thư-mục-lưu-trữ)
- [Cấu trúc mã nguồn](#cấu-trúc-mã-nguồn)
- [Phát triển](#phát-triển)
- [Đóng góp](#đóng-góp)
- [Giấy phép](#giấy-phép)

---

## Tính năng

- **Khử trùng lặp theo chunk.** File được cắt bằng FastCDC (tối thiểu 16 KiB, mục tiêu 64 KiB, tối đa 256 KiB) và mỗi chunk được định danh bằng hash BLAKE3. Sửa một phần của file lớn thì chỉ các chunk thay đổi được lưu thêm.
- **Lịch sử phiên bản.** Mỗi lần `put` một tên đã có sẽ tạo phiên bản mới. Bạn có thể đọc lại, rollback hoặc xóa bớt phiên bản cũ.
- **Snapshot.** Snapshot ghi lại phiên bản hiện tại của từng file, không sao chép dữ liệu file.
- **Khôi phục sau crash.** Mọi thao tác ghi đi qua write-ahead log (WAL) kiểu redo, được fsync trước khi cập nhật metadata. Khi khởi động lại, các thao tác đã commit được chạy lại, và phần ghi dở ở cuối segment mới nhất bị cắt bỏ. Có test kill rồi khởi động lại để kiểm chứng.
- **Nén.** Chunk được nén bằng Zstandard khi tiết kiệm được ít nhất 5%. Hash luôn tính trên dữ liệu chưa nén nên nén không ảnh hưởng đến khử trùng lặp.
- **Mã hóa khi lưu (tùy chọn).** Nội dung chunk và bản ghi WAL được mã hóa bằng XChaCha20-Poly1305, master key được bảo vệ bằng Argon2id. Xem mục [Mã hóa](#mã-hóa) để biết chính xác phần nào được mã hóa.
- **Dọn rác (GC).** GC kiểu mark-and-sweep chép các chunk còn dùng sang segment mới rồi thay thế bằng quy trình đổi tên an toàn khi crash.
- **Kiểm tra toàn vẹn.** `fsck` kiểm tra bản ghi segment, checksum, manifest và (với kho mã hóa) tag xác thực.
- **Auto-Vault.** Theo dõi một thư mục và tự tạo phiên bản mới, có debounce, cooldown, giới hạn I/O và hỗ trợ `.oosignore`.
- **Ổ đĩa ảo.** Mount kho ở chế độ chỉ đọc qua WebDAV trên Windows (không cần driver kernel) hoặc FUSE trên Linux/macOS.
- **Ứng dụng desktop và Web UI.** Dashboard để duyệt file, phiên bản, snapshot, tải lên và chạy bảo trì. Giao diện tiếng Anh và tiếng Việt.

## Nền tảng hỗ trợ

| Nền tảng | Bản phát hành sẵn | Kiểu mount | Test trong CI |
| :--- | :--- | :--- | :--- |
| Windows x86_64 | Bộ cài và `.zip` portable | WebDAV | Có |
| Linux x86_64 (glibc) | `.tar.gz` | FUSE 3 | Có |
| macOS | Tự build từ mã nguồn | FUSE (macFUSE) | Không |

Binary Windows hiện chưa được ký số, nên SmartScreen có thể báo "Unknown publisher".

## Cài đặt

### Windows

Tải `OOS-Lite-Setup-<version>.exe` từ [trang phát hành](https://github.com/pudo58/oos-lite/releases) và chạy. Bộ cài gồm CLI (`oos-lite.exe`) và ứng dụng desktop (`oos-lite-gui.exe`), có thể thêm OOS-Lite vào `PATH` và menu chuột phải của Explorer.

Muốn dùng bản portable thì giải nén `oos-lite-windows-x86_64-<version>.zip` rồi chạy `.\oos-lite.exe --version`.

### Linux

Cài thư viện runtime (Ubuntu/Debian):

```bash
sudo apt update
sudo apt install -y libfuse3-3 libwayland-client0 libxkbcommon0 libdbus-1-3
```

Tải, kiểm tra checksum và cài cho user hiện tại (đổi `v1.0.0` thành phiên bản bạn muốn):

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

Đảm bảo `$HOME/.local/bin` có trong `PATH`.

### Build từ mã nguồn

Cần Rust stable bản gần đây và trình liên kết C (MSVC hoặc MinGW trên Windows). Trên Linux, để có FUSE cần thêm `libfuse3-dev` và `pkg-config`.

```bash
git clone https://github.com/pudo58/oos-lite.git
cd oos-lite
cargo build --release -p oos-lite
```

Binary nằm ở `target/release/oos-lite` và `target/release/oos-lite-gui` (đuôi `.exe` trên Windows).

## Bắt đầu nhanh

```bash
# Tạo kho (mặc định ./.oos-store; đổi bằng --store-dir)
oos-lite init

# Lưu một file, sửa nó rồi lưu lần nữa
oos-lite put report.docx
oos-lite put report.docx

# Xem các phiên bản và lấy lại phiên bản đầu tiên
oos-lite versions report.docx
oos-lite get report.docx ./report_v1.docx --version 1

# Chụp snapshot toàn bộ kho, rồi xem khử trùng lặp tiết kiệm được bao nhiêu
oos-lite snapshot create before-cleanup
oos-lite stats
```

## Danh sách lệnh

Tùy chọn chung: `--store-dir <path>` (bí danh `--store`, mặc định `.oos-store`), `--password <pass>`, `--password-file <file>`. Passphrase cũng có thể truyền qua biến môi trường `OOS_PASSWORD`.

### File và phiên bản

| Lệnh | Mô tả |
| :--- | :--- |
| `oos-lite init` | Khởi tạo kho (mã hóa nếu có passphrase) |
| `oos-lite put <path> [--name <name>]` | Lưu file, hoặc thêm phiên bản mới |
| `oos-lite get <name\|id> <out> [--version N]` | Ghi file (bản mới nhất hoặc bản chỉ định) ra đĩa |
| `oos-lite list` | Liệt kê file đã lưu |
| `oos-lite versions <name\|id>` | Xem lịch sử phiên bản |
| `oos-lite rollback <name> --version N [--out <path>]` | Đưa một phiên bản cũ trở thành bản mới nhất, có thể ghi luôn ra đĩa |
| `oos-lite rm <name>` | Xóa tên khỏi kho (lịch sử còn đến khi prune và gc) |
| `oos-lite prune [--keep N] [name]` | Chỉ giữ N phiên bản mới nhất (mặc định 10) |

### Snapshot

| Lệnh | Mô tả |
| :--- | :--- |
| `oos-lite snapshot create <label>` | Tạo snapshot |
| `oos-lite snapshot list` | Liệt kê snapshot |
| `oos-lite snapshot restore <label> <dir>` | Khôi phục snapshot vào một thư mục |
| `oos-lite snapshot delete <label>` | Xóa snapshot |

### Bảo trì

| Lệnh | Mô tả |
| :--- | :--- |
| `oos-lite stats` | Dung lượng logic, dung lượng thật, tỉ lệ tiết kiệm, số chunk |
| `oos-lite gc` | Thu hồi dung lượng của chunk không còn được tham chiếu |
| `oos-lite fsck` | Kiểm tra toàn vẹn dữ liệu |

### Ứng dụng, mount và tích hợp

| Lệnh | Mô tả |
| :--- | :--- |
| `oos-lite app` | Mở ứng dụng desktop (bí danh `desktop`, `gui`) |
| `oos-lite ui [--host 127.0.0.1] [--port 3000] [--no-open]` | Chạy Web UI |
| `oos-lite mount [Z: \| /mnt/dir] [--webdav] [--port 8080] [--cache-mb 128]` | Mount kho ở chế độ chỉ đọc |
| `oos-lite watch <dir> [--debounce-secs 3] [--cooldown-secs 60] [--throttle-ms 10]` | Tự tạo phiên bản cho một thư mục |
| `oos-lite shell-ext enable\|disable\|status` | Menu chuột phải của Windows Explorer |
| `oos-lite stop` | Dừng các tiến trình OOS-Lite chạy nền, giải phóng cổng 3000/8080 |
| `oos-lite uninstall` | Dừng tiến trình và gỡ các thành phần đã cài |

Chạy `oos-lite <lệnh> --help` để xem đủ tùy chọn.

Ổ đĩa mount có ba thư mục: `current/` (bản mới nhất của mọi file), `snapshots/<label>/`, và `history/<path>/<file>@v<N>` cùng `@latest`. Chi tiết xem [hướng dẫn mount](../MOUNT_GUIDE.md).

## Kiến trúc

### Luồng ghi

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

### Thành phần

- **Chunk:** đoạn byte bất biến do FastCDC cắt ra, định danh bằng BLAKE3 và kiểm tra bằng CRC32C.
- **Segment store:** các file chỉ ghi nối (`segment_00000001.seg`, ...), sang file mới khi đạt khoảng 256 MiB. Phía đọc dùng cache file descriptor.
- **Manifest:** danh sách có thứ tự các chunk tạo nên một phiên bản của file.
- **ObjectId:** ID logic lai 128 bit, giữ nguyên qua các lần sửa và đổi tên, nên lịch sử đi theo file.
- **Metadata:** lưu trong [redb](https://crates.io/crates/redb): chỉ mục tên, bản ghi object, manifest, snapshot và trạng thái watcher.
- **WAL:** log kiểu redo. Bản ghi chỉ mang dữ liệu của chunk chưa có trong kho, nên mỗi chunk không bị ghi hai lần.

### An toàn khi crash

- Metadata chỉ được cập nhật sau khi bản ghi WAL đã bền vững. Quá trình khôi phục chạy lại các bản ghi đã commit theo cách idempotent và kiểm tra mọi chunk trong manifest đã bền vững trước khi commit.
- Khôi phục chỉ cắt phần ghi dở ở cuối segment mới nhất. Hỏng ở chỗ khác được báo lỗi và dữ liệu giữ nguyên.
- GC ghi segment đã nén gọn vào thư mục tạm, đổi tên segment cũ thành `.seg.old`, rồi đưa segment mới vào. Nếu crash giữa chừng, lần khởi động sau sẽ khôi phục từ các file `.seg.old`.

## Mã hóa

```bash
# Tạo kho mã hóa (passphrase tối thiểu 8 ký tự)
oos-lite --password-file ~/.oos-password init

# Mọi lệnh sau đó cần cùng passphrase
oos-lite --password-file ~/.oos-password put backup.tar.gz
export OOS_PASSWORD='...'   # cách khác
```

Nên dùng `--password-file` (kèm `chmod 600`) thay vì `--password`, vì `--password` sẽ nằm lại trong lịch sử shell.

Cách hoạt động: một master key ngẫu nhiên 256 bit mã hóa nội dung chunk và bản ghi WAL bằng XChaCha20-Poly1305, mỗi bản ghi một nonce ngẫu nhiên. Master key được bọc trong file `vault.key` 100 byte, dùng khóa dẫn xuất từ passphrase bằng Argon2id (64 MiB, 3 vòng lặp). Không thể chuyển một kho chưa mã hóa sang chế độ mã hóa.

**Những giới hạn bạn cần biết:**

- **Metadata không được mã hóa.** Tên file, kích thước, thời gian, lịch sử phiên bản, nhãn snapshot và manifest trong database redb đều ở dạng rõ.
- **Chunk ID là hash BLAKE3 thường của nội dung.** Người có quyền đọc kho có thể biết một file họ đang có có nằm trong kho hay không, bằng cách cắt chunk rồi so hash.
- **Mất passphrase hoặc `vault.key` là mất dữ liệu.** Không có cơ chế khôi phục.

Hãy dùng mã hóa để bảo vệ nội dung file khi mất ổ đĩa hoặc khi backup lên nơi không tin cậy, không phải để giấu việc bạn có những file nào.

## Web UI và chia sẻ công khai

```bash
oos-lite ui --port 3000   # mở http://127.0.0.1:3000
oos-lite app              # cùng dashboard trong cửa sổ desktop
```

Dashboard có trình duyệt file (dạng danh sách, cây và lưới), xem trước, lịch sử phiên bản, snapshot, tải lên (cả thư mục), điều khiển watcher, và GC/fsck.

Server UI mặc định bind vào `127.0.0.1` và kiểm tra header `Host`, `Origin`, `Sec-Fetch-Site` để chặn DNS rebinding và request chéo trang. Server **không có đăng nhập**. Nếu bind vào `0.0.0.0`, bất kỳ ai trong mạng của bạn đều dùng được.

**Chia sẻ công khai được bật mặc định.** Khi UI khởi động, OOS-Lite đồng thời:

1. chạy một server chia sẻ ở cổng `3001`, lắng nghe trên mọi network interface;
2. mở một Cloudflare quick tunnel tới cổng đó, tự tải bản `cloudflared` mới nhất vào thư mục kho nếu chưa có.

Link chia sẻ được bảo vệ bằng token ngẫu nhiên và thời hạn tùy chọn. Nếu không cần chia sẻ, hãy tắt:

```bash
OOS_DISABLE_PUBLIC_SHARING=1 oos-lite ui        # Linux / macOS
$env:OOS_DISABLE_PUBLIC_SHARING = "1"           # Windows PowerShell, rồi chạy oos-lite ui
```

## Cấu trúc thư mục lưu trữ

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

Các phiên bản trước khi chuyển sang redb lưu metadata bằng sled. Khi bản mới mở một kho như vậy, nó nhập dữ liệu sled qua `metadata.redb.staging`, giữ nguyên các file sled gốc, và cài một file chặn để binary cũ không mở được kho đã chuyển đổi. Nếu quá trình chuyển đổi bị ngắt, nó tự chạy tiếp. Khi `.redb-active` đã tồn tại, database bị thiếu hoặc hỏng sẽ được báo lỗi chứ không âm thầm nhập lại dữ liệu sled cũ.

Khi backup kho, hãy chép cả thư mục và không xóa các file đánh dấu trạng thái.

## Cấu trúc mã nguồn

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

## Phát triển

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all
cargo bench --workspace                # benchmark Criterion
cargo run -p oos-lite-benchmarks       # so sánh với sao chép thông thường
```

Lịch sử phát hành nằm trong [release_notes.md](../../release_notes.md) và [trang phát hành](https://github.com/pudo58/oos-lite/releases).

## Đóng góp

Mọi đóng góp đều được hoan nghênh. Hãy đọc [CONTRIBUTING.vi.md](CONTRIBUTING.vi.md) trước khi mở issue hoặc pull request. Lỗi bảo mật cần báo riêng theo hướng dẫn trong đó.

Nếu OOS-Lite hữu ích với bạn, bạn có thể [ủng hộ dự án](https://pudo58.github.io/oos-lite/support/).

## Giấy phép

Dự án dùng song song hai giấy phép, bạn chọn một:

- MIT License ([LICENSE-MIT](../../LICENSE-MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
