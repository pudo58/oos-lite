# Tài Liệu Kỹ Thuật: Tính Năng Mount (Gắn Ổ Đĩa Ảo) Trong OOS-Lite

---

## 1. Giới Thiệu & Mục Đích Cốt Lõi

Thông thường, các hệ thống lưu trữ khử trùng lặp (Deduplicated & Content-Addressed Storage) như Git, Borg hay Kopia lưu trữ dữ liệu dưới dạng các **chunk vụn vặt bị băm nhỏ, nén và mã hóa**. Người dùng muốn lấy file phải chạy dòng lệnh `get` hoặc `restore` để tải file ra ổ cứng vật lý. Thao tác này gây ra 2 nhược điểm lớn:
1. **Tốn công sức:** Phải nhớ tên file, nhớ lệnh gõ terminal.
2. **Tốn gấp đôi dung lượng ổ đĩa:** Phải copy file thật ra ngoài thì phần mềm khác (như VLC, Word, VS Code) mới mở xem được.

**Tính năng Mount của OOS-Lite giải quyết triệt để vấn đề này:**
> Tính năng **Mount** biến toàn bộ kho dữ liệu OOS-Lite thành một **ổ đĩa ảo (mặc định là `Z:\` trên Windows hoặc thư mục `/mnt/...` trên Linux/macOS)**. Người dùng có thể mở duyệt trực tiếp, phát video, xem code, copy file như một chiếc USB hoặc ổ cứng thông thường mà **hoàn toàn không tốn thêm 1 byte dung lượng đĩa cứng nào**.

---

## 2. Cấu Trúc Thư Mục Trên Ổ Ảo `Z:\`

Khi gắn ổ ảo `Z:\` thành công, bạn mở Windows Explorer lên sẽ thấy hệ thống được tổ chức thành 3 thư mục ảo chuẩn hóa:

```
Z:\ (Ổ Đĩa Ảo OOS-Lite)
├── 📁 current\                       <── Toàn bộ file ở phiên bản MỚI NHẤT
│   ├── 📄 baocao.docx
│   └── 📁 source\
│       └── 📄 main.rs
│
├── 📁 history\                       <── Lịch sử mọi phiên bản quá khứ
│   ├── 📄 baocao.docx@v1
│   ├── 📄 baocao.docx@v2
│   ├── 🔗 baocao.docx@latest (Symlink trỏ tới v2)
│   └── 📁 source\
│       ├── 📄 main.rs@v1
│       └── 🔗 main.rs@latest
│
└── 📁 snapshots\                     <── Các mốc đóng băng thời gian
    ├── 📁 backup_ngay_01\
    │   └── ... (toàn bộ file tại thời điểm tạo mốc)
    └── 📁 release_v1.0\
        └── ...
```

### Ý nghĩa từng thư mục:
* **`Z:\current\`**: Hiển thị phiên bản mới nhất của tất cả các file trong kho lưu trữ, giữ nguyên 100% cấu trúc thư mục logic như lúc bạn tải lên.
* **`Z:\history\`**: Cung cấp khả năng "du hành thời gian". Mỗi khi bạn cập nhật file nhiều lần, các phiên bản cũ sẽ tự động lưu dưới dạng `@v1`, `@v2`, `@v3`... kèm một liên kết ảo `@latest` trỏ đến bản cao nhất.
* **`Z:\snapshots\`**: Chứa danh sách các mốc sao lưu (Snapshots). Mỗi thư mục con là một bản đóng băng toàn vẹn trạng thái hệ thống tại thời điểm snapshot được bấm tạo.

---

## 3. Cơ Chế Hoạt Động Kỹ Thuật (Under the Hood)

### 3.1. Ráp Chunks Động Theo Yêu Cầu (On-Demand Chunk Assembly)
* Dưới ổ cứng thật (`.oos-store/segments/`), dữ liệu chỉ là các chunk nhỏ (16 KiB – 64 KiB) nén Zstandard.
* Khi Windows hoặc VLC yêu cầu đọc file (ví dụ đọc đoạn từ byte `10,000` đến `50,000` của `phim.mp4`):
  1. Động cơ VFS tra cứu **Manifest** của file để biết đoạn dữ liệu đó nằm ở chunk nào.
  2. Đọc đúng chunk cần thiết từ file segment lên bộ nhớ RAM.
  3. Giải nén Zstandard tức thì và stream trả byte về cho Windows/VLC.
* 👉 **Ưu điểm:** Tua video mượt mà, mở file vài GB trong 1 giây mà không phải giải nén toàn bộ file ra đĩa.

### 3.2. Bounded LRU Cache (Chống Tràn Bộ Nhớ RAM)
* Các chunk sau khi giải nén được giữ tạm trong bộ nhớ đệm **LRU Cache** (mặc định giới hạn trần 128 MB).
* Khi người dùng xem video hoặc đọc file liên tục, các chunk cũ nhất sẽ tự động bị dọn dẹp để nhường chỗ cho chunk mới.
* Đảm bảo hệ thống tiêu thụ RAM ổn định, **không bao giờ bị rò rỉ bộ nhớ (memory leak)**.

### 3.3. Tự Động Cập Nhật Thời Gian Thực (Live Sync / Auto-Refresh)
* Khi bạn nạp file mới qua Desktop App (tab Upload Center) hoặc tạo snapshot mới:
  * VFS tự động phát hiện thay đổi và làm mới danh mục node trong $\le 500\text{ ms}$.
  * Bạn chỉ cần mở Windows Explorer hoặc ấn phím **`F5`** là file mới xuất hiện ngay lập tức trong `Z:\current\`.

### 3.4. Chế Độ Chỉ Đọc (Read-Only) Bảo Vệ Tuyệt Đối
* Ổ `Z:\` được khóa cứng ở chế độ Read-Only:
  * Từ chối mọi thao tác ghi, đổi tên hoặc xóa trực tiếp từ Windows (`403 Forbidden`).
  * **Chống lỡ tay:** Ngăn chặn việc bấm nhầm `Shift + Delete` làm mất dữ liệu sao lưu.
  * **Miễn nhiễm Ransomware:** Nếu máy tính bị dính mã độc tống tiền, virus không thể mã hóa hoặc xóa dữ liệu bên trong ổ `Z:\`.

---

## 4. Kiến Trúc Hỗ Trợ Đa Nền Tảng (Cross-Platform)

| Tiêu chí | Trên Windows | Trên Linux / macOS |
| :--- | :--- | :--- |
| **Giao thức cốt lõi** | **Native WebDAV Redirector** | **POSIX FUSE Driver** |
| **Điểm mount** | Ổ đĩa ký tự (Ví dụ: `Z:\`) | Đường dẫn thư mục (Ví dụ: `/mnt/oos`) |
| **Driver bổ sung** | **Không cần** (Sử dụng service `WebClient` có sẵn của Windows 10/11) | Kernel module `fuse` có sẵn trên hệ điều hành |
| **Bản quyền mã nguồn** | **100% Thuần Rust (MIT / Apache-2.0)**, không dính mã nguồn GPL | Crate `fuser` mã nguồn mở chuẩn |

---

## 5. Hướng Dẫn Sử Dụng

### Cách 1: Sử Dụng Trên Ứng Dụng Desktop (Khuyên Dùng — 1 Click)
1. Bấm đúp chuột vào shortcut **`OOS-Lite.bat`** ngoài màn hình Desktop.
2. Ứng dụng mở ra và tự động gắn ổ `Z:\`.
3. Nhấp vào nút **`Mở Explorer`** trên màn hình để duyệt file ngay.
4. Có thể bấm **`⚡ Kết Nối Ổ Đĩa Z:\`** hoặc **`✕ Ngắt Kết Nối`** bất kỳ lúc nào ngay trên giao diện.

### Cách 2: Sử Dụng Qua Dòng Lệnh CLI

```powershell
# 1. Gắn ổ đĩa mặc định (Z: trên Windows, hoặc thư mục trên Linux)
oos-lite mount Z:

# 2. Tùy chỉnh dung lượng bộ nhớ đệm RAM (mặc định 128 MB)
oos-lite mount Z: --cache-mb 256

# 3. Chạy WebDAV server trên port tùy chọn
oos-lite mount Z: --port 8080
```

---

## 6. Bảng So Sánh: Mount OOS-Lite vs Sao Chép File Truyền Thống

| Tiêu chí | Sao chép truyền thống (`cp` / `restore`) | Gắn ổ đĩa ảo OOS-Lite (`mount`) |
| :--- | :--- | :--- |
| **Dung lượng đĩa tiêu tốn** | Tốn thêm $100\%$ dung lượng gốc cho mỗi file trích xuất | **$0\text{ Byte}$** (Hoàn toàn không tốn thêm đĩa) |
| **Thời gian mở file 5 GB** | Chờ 30 – 60 giây để ghi ra đĩa | **Mở ngay lập tức trong 1 giây** (Streaming) |
| **Xem lại phiên bản cũ** | Phải tìm bản sao lưu cũ và giải nén đè lên | Vào thẳng `Z:\history\` mở song song cả bản cũ và mới |
| **Độ an toàn trước Ransomware** | Dễ bị virus quét trúng và mã hóa đè | **Khóa ghi Read-Only**, virus hoàn toàn bất lực |
