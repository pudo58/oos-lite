# Đóng góp cho OOS-Lite

[English](../../CONTRIBUTING.md) | [简体中文](CONTRIBUTING.zh-CN.md) | [日本語](CONTRIBUTING.ja.md) | [Français](CONTRIBUTING.fr.md) | [한국어](CONTRIBUTING.ko.md) | **Tiếng Việt**

> Bản dịch này dựa trên [CONTRIBUTING tiếng Anh](../../CONTRIBUTING.md). Nếu hai bản khác nhau, hãy theo bản tiếng Anh.

Cảm ơn bạn đã quan tâm đến OOS-Lite. Báo lỗi, sửa tài liệu, viết test hay gửi code đều được hoan nghênh. Tài liệu này hướng dẫn cách cài môi trường và thế nào là một đóng góp tốt.

## Các cách đóng góp

- **Báo lỗi.** Mở issue, ghi rõ hệ điều hành, phiên bản OOS-Lite (`oos-lite --version`), các lệnh đã chạy, kết quả mong đợi và kết quả thực tế. Log rất có ích; chạy với `RUST_LOG=debug` để có thêm chi tiết.
- **Đề xuất tính năng.** Mở issue mô tả vấn đề cần giải quyết trước khi viết code, nhất là với thay đổi định dạng lưu trên đĩa hoặc CLI.
- **Cải thiện tài liệu hoặc bản dịch.** README và CONTRIBUTING có bản tiếng Anh, Trung, Nhật, Pháp, Hàn và Việt. Bản tiếng Anh là bản gốc; các bản dịch nằm trong [`docs/i18n/`](.).
- **Gửi pull request.** PR nhỏ, tập trung được review nhanh hơn PR lớn.

## Báo lỗi bảo mật

**Không** mở issue công khai cho lỗi bảo mật, ví dụ lộ dữ liệu qua Web UI hoặc server chia sẻ, điểm yếu mã hóa, hay path traversal. Hãy dùng tính năng [báo lỗ hổng riêng của GitHub](https://github.com/pudo58/oos-lite/security/advisories/new) và kèm các bước tái hiện. Vui lòng cho người bảo trì thời gian hợp lý để phát hành bản vá trước khi công bố.

## Cài môi trường phát triển

Bạn cần:

- Rust stable bản gần đây, có `clippy` và `rustfmt` (`rustup component add clippy rustfmt`);
- trình liên kết C (MSVC hoặc MinGW trên Windows);
- trên Linux, `libfuse3-dev` và `pkg-config` để build phần FUSE.

```bash
git clone https://github.com/<your-username>/oos-lite.git
cd oos-lite
cargo build --workspace
cargo test --workspace
```

Workspace có ba crate:

| Crate | Đường dẫn | Nội dung |
| :--- | :--- | :--- |
| `oos-lite-core` | `core/` | Storage engine: chunk, segment, WAL, metadata, GC, mã hóa, watcher |
| `oos-lite` | `cli/` | CLI, ứng dụng desktop, Web UI, mount, chia sẻ, tích hợp Windows |
| `oos-lite-benchmarks` | `benchmarks/` | Benchmark Criterion và công cụ so sánh |

Khi test thủ công, luôn dùng một kho tạm, ví dụ `oos-lite --store-dir /tmp/oos-test ...`, để không đụng vào dữ liệu thật. Đặt `OOS_DISABLE_PUBLIC_SHARING=1` khi chạy UI ở máy nếu không cần chia sẻ.

## Trước khi mở pull request

Chạy các lệnh sau ở thư mục gốc:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test --workspace
```

CI chạy `cargo check` và `cargo test` trên Ubuntu và Windows. Hãy đảm bảo thay đổi của bạn không thêm cảnh báo clippy mới.

### Thay đổi storage engine

`core/` giữ dữ liệu của người dùng, nên thay đổi ở đây cần cẩn thận hơn:

- **Thêm test.** Sửa lỗi cần có regression test (xem `core/tests/core_regressions.rs`). Thay đổi luồng ghi, khôi phục hoặc GC cần có test crash trong `core/tests/kill_recovery.rs`.
- **Giữ tương thích định dạng trên đĩa.** Kho, segment, bản ghi WAL và metadata hiện có phải tiếp tục dùng được. Nếu buộc phải đổi định dạng, hãy thảo luận trong issue trước và kèm đường chuyển đổi cùng test, giống phần chuyển từ sled sang redb trong `core/src/index/migration.rs`.
- **Tôn trọng thứ tự bền vững.** Dữ liệu phải được fsync trước khi metadata trỏ tới nó được công bố.
- **Không ghi log bí mật.** Passphrase và khóa không bao giờ được xuất hiện trong log, thông báo lỗi hay output `Debug`.
- **Tránh `unwrap()` và `expect()`** trong code không phải test mà xử lý input người dùng hoặc dữ liệu trên đĩa. Hãy trả về `OosLiteError`.

### Thay đổi CLI, UI hoặc mạng

- Giữ server UI mặc định bind vào `127.0.0.1` và giữ nguyên kiểm tra Host/Origin.
- Bất cứ thứ gì lắng nghe trên mạng hoặc tải và chạy code đều cần giải thích rõ trong mô tả PR.
- Nếu đổi lệnh hoặc tùy chọn, hãy cập nhật README (ít nhất bản tiếng Anh).

## Commit message

Dùng [Conventional Commits](https://www.conventionalcommits.org/), giống lịch sử hiện có:

```
feat(core): add keyed chunk IDs for encrypted stores
fix(watcher): retry files locked during startup
docs: update mount guide
```

Các loại thường dùng: `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `ci`, `chore`. Mỗi commit chỉ chứa một thay đổi; tránh gộp nhiều tính năng vào một commit.

## Quy trình pull request

1. Fork repo và tạo branch từ `master`.
2. Thực hiện thay đổi, kèm test và tài liệu.
3. Chạy format, clippy và test như trên.
4. Mở pull request, giải thích đã đổi gì, vì sao và đã test thế nào. Liên kết issue liên quan nếu có.
5. Xử lý góp ý review bằng cách push thêm commit lên cùng branch.

## Bản dịch

Khi sửa `README.md` hoặc `CONTRIBUTING.md`, bạn không cần tự cập nhật mọi bản dịch. Hãy ghi trong PR rằng bản dịch đã cũ để người bản ngữ cập nhật sau. Nếu bạn có sửa bản dịch, giữ nguyên code block, lệnh và link như bản tiếng Anh.

## Giấy phép

Khi đóng góp, bạn đồng ý rằng phần đóng góp sẽ được cấp phép song song theo MIT License và Apache License 2.0, giống dự án, không kèm điều khoản bổ sung nào.
