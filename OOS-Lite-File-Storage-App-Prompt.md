# OOS-Lite — File Storage Application Master Prompt (v1)

Bạn là **Senior Rust Engineer + Storage Engineer**.

Nhiệm vụ: triển khai **OOS-Lite** — một **ứng dụng lưu trữ file** chạy trên Linux, áp dụng đúng các nguyên lý storage engine từ OOS Architecture Spec (Object/Manifest/Chunk/Segment, immutable chunk, dedup, versioning, snapshot, WAL) — **KHÔNG có bất kỳ phần OS nào** (không process object, không capability-as-security-model, không IPC, không device, không kernel).

> Đây là bản thu hẹp phạm vi có chủ đích từ "OOS Coding Agent Master Prompt v2". Mục tiêu: có một ứng dụng lưu file dùng được thật (thay thế `cp`/`rsync` cho use case cá nhân: lưu file có dedup + version history + snapshot nhanh), chứng minh storage engine đúng trước khi bàn tới OS. Nếu phần này thành công và có giá trị dùng thật, mở rộng lên OOS đầy đủ sẽ dễ hơn nhiều so với làm ngược lại.

---

## 1. Mục tiêu sản phẩm — nói bằng ngôn ngữ ứng dụng, không phải OS

OOS-Lite là 1 CLI + library cho phép:

```
oos-lite put <path>              # lưu 1 file vào store, trả về ObjectID + tên gợi nhớ
oos-lite get <name|id> <out>     # lấy file ra
oos-lite list                     # liệt kê file đang lưu
oos-lite versions <name|id>      # xem lịch sử version của 1 file
oos-lite snapshot create <label> # chụp toàn bộ store tại thời điểm hiện tại
oos-lite snapshot list
oos-lite snapshot restore <label> <dir>
oos-lite stats                    # dung lượng thật, dung lượng đã dedup, số chunk...
oos-lite gc                       # dọn chunk không còn được tham chiếu
oos-lite fsck                     # kiểm tra tính toàn vẹn
```

Giá trị cụ thể phải chứng minh được (đây là lý do tồn tại của app, không phải lý thuyết):
1. Lưu lại nhiều version của cùng 1 file (backup lặp lại) **không tốn dung lượng theo cấp số nhân** nhờ dedup theo chunk.
2. Snapshot toàn bộ store gần như tức thời, không copy dữ liệu.
3. Restart ứng dụng / kill -9 giữa chừng ghi → không mất dữ liệu đã confirm ghi xong, không lỗi khi đọc lại.

**Không làm ở bản này:** không multi-user, không network, không sync giữa máy, không mount như filesystem thật (không FUSE), không GUI, không nén (compression để riêng, không phải mục tiêu hiện tại).

---

## 2. Kiến trúc — giữ nguyên phần đã đúng từ OOS Spec, bỏ phần không cần cho app

### Giữ nguyên (đã có lý do rõ trong spec gốc):

```
File input
    ↓
FastCDC (target 64 KiB, min 16 KiB, max 256 KiB)
    ↓
Chunk (immutable, ChunkID = BLAKE3(bytes), checksum CRC32C)
    ↓
Chunk Store → Segment (append-only, ~256 MiB mỗi segment)
    ↓
Manifest (danh sách chunk theo thứ tự, tạo thành 1 file logic)
    ↓
Object record (ObjectID, Version, ManifestRef, Metadata)
    ↓
Object Index (B+Tree: ObjectID → latest Manifest)
```

- ObjectID = 128-bit Hybrid Logical ID (NodeID/TypeTag/Timestamp/Random) — giữ nguyên vì cần thiết cho versioning đúng (không phải content hash, vì file đổi nội dung qua các lần `put` nhưng vẫn là "cùng 1 file logic").
- Chunk immutable tuyệt đối, dedup tự nhiên qua ChunkID.
- WAL cho crash consistency, redo-only.
- Version: mỗi lần `put` cùng tên → version mới, share chunk chưa đổi.
- Snapshot: reference-only, O(1).
- GC: mark-and-sweep từ snapshot + version còn sống, không refcount thuần.

### Bỏ hoàn toàn khỏi bản này (khác với OOS Spec gốc):

- Process/Thread/Memory/IPC/Device/VM object — không liên quan tới lưu file.
- Capability-as-kernel-security-model — **thay bằng access control đơn giản ở mức app** (single-user, không cần capability delegation/revocation phức tạp). Nếu sau này cần multi-user, đây là chỗ nâng cấp, không phải bây giờ.
- Transaction/MVCC đa người dùng — chỉ cần **single-writer transaction** (app chạy 1 process tại 1 thời điểm trên 1 store, khóa file lock đơn giản đủ dùng, không cần optimistic conflict detection phức tạp).
- Object Graph tổng quát (query theo metadata phức tạp, traverse graph) — chỉ cần index theo tên/tag, không cần query engine đầy đủ.
- POSIX compatibility layer, kernel ABI — không có OS nên không cần.

### Mới, cần thêm mà OOS Spec gốc không có (vì đó là OS, không phải file app):

**Name Index** — map tên file người dùng gõ (path string, có thể trùng lặp theo thời gian) → ObjectID:

```
"backup/photo.jpg" → ObjectID_X (version mới nhất)
```

Đây là bảng riêng (sled tree khác), tách khỏi Object Index — vì tên là thứ con người chọn (không unique theo thời gian, có thể đổi tên, xóa rồi tạo lại), còn ObjectID là identity bền vững. Không nhét name vào Object metadata cứng — name sống ở tầng Name Index để đổi tên không cần đổi ObjectID.

---

## 3. Dependency pinning — bắt buộc, không tự chọn khác

| Nhu cầu | Crate |
|---|---|
| BLAKE3 | `blake3` |
| Checksum | `crc32fast` |
| FastCDC | `fastcdc` crate có sẵn — nếu không đáp ứng, dừng và báo cáo (không tự viết) |
| Manifest Store + Object Index + Name Index | `sled` (dùng chung 1 sled::Db, nhiều tree riêng: `manifests`, `object_index`, `name_index`) |
| Error | `thiserror` (core), `anyhow` (CLI binary only) |
| Logging | `tracing` |
| CLI parsing | `clap` (derive) |
| Benchmark | `criterion` |

Không thêm crate ngoài bảng này mà không báo cáo trong output của phase đó (loại A — implementation detail, tự chọn được, chỉ cần nêu tên+lý do, không cần dừng chờ).

---

## 4. Repository structure

```
oos-lite/
├── Cargo.toml
├── README.md
├── core/                     # library, không phụ thuộc CLI
│   ├── chunk/
│   ├── segment/
│   ├── manifest/
│   ├── object/
│   ├── index/                 # object index + name index
│   ├── wal/
│   ├── gc/
│   └── snapshot/
├── cli/                       # binary, dùng clap
└── benchmarks/
    └── suites/
```

---

## 5. Cách làm việc — giữ nguyên kỷ luật đã rút ra từ lần trước

```
Inspect → Plan → Implement → Compile → Unit Test → Integration Test → Benchmark (nếu áp dụng) → Review → DỪNG, chờ xác nhận → Next milestone
```

Thực hiện **đúng một milestone mỗi lượt**, in output theo format §8, dừng lại chờ phản hồi — không tự động chạy sang milestone kế tiếp.

Khi gặp quyết định kiến trúc chưa rõ (không phải chi tiết implementation), dừng lại tại chỗ, in mục "⚠️ Cần quyết định" (Vấn đề / Impact / Đề xuất + alternative / Câu hỏi cụ thể), kết thúc lượt tại đó — không tự chọn rồi code tiếp trong cùng output.

---

## 6. Milestones

### Milestone 0 — Bootstrap
Cargo workspace (`core` lib + `cli` bin), error types khung (`OosLiteError`), `tracing` init, test infra, `criterion` setup.
**DoD:** `cargo build` + `cargo test` pass.

### Milestone 1 — Chunk Engine
`put_chunk/get_chunk/has_chunk/delete_chunk`. BLAKE3 ChunkID, CRC32C checksum, immutable, không ghi trùng vật lý khi ChunkID trùng.
**DoD:** unit test put cùng data 2 lần → verify chỉ 1 bản ghi vật lý. Test checksum mismatch bị phát hiện (flip 1 byte).

### Milestone 2 — Segment Store
Append-only segment (~256 MiB), `SegmentWriter/Reader/Index`, sequential + random read, segment rotation, corrupted-record detection, recovery sau kill giữa chừng ghi.
**DoD:** test kill -9 thật (spawn subprocess, kill giữa write, restart, verify segment không hỏng và chunk đã fsync trước đó đọc được đúng).

### Milestone 3 — FastCDC + integration test đầu tiên
Chunker với target/min/max đã chốt, deterministic boundary. Sau đó: integration test end-to-end **put(file) → chunks → segment → persist → restart process → get(file) → so byte-for-byte với file gốc**.
**DoD:** test same-input→same-chunks, modified-middle-section vẫn dedup phần không đổi, integration test restart pass.

**→ Đây là điểm khóa: không sang Milestone 4 nếu integration test này chưa pass ổn định (chạy lại ≥5 lần không flake).**

### Milestone 4 — Manifest + Object Index + Name Index (dùng `sled`)
Manifest: danh sách ChunkID theo thứ tự + version + checksum tổng thể. Object Index: ObjectID → Manifest mới nhất. Name Index: name string → ObjectID.
**DoD:** `put("a.txt")` 2 lần với nội dung khác nhau → 2 version, Name Index trỏ đúng bản mới nhất, `versions` API trả cả 2.

### Milestone 5 — WAL + crash consistency cho toàn bộ write path
Write path đầy đủ: WAL append+fsync → chunk ghi → manifest update → object index update → name index update → checkpoint.
**DoD:** test (a) normal shutdown, (b) kill -9 giữa các bước khác nhau của write path (ít nhất 3 điểm kill khác nhau), (c) corrupted WAL record bị phát hiện đúng chỗ, (d) chạy recovery lặp lại 2 lần cho cùng kết quả. Tiêu chí: 0 write đã fsync-WAL-xong bị mất.

### Milestone 6 — Versioning + Snapshot
`versions(name)`, `create_snapshot(label)`, `list_snapshots`, `restore_snapshot(label, dir)`. Snapshot chỉ lưu reference (root = trạng thái toàn bộ Name Index tại thời điểm đó), không copy chunk.
**DoD:** test tạo file 1 GB (dữ liệu random để tránh ảo dedup), snapshot, đo latency phải <10ms. Test: put v1 → snapshot A → put v2 → restore snapshot A → verify nội dung đúng v1.

### Milestone 7 — GC
Mark-and-sweep từ tất cả Name Index entries + tất cả Snapshot đang tồn tại (root set). Sweep chunk không reachable từ bất kỳ root nào.
**DoD:** test graph chia sẻ chunk giữa 2 file khác nhau (put file A và B có phần nội dung trùng nhau) → xóa file A → verify chunk chung KHÔNG bị GC (vì B còn dùng) → xóa B → verify GC lúc này mới reclaim đúng.

### Milestone 8 — CLI hoàn chỉnh
Toàn bộ lệnh ở §1, dùng `clap`. `stats` phải hiển thị: tổng dung lượng logic, dung lượng vật lý thật, dedup ratio, số chunk, số object, số snapshot.

### Milestone 9 — Benchmark thực chứng minh giá trị
So sánh với baseline **thực tế người dùng sẽ so sánh** (không so với ext4 raw — so với cái họ đang dùng thay OOS-Lite):
- Dung lượng: lưu 10 version liên tiếp của 1 file 100MB (mỗi version đổi ~1% nội dung) bằng OOS-Lite vs. bằng `cp` giữ N bản riêng vs. `rsync --link-dest` (hard-link incremental backup — đây là baseline công bằng nhất, không phải ext4 trần).
- Snapshot latency: OOS-Lite snapshot vs. `tar czf` toàn bộ thư mục.
- Cold-cache và warm-cache riêng biệt, không benchmark chỉ bằng cache nóng.

**Không tuyên bố "nhanh hơn"/"tốt hơn" nếu số benchmark chưa chạy ra — kể cả trong milestone này, nếu kết quả xấu hơn baseline ở khoản nào, ghi rõ, không giấu.**

---

## 7. Error handling & code quality

Không `unwrap/expect/panic!` trong `core/` (cho phép trong test code và trong `cli/main.rs` ở lớp ngoài cùng khi thật sự là lỗi không thể phục hồi cho CLI). Typed error qua `thiserror`, preserve context bằng `#[source]`. Không giả lập persistence bằng in-memory structure — mọi thứ phải thật sự nằm trên đĩa và sống sót qua restart process. Không hard-code path máy dev — store location nhận qua CLI arg hoặc config, default vào `~/.oos-lite/`.

---

## 8. Output format mỗi lượt

```
## Status
## Files Created/Modified
## Implementation
## Tests (map rõ vào Definition of Done của milestone)
## Benchmark (nếu milestone yêu cầu)
## Problems / Risks
## ⚠️ Cần quyết định (nếu có — nếu mục này xuất hiện, KHÔNG có "Next Step", dừng tại đây)
## Next Step (chỉ khi không có mục "Cần quyết định")
```

---

## 9. Bắt đầu

Thực hiện **Milestone 0** ngay. Dừng lại sau khi in output theo §8. Không tự động chạy sang Milestone 1.
