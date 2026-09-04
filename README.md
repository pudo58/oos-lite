# OOS-Lite — Content-Addressed & Deduplicated File Storage Engine

[![Language](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![Engine](https://img.shields.io/badge/storage-Append--Only%20Segments%20%2B%20Sled-success.svg)]()

> **OOS-Lite** là ứng dụng và thư viện lưu trữ file cục bộ viết bằng Rust, hiện thực hoá các nguyên lý storage engine từ kiến trúc OOS (Object / Manifest / Chunk / Segment, immutable chunks, content deduplication, multi-versioning, zero-copy snapshots, redo-only WAL, crash consistency).

---

## 1. Tổng quan & Mục tiêu sản phẩm

OOS-Lite được thiết kế để giải quyết bài toán backup và lưu trữ phiên bản file cá nhân (thay thế cho `cp`, `rsync` trong các kịch bản sao lưu cục bộ nhiều phiên bản):

1. **Deduplication cấp độ Chunk (FastCDC):** Lưu lại nhiều phiên bản của cùng một tập tin (hoặc các tập tin có nội dung tương đồng) mà **không làm tốn dung lượng theo cấp số nhân**. Chỉ những khối dữ liệu bị thay đổi mới ghi thêm vào đĩa.
2. **Snapshot gần như tức thì ($O(1)$):** Chụp trạng thái toàn bộ store tại một thời điểm tức thời thông qua tham chiếu cây chỉ mục, hoàn toàn không copy dữ liệu vật lý.
3. **Crash Consistency tuyệt đối:** Áp dụng Write-Ahead Logging (WAL redo-only) và append-only segments. Quy trình ghi được đảm bảo an toàn ngay cả khi tiến trình bị dừng đột ngột (`kill -9` hoặc mất điện).
4. **Không phụ thuộc nhân OS:** Đóng gói trọn vẹn trong môi trường user-space (CLI + core library), độc lập, sẵn sàng nhúng và chạy trên Linux/Windows/macOS.

---

## 2. Kiến trúc Storage Engine

### 2.1 Luồng dữ liệu ghi (Write Path)

```
File Input
    │
    ▼
FastCDC Chunker (Target: 64 KiB, Min: 16 KiB, Max: 256 KiB)
    │
    ▼
Chunk ID = BLAKE3(bytes)  ───► Checksum = CRC32C(bytes)
    │
    ▼
Chunk Store ──(Kiểm tra dedup)──► Nếu chưa có: Ghi Append-Only vào Segment (~256 MiB)
    │
    ▼
Manifest (Danh sách thứ tự ChunkID cấu thành file logic + version + metadata)
    │
    ▼
Object Record (ObjectID 128-bit HLID, Version, ManifestRef)
    │
    ▼
Name Index (Sled Tree: path string -> ObjectID) & Object Index (ObjectID -> Latest Manifest)
```

### 2.2 Các thành phần cốt lõi

- **Chunk:** Khối dữ liệu bất biến (immutable). Định danh duy nhất bằng hash `BLAKE3(bytes)` và kiểm tra tính toàn vẹn bằng `CRC32C`.
- **Segment Store:** Các file segment ghi tuần tự append-only với kích thước mục tiêu ~256 MiB mỗi segment, hỗ trợ sequential write và fast random read.
- **Manifest:** Mô tả một phiên bản cụ thể của file thông qua mảng các `ChunkID` liên tục.
- **Name Index vs. Object Index:**
  - `ObjectID`: 128-bit Hybrid Logical ID (NodeID / TypeTag / Timestamp / Random), duy trì định danh bền vững của file qua các lần chỉnh sửa nội dung.
  - `Name Index`: Ánh xạ đường dẫn (`"backup/photo.jpg"`) → `ObjectID`. Giúp thao tác đổi tên file không cần sửa đổi dữ liệu vật lý hay ObjectID.
  - `Object Index`: B-Tree (Sled) quản lý lịch sử version và trỏ tới Manifest mới nhất của từng `ObjectID`.
- **WAL (Write-Ahead Log):** Đảm bảo tính nguyên tử (atomic) của chuỗi thao tác ghi: `WAL append + fsync` → ghi chunk → cập nhật metadata index → checkpoint.

---

## 3. Giao diện dòng lệnh (CLI Interface)

OOS-Lite cung cấp bộ công cụ CLI đơn giản và trực quan:

```bash
# Lưu file vào store (tạo file mới hoặc đẩy version mới)
oos-lite put <path>

# Trích xuất nội dung file ra đường dẫn chỉ định
oos-lite get <name|id> <out_path>

# Liệt kê các file đang lưu trữ trong store
oos-lite list

# Xem lịch sử các phiên bản (version history) của một file
oos-lite versions <name|id>

# Tạo snapshot trạng thái toàn bộ store
oos-lite snapshot create <label>

# Liệt kê các snapshot đã tạo
oos-lite snapshot list

# Khôi phục toàn bộ store từ snapshot ra thư mục
oos-lite snapshot restore <label> <dest_dir>

# Thống kê dung lượng logic, dung lượng vật lý, tỉ lệ dedup, số chunk...
oos-lite stats

# Dọn dẹp (Mark-and-Sweep) các chunk mồ côi không còn được tham chiếu
oos-lite gc

# Kiểm tra tính toàn vẹn dữ liệu (Integrity Check)
oos-lite fsck
```

---

## 4. Cấu trúc Workspace & Mã nguồn

```
oos-lite/
├── Cargo.toml                  # Cargo workspace cấu hình chung
├── README.md                   # Tài liệu hướng dẫn sử dụng & thiết kế
├── OOS-Lite-File-Storage-App-Prompt.md # Đặc tả kiến trúc & quy chuẩn kỹ thuật
├── core/                       # Core library (logic lưu trữ, độc lập với CLI)
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── error.rs            # Định nghĩa lỗi tập trung (thiserror)
│       ├── chunk/              # Chunk engine, BLAKE3, CRC32C, FastCDC
│       ├── segment/            # Append-only segment file writer/reader
│       ├── manifest/           # Manifest cấu trúc chunk sequence
│       ├── object/             # ObjectID 128-bit & Object records
│       ├── index/              # Sled database: Name Index, Object Index
│       ├── wal/                # Redo-only Write-Ahead Log
│       ├── gc/                 # Mark-and-sweep garbage collection
│       └── snapshot/           # Zero-copy snapshot management
├── cli/                        # CLI binary (tương tác người dùng qua clap)
│   ├── Cargo.toml
│   └── src/
│       └── main.rs
└── benchmarks/                 # Bộ kiểm thử hiệu năng (Criterion)
    ├── Cargo.toml
    └── benches/
```

---

## 5. Danh mục công nghệ & Crate Dependencies

Các dependency cốt lõi được ghim chặt chẽ theo đặc tả kỹ thuật:

| Thành phần | Crate | Vai trò |
|---|---|---|
| **Content Hashing** | [`blake3`](https://crates.io/crates/blake3) | Sinh định danh băm 256-bit cho từng Chunk |
| **Checksum** | [`crc32fast`](https://crates.io/crates/crc32fast) | Kiểm tra tính toàn vẹn khối dữ liệu vật lý |
| **Content-Defined Chunking** | [`fastcdc`](https://crates.io/crates/fastcdc) | Cắt nhỏ luồng dữ liệu theo ranh giới động tối ưu |
| **Index & Metadata DB** | [`sled`](https://crates.io/crates/sled) | Embedded KV database cho Name/Object/Manifest trees |
| **Error Handling** | [`thiserror`](https://crates.io/crates/thiserror), [`anyhow`](https://crates.io/crates/anyhow) | Typed error cho `core`, linh hoạt cho `cli` |
| **Structured Logging** | [`tracing`](https://crates.io/crates/tracing) | Structured logging & diagnostics |
| **CLI Parser** | [`clap`](https://crates.io/crates/clap) | Phân tích cú pháp dòng lệnh (derive API) |
| **Benchmarking** | [`criterion`](https://crates.io/crates/criterion) | Đo lường hiệu năng, so sánh tỉ lệ dedup & độ trễ |

---

## 6. Lộ trình phát triển (Milestones)

- [ ] **Milestone 0 — Bootstrap:** Khởi tạo Cargo workspace (`core`, `cli`, `benchmarks`), error types (`OosLiteError`), tracing, test harness.
- [ ] **Milestone 1 — Chunk Engine:** `put_chunk`, `get_chunk`, `has_chunk`, BLAKE3 identity, CRC32C checksum, kiểm tra dedup vật lý.
- [ ] **Milestone 2 — Segment Store:** Quản lý segment append-only 256 MiB, segment rotation, phát hiện bản ghi hỏng, phục hồi sau `kill -9`.
- [ ] **Milestone 3 — FastCDC & End-to-End Integration:** Tích hợp CDC chunking, kiểm thử end-to-end chu trình ghi → đọc lại byte-for-byte sau khi restart.
- [ ] **Milestone 4 — Manifest & Sled Index Trees:** Quản lý manifest theo phiên bản, Name Index và Object Index trên `sled`.
- [ ] **Milestone 5 — WAL & Crash Consistency:** Hoàn thiện write-path có bảo vệ bằng WAL redo-only và kiểm thử phục hồi lỗi tại nhiều điểm ghi.
- [ ] **Milestone 6 — Versioning & Zero-Copy Snapshot:** Truy vấn lịch sử file, tạo snapshot trong thời gian < 10ms, khôi phục snapshot.
- [ ] **Milestone 7 — Mark-and-Sweep GC:** Dọn dẹp chunk mồ côi dựa trên root-set (các snapshot còn sống + các version hợp lệ).
- [ ] **Milestone 8 — Hoàn thiện CLI:** Đóng gói toàn bộ lệnh qua CLI clap, báo cáo `stats` chi tiết.
- [ ] **Milestone 9 — Benchmark thực tế:** Đo lường và đối sánh định lượng với `cp` và `rsync --link-dest`.

---

## 7. Hướng dẫn cài đặt & Kiểm thử

### Yêu cầu môi trường
- **Rust Toolchain:** Stable (phiên bản >= 1.75 khuyên dùng)
- **Cargo**

### Biên dịch dự án
```bash
cargo build --workspace
```

### Chạy kiểm thử tự động
```bash
cargo test --workspace
```

### Chạy kiểm thử hiệu năng (Benchmarks)
```bash
cargo bench --workspace
```
