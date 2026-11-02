# OOS-Lite

[English](../../README.md) | **简体中文** | [日本語](README.ja.md) | [Français](README.fr.md) | [한국어](README.ko.md) | [Tiếng Việt](README.vi.md)

[![CI](https://github.com/pudo58/oos-lite/actions/workflows/ci.yml/badge.svg)](https://github.com/pudo58/oos-lite/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/pudo58/oos-lite)](https://github.com/pudo58/oos-lite/releases)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE)
[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Support OOS-Lite](https://img.shields.io/badge/support-SOL%20%7C%20USDT-087f5b.svg)](https://pudo58.github.io/oos-lite/support/)

> 本文译自[英文 README](../../README.md)。如两者内容不一致，以英文版为准。

OOS-Lite 是一个用 Rust 编写的本地优先、基于内容寻址的文件保险库。它保存文件的每一个版本，在分块级别去重，并且创建快照时不复制数据。你可以通过命令行、桌面应用、Web 仪表盘使用它，也可以把它挂载为只读虚拟驱动器。

---

## 目录

- [功能](#功能)
- [平台支持](#平台支持)
- [安装](#安装)
- [快速开始](#快速开始)
- [命令参考](#命令参考)
- [架构](#架构)
- [加密](#加密)
- [Web UI 与公开分享](#web-ui-与公开分享)
- [磁盘存储布局](#磁盘存储布局)
- [项目结构](#项目结构)
- [开发](#开发)
- [参与贡献](#参与贡献)
- [许可证](#许可证)

---

## 功能

- **分块级去重。** 文件通过 FastCDC 切分（最小 16 KiB，目标 64 KiB，最大 256 KiB），每个分块以其 BLAKE3 哈希标识。修改大文件的一部分时，只会存储发生变化的分块。
- **版本历史。** 对已有名称再次执行 `put` 会新增一个版本。你可以读取、回滚或清理旧版本。
- **快照。** 快照记录每个文件当时的版本，不复制文件数据。
- **崩溃恢复。** 写入先经过仅重做（redo-only）的预写日志（WAL），并在更新元数据前执行 fsync。重启时会重放已提交的操作，并截断最新分段末尾未写完的部分。恢复逻辑有"强制终止后重启"测试覆盖。
- **压缩。** 当能节省至少 5% 时，分块会用 Zstandard 压缩。哈希始终基于未压缩的数据计算，因此压缩不影响去重。
- **静态加密（可选）。** 分块内容和 WAL 记录使用 XChaCha20-Poly1305 加密，主密钥由 Argon2id 保护。具体覆盖范围见[加密](#加密)。
- **垃圾回收。** 标记-清除式 GC 将仍在使用的分块复制到新分段，并通过崩溃安全的重命名流程替换旧分段。
- **完整性检查。** `fsck` 校验分段记录、校验和、清单（manifest），以及加密仓库的认证标签。
- **Auto-Vault 监视器。** 监视一个文件夹并自动提交新版本，支持防抖、冷却、I/O 限速和 `.oosignore`。
- **虚拟驱动器。** 在 Windows 上通过 WebDAV（无需内核驱动），或在 Linux/macOS 上通过 FUSE，以只读方式挂载仓库。
- **桌面应用与 Web UI。** 用于浏览文件、版本和快照，上传文件以及执行维护操作的仪表盘。界面支持英语和越南语。

## 平台支持

| 平台 | 预编译版本 | 挂载方式 | CI 测试 |
| :--- | :--- | :--- | :--- |
| Windows x86_64 | 安装程序和便携版 `.zip` | WebDAV | 是 |
| Linux x86_64 (glibc) | `.tar.gz` | FUSE 3 | 是 |
| macOS | 从源码构建 | FUSE (macFUSE) | 否 |

Windows 可执行文件目前未签名，SmartScreen 可能会显示"未知发布者"警告。

## 安装

### Windows

从[发布页面](https://github.com/pudo58/oos-lite/releases)下载 `OOS-Lite-Setup-<version>.exe` 并运行。安装程序包含命令行工具（`oos-lite.exe`）和桌面应用（`oos-lite-gui.exe`），并可选择将 OOS-Lite 加入 `PATH` 和资源管理器右键菜单。

如需便携版，解压 `oos-lite-windows-x86_64-<version>.zip` 后运行 `.\oos-lite.exe --version`。

### Linux

安装运行时依赖（Ubuntu/Debian）：

```bash
sudo apt update
sudo apt install -y libfuse3-3 libwayland-client0 libxkbcommon0 libdbus-1-3
```

为当前用户下载、校验并安装（将 `v1.0.0` 替换为你需要的版本）：

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

请确保 `$HOME/.local/bin` 在 `PATH` 中。

### 从源码构建

需要较新的 Rust 稳定版工具链和 C 链接器（Windows 上为 MSVC 或 MinGW）。在 Linux 上，FUSE 支持还需要 `libfuse3-dev` 和 `pkg-config`。

```bash
git clone https://github.com/pudo58/oos-lite.git
cd oos-lite
cargo build --release -p oos-lite
```

生成的文件位于 `target/release/oos-lite` 和 `target/release/oos-lite-gui`（Windows 上带 `.exe` 后缀）。

## 快速开始

```bash
# 创建仓库（默认 ./.oos-store，可用 --store-dir 修改）
oos-lite init

# 存入一个文件，编辑后再存一次
oos-lite put report.docx
oos-lite put report.docx

# 查看版本，并取回第一个版本
oos-lite versions report.docx
oos-lite get report.docx ./report_v1.docx --version 1

# 为整个仓库创建快照，然后查看去重节省了多少空间
oos-lite snapshot create before-cleanup
oos-lite stats
```

## 命令参考

全局选项：`--store-dir <path>`（别名 `--store`，默认 `.oos-store`）、`--password <pass>`、`--password-file <file>`。口令也可以通过环境变量 `OOS_PASSWORD` 提供。

### 文件与版本

| 命令 | 说明 |
| :--- | :--- |
| `oos-lite init` | 初始化仓库（提供口令时为加密仓库） |
| `oos-lite put <path> [--name <name>]` | 存入文件，或为其新增版本 |
| `oos-lite get <name\|id> <out> [--version N]` | 将文件（最新版或指定版本）写出到磁盘 |
| `oos-lite list` | 列出已存储的文件 |
| `oos-lite versions <name\|id>` | 查看文件的版本历史 |
| `oos-lite rollback <name> --version N [--out <path>]` | 将旧版本重新设为最新版，可同时写出到磁盘 |
| `oos-lite rm <name>` | 从仓库中删除名称（历史在 prune 和 gc 之前仍保留） |
| `oos-lite prune [--keep N] [name]` | 只保留最新的 N 个版本（默认 10） |

### 快照

| 命令 | 说明 |
| :--- | :--- |
| `oos-lite snapshot create <label>` | 创建快照 |
| `oos-lite snapshot list` | 列出快照 |
| `oos-lite snapshot restore <label> <dir>` | 将快照恢复到目录 |
| `oos-lite snapshot delete <label>` | 删除快照 |

### 维护

| 命令 | 说明 |
| :--- | :--- |
| `oos-lite stats` | 逻辑大小、物理大小、节省比例、分块数量 |
| `oos-lite gc` | 回收未被引用分块占用的空间 |
| `oos-lite fsck` | 校验数据完整性 |

### 应用、挂载与集成

| 命令 | 说明 |
| :--- | :--- |
| `oos-lite app` | 启动桌面应用（别名 `desktop`、`gui`） |
| `oos-lite ui [--host 127.0.0.1] [--port 3000] [--no-open]` | 启动 Web UI |
| `oos-lite mount [Z: \| /mnt/dir] [--webdav] [--port 8080] [--cache-mb 128]` | 以只读方式挂载仓库 |
| `oos-lite watch <dir> [--debounce-secs 3] [--cooldown-secs 60] [--throttle-ms 10]` | 自动为文件夹创建版本 |
| `oos-lite shell-ext enable\|disable\|status` | Windows 资源管理器右键菜单 |
| `oos-lite stop` | 停止后台 OOS-Lite 进程并释放 3000/8080 端口 |
| `oos-lite uninstall` | 停止进程并移除已安装的组件 |

运行 `oos-lite <command> --help` 查看全部选项。

挂载后的驱动器包含三个文件夹：`current/`（所有文件的最新版本）、`snapshots/<label>/`，以及 `history/<path>/<file>@v<N>` 和 `@latest`。详见[挂载指南](../MOUNT_GUIDE.md)（越南语）。

## 架构

### 写入路径

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

### 组件

- **分块（Chunk）：** 由 FastCDC 切出的不可变字节区间，以 BLAKE3 标识，以 CRC32C 校验。
- **分段存储（Segment store）：** 只追加的文件（`segment_00000001.seg` 等），约 256 MiB 时切换到新文件。读取端使用文件描述符缓存。
- **清单（Manifest）：** 构成文件某一版本的有序分块列表。
- **ObjectId：** 128 位混合逻辑 ID，在编辑和重命名后保持不变，因此历史会跟随文件。
- **元数据：** 存储在 [redb](https://crates.io/crates/redb) 中，包括名称索引、对象记录、清单、快照和监视器状态。
- **WAL：** 仅重做日志。记录中只携带仓库里尚不存在的分块数据，避免每个分块被写两次。

### 崩溃安全

- 只有在 WAL 记录持久化之后才会更新元数据。恢复时以幂等方式重放已提交的记录，并在提交前确认清单中的每个分块都已持久化。
- 恢复只会截断最新分段末尾未写完的部分。其他位置的损坏会报告为错误，数据保持不变。
- GC 先将压实后的分段写入临时目录，再把旧分段重命名为 `.seg.old`，然后移入新分段。若此过程被崩溃打断，下次启动时会从 `.seg.old` 文件恢复。

## 加密

```bash
# 创建加密仓库（口令至少 8 个字符）
oos-lite --password-file ~/.oos-password init

# 之后的每个命令都需要同一口令
oos-lite --password-file ~/.oos-password put backup.tar.gz
export OOS_PASSWORD='...'   # 另一种方式
```

建议使用 `--password-file`（并 `chmod 600`），而不是 `--password`，后者会留在 shell 历史中。

工作原理：一个随机生成的 256 位主密钥使用 XChaCha20-Poly1305 加密分块内容和 WAL 记录，每条记录使用随机 nonce。主密钥被封装在 100 字节的 `vault.key` 文件中，封装密钥由口令经 Argon2id（64 MiB，3 次迭代）派生。已有的未加密仓库不能切换为加密模式。

**需要了解的限制：**

- **元数据未加密。** redb 数据库中的文件名、大小、时间戳、版本历史、快照标签和清单都以明文存储。
- **分块 ID 是内容的普通 BLAKE3 哈希。** 能读取仓库的人可以把自己已有的文件切块并比较哈希，从而判断该文件是否存在于仓库中。
- **丢失口令或 `vault.key` 即丢失数据。** 没有任何恢复机制。

加密适合在磁盘丢失或备份到不可信位置时保护文件内容，而不适合隐藏你拥有哪些文件。

## Web UI 与公开分享

```bash
oos-lite ui --port 3000   # 打开 http://127.0.0.1:3000
oos-lite app              # 在桌面窗口中打开同一仪表盘
```

仪表盘包括文件浏览器（列表、树形和网格视图）、预览、版本历史、快照、上传（包括文件夹）、监视器控制以及 GC/fsck。

UI 服务器默认绑定 `127.0.0.1`，并检查 `Host`、`Origin` 和 `Sec-Fetch-Site` 请求头，以阻止 DNS 重绑定和跨站请求。它**没有登录机制**。如果绑定到 `0.0.0.0`，你所在网络中的任何人都可以使用它。

**公开分享默认开启。** UI 启动时，OOS-Lite 还会：

1. 在端口 `3001` 上启动分享服务器，监听所有网络接口；
2. 启动指向该端口的 Cloudflare quick tunnel；如果仓库目录中还没有 `cloudflared`，会自动下载最新版本。

分享链接由随机令牌和可选的过期时间保护。如果不需要分享，请关闭它：

```bash
OOS_DISABLE_PUBLIC_SHARING=1 oos-lite ui        # Linux / macOS
$env:OOS_DISABLE_PUBLIC_SHARING = "1"           # Windows PowerShell，然后运行 oos-lite ui
```

## 磁盘存储布局

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

迁移到 redb 之前的版本使用 sled 存储元数据。新版本打开这类仓库时，会通过 `metadata.redb.staging` 导入 sled 数据，保留原始 sled 文件，并安装一个阻止文件，使旧版程序无法打开已迁移的仓库。迁移被中断时会自动继续。一旦 `.redb-active` 存在，数据库缺失或损坏将被报告为错误，而不会悄悄地从过时的 sled 数据重新导入。

备份仓库时，请复制整个目录，且不要删除状态标记文件。

## 项目结构

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

## 开发

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all
cargo bench --workspace                # Criterion 基准测试
cargo run -p oos-lite-benchmarks       # 与普通复制方式的对比
```

发布历史见 [release_notes.md](../../release_notes.md) 和[发布页面](https://github.com/pudo58/oos-lite/releases)。

## 参与贡献

欢迎任何形式的贡献。提交 issue 或 pull request 前，请先阅读 [CONTRIBUTING.zh-CN.md](CONTRIBUTING.zh-CN.md)。安全问题请按其中说明私下报告。

如果 OOS-Lite 对你有帮助，也可以[支持本项目](https://pudo58.github.io/oos-lite/support/)。

## 许可证

本项目采用双许可证，你可以任选其一：

- MIT License（[LICENSE-MIT](../../LICENSE-MIT)）
- Apache License, Version 2.0（[LICENSE-APACHE](../../LICENSE-APACHE)）
