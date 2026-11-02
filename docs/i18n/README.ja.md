# OOS-Lite

[English](../../README.md) | [简体中文](README.zh-CN.md) | **日本語** | [Français](README.fr.md) | [한국어](README.ko.md) | [Tiếng Việt](README.vi.md)

[![CI](https://github.com/pudo58/oos-lite/actions/workflows/ci.yml/badge.svg)](https://github.com/pudo58/oos-lite/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/pudo58/oos-lite)](https://github.com/pudo58/oos-lite/releases)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE)
[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Support OOS-Lite](https://img.shields.io/badge/support-SOL%20%7C%20USDT-087f5b.svg)](https://pudo58.github.io/oos-lite/support/)

> この文書は[英語版 README](../../README.md) の翻訳です。内容が異なる場合は英語版を優先してください。

OOS-Lite は Rust で書かれた、ローカルファーストでコンテンツアドレス方式のファイル保管庫です。ファイルのすべてのバージョンを保持し、チャンク単位で重複排除を行い、データをコピーせずにスナップショットを作成します。コマンドライン、デスクトップアプリ、Web ダッシュボードから利用できるほか、読み取り専用の仮想ドライブとしてマウントすることもできます。

---

## 目次

- [機能](#機能)
- [対応プラットフォーム](#対応プラットフォーム)
- [インストール](#インストール)
- [クイックスタート](#クイックスタート)
- [コマンドリファレンス](#コマンドリファレンス)
- [アーキテクチャ](#アーキテクチャ)
- [暗号化](#暗号化)
- [Web UI と公開共有](#web-ui-と公開共有)
- [ディスク上のレイアウト](#ディスク上のレイアウト)
- [プロジェクト構成](#プロジェクト構成)
- [開発](#開発)
- [コントリビュート](#コントリビュート)
- [ライセンス](#ライセンス)

---

## 機能

- **チャンク単位の重複排除。** ファイルは FastCDC（最小 16 KiB、目標 64 KiB、最大 256 KiB）で分割され、各チャンクは BLAKE3 ハッシュで識別されます。大きなファイルの一部を編集した場合、変更されたチャンクだけが保存されます。
- **バージョン履歴。** 既存の名前に対して `put` を実行するたびに新しいバージョンが追加されます。古いバージョンの読み出し、ロールバック、削除（prune）ができます。
- **スナップショット。** スナップショットは各ファイルのその時点のバージョンを記録するだけで、ファイルデータはコピーしません。
- **クラッシュリカバリ。** 書き込みは redo 専用のログ先行書き込み（WAL）を経由し、メタデータ更新の前に fsync されます。再起動時にはコミット済みの操作が再適用され、最新セグメント末尾の書きかけ部分は切り詰められます。リカバリは強制終了と再起動のテストで検証されています。
- **圧縮。** 5% 以上削減できる場合、チャンクは Zstandard で圧縮されます。ハッシュは常に非圧縮データで計算されるため、圧縮は重複排除に影響しません。
- **保存時の暗号化（任意）。** チャンクの内容と WAL レコードは XChaCha20-Poly1305 で暗号化され、マスターキーは Argon2id で保護されます。対象範囲の詳細は[暗号化](#暗号化)を参照してください。
- **ガベージコレクション。** マーク＆スイープ方式の GC が使用中のチャンクを新しいセグメントにコピーし、クラッシュに安全なリネーム手順で置き換えます。
- **整合性チェック。** `fsck` はセグメントレコード、チェックサム、マニフェスト、そして暗号化ストアでは認証タグを検証します。
- **Auto-Vault ウォッチャー。** フォルダーを監視して新しいバージョンを自動的にコミットします。デバウンス、クールダウン、I/O スロットリング、`.oosignore` に対応しています。
- **仮想ドライブ。** Windows では WebDAV（カーネルドライバー不要）、Linux/macOS では FUSE を使って、ストアを読み取り専用でマウントできます。
- **デスクトップアプリと Web UI。** ファイル、バージョン、スナップショットの閲覧、アップロード、メンテナンスを行えるダッシュボードです。UI は英語とベトナム語に対応しています。

## 対応プラットフォーム

| プラットフォーム | ビルド済みリリース | マウント方式 | CI でのテスト |
| :--- | :--- | :--- | :--- |
| Windows x86_64 | インストーラーとポータブル版 `.zip` | WebDAV | あり |
| Linux x86_64 (glibc) | `.tar.gz` | FUSE 3 | あり |
| macOS | ソースからビルド | FUSE (macFUSE) | なし |

Windows のバイナリは現在署名されていないため、SmartScreen が「不明な発行元」の警告を表示することがあります。

## インストール

### Windows

[リリースページ](https://github.com/pudo58/oos-lite/releases)から `OOS-Lite-Setup-<version>.exe` をダウンロードして実行します。インストーラーには CLI（`oos-lite.exe`）とデスクトップアプリ（`oos-lite-gui.exe`）が含まれ、必要に応じて OOS-Lite を `PATH` とエクスプローラーの右クリックメニューに追加できます。

ポータブル版を使う場合は `oos-lite-windows-x86_64-<version>.zip` を展開し、`.\oos-lite.exe --version` を実行します。

### Linux

ランタイムライブラリをインストールします（Ubuntu/Debian）。

```bash
sudo apt update
sudo apt install -y libfuse3-3 libwayland-client0 libxkbcommon0 libdbus-1-3
```

現在のユーザー向けにダウンロード、検証、インストールします（`v1.0.0` は必要なバージョンに置き換えてください）。

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

`$HOME/.local/bin` が `PATH` に含まれていることを確認してください。

### ソースからビルド

最近の Rust 安定版ツールチェーンと C リンカー（Windows では MSVC または MinGW）が必要です。Linux で FUSE を使う場合は `libfuse3-dev` と `pkg-config` も必要です。

```bash
git clone https://github.com/pudo58/oos-lite.git
cd oos-lite
cargo build --release -p oos-lite
```

バイナリは `target/release/oos-lite` と `target/release/oos-lite-gui`（Windows では `.exe` 付き）に出力されます。

## クイックスタート

```bash
# ストアを作成（既定は ./.oos-store、--store-dir で変更可能）
oos-lite init

# ファイルを保存し、編集してからもう一度保存
oos-lite put report.docx
oos-lite put report.docx

# バージョンを確認し、最初のバージョンを取り出す
oos-lite versions report.docx
oos-lite get report.docx ./report_v1.docx --version 1

# ストア全体のスナップショットを作成し、重複排除で節約できた容量を確認
oos-lite snapshot create before-cleanup
oos-lite stats
```

## コマンドリファレンス

グローバルオプション：`--store-dir <path>`（別名 `--store`、既定値 `.oos-store`）、`--password <pass>`、`--password-file <file>`。パスフレーズは環境変数 `OOS_PASSWORD` でも指定できます。

### ファイルとバージョン

| コマンド | 説明 |
| :--- | :--- |
| `oos-lite init` | ストアを初期化（パスフレーズ指定時は暗号化） |
| `oos-lite put <path> [--name <name>]` | ファイルを保存、または新しいバージョンを追加 |
| `oos-lite get <name\|id> <out> [--version N]` | ファイル（最新版または指定版）をディスクに書き出す |
| `oos-lite list` | 保存済みファイルの一覧 |
| `oos-lite versions <name\|id>` | ファイルのバージョン履歴を表示 |
| `oos-lite rollback <name> --version N [--out <path>]` | 古いバージョンを再び最新にする（ディスクへの書き出しも可） |
| `oos-lite rm <name>` | ストアから名前を削除（履歴は prune と gc まで残る） |
| `oos-lite prune [--keep N] [name]` | 最新の N バージョンだけを残す（既定 10） |

### スナップショット

| コマンド | 説明 |
| :--- | :--- |
| `oos-lite snapshot create <label>` | スナップショットを作成 |
| `oos-lite snapshot list` | スナップショットの一覧 |
| `oos-lite snapshot restore <label> <dir>` | スナップショットをディレクトリに復元 |
| `oos-lite snapshot delete <label>` | スナップショットを削除 |

### メンテナンス

| コマンド | 説明 |
| :--- | :--- |
| `oos-lite stats` | 論理サイズ、物理サイズ、節約率、チャンク数 |
| `oos-lite gc` | 参照されていないチャンクの領域を回収 |
| `oos-lite fsck` | データの整合性を検証 |

### アプリ、マウント、連携

| コマンド | 説明 |
| :--- | :--- |
| `oos-lite app` | デスクトップアプリを起動（別名 `desktop`、`gui`） |
| `oos-lite ui [--host 127.0.0.1] [--port 3000] [--no-open]` | Web UI を起動 |
| `oos-lite mount [Z: \| /mnt/dir] [--webdav] [--port 8080] [--cache-mb 128]` | ストアを読み取り専用でマウント |
| `oos-lite watch <dir> [--debounce-secs 3] [--cooldown-secs 60] [--throttle-ms 10]` | フォルダーを自動でバージョン管理 |
| `oos-lite shell-ext enable\|disable\|status` | Windows エクスプローラーの右クリックメニュー |
| `oos-lite stop` | バックグラウンドの OOS-Lite プロセスを停止し、ポート 3000/8080 を解放 |
| `oos-lite uninstall` | プロセスを停止し、インストールされたコンポーネントを削除 |

すべてのオプションは `oos-lite <command> --help` で確認できます。

マウントしたドライブには 3 つのフォルダーがあります：`current/`（全ファイルの最新版）、`snapshots/<label>/`、そして `history/<path>/<file>@v<N>` と `@latest`。詳しくは[マウントガイド](../MOUNT_GUIDE.md)（ベトナム語）を参照してください。

## アーキテクチャ

### 書き込みパス

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

### コンポーネント

- **チャンク：** FastCDC が切り出した不変のバイト列。BLAKE3 で識別し、CRC32C で検証します。
- **セグメントストア：** 追記専用ファイル（`segment_00000001.seg` など）で、約 256 MiB で次のファイルに切り替わります。読み取り側はファイルディスクリプタのキャッシュを使います。
- **マニフェスト：** ファイルのあるバージョンを構成するチャンクの順序付きリスト。
- **ObjectId：** 編集やリネームをしても変わらない 128 ビットのハイブリッド論理 ID。履歴がファイルに追従します。
- **メタデータ：** [redb](https://crates.io/crates/redb) に保存されます。名前インデックス、オブジェクトレコード、マニフェスト、スナップショット、ウォッチャーの状態が含まれます。
- **WAL：** redo 専用ログ。レコードにはストアにまだ存在しないチャンクのデータだけが含まれるため、各チャンクが二重に書き込まれることはありません。

### クラッシュ安全性

- メタデータは WAL レコードが永続化されてから更新されます。リカバリ時にはコミット済みのレコードを冪等に再適用し、コミット前にマニフェスト内のすべてのチャンクが永続化されていることを確認します。
- リカバリが切り詰めるのは、最新セグメント末尾の書きかけ部分だけです。それ以外の場所の破損はエラーとして報告され、データには手を加えません。
- GC は圧縮後のセグメントを一時ディレクトリに書き出し、古いセグメントを `.seg.old` にリネームしてから新しいセグメントを配置します。途中でクラッシュした場合は、次回起動時に `.seg.old` から復元されます。

## 暗号化

```bash
# 暗号化ストアを作成（パスフレーズは 8 文字以上）
oos-lite --password-file ~/.oos-password init

# 以降のコマンドはすべて同じパスフレーズが必要
oos-lite --password-file ~/.oos-password put backup.tar.gz
export OOS_PASSWORD='...'   # 別の方法
```

`--password` はシェル履歴に残るため、`--password-file`（`chmod 600` 付き）の使用を推奨します。

仕組み：ランダムな 256 ビットのマスターキーが、XChaCha20-Poly1305 でチャンクの内容と WAL レコードを暗号化します。レコードごとにランダムな nonce を使います。マスターキーは 100 バイトの `vault.key` ファイルに格納され、パスフレーズから Argon2id（64 MiB、3 回反復）で導出した鍵でラップされます。既存の非暗号化ストアを暗号化モードに切り替えることはできません。

**知っておくべき制限：**

- **メタデータは暗号化されません。** redb データベース内のファイル名、サイズ、タイムスタンプ、バージョン履歴、スナップショットのラベル、マニフェストは平文で保存されます。
- **チャンク ID は内容の通常の BLAKE3 ハッシュです。** ストアを読み取れる人は、手元のファイルをチャンク分割してハッシュを比較することで、そのファイルがストアに含まれているかどうかを判別できます。
- **パスフレーズまたは `vault.key` を失うと、データも失われます。** 復旧手段はありません。

暗号化は、ディスクの紛失時や信頼できないバックアップ先でファイルの内容を守るためのものであり、どのファイルを持っているかを隠すためのものではありません。

## Web UI と公開共有

```bash
oos-lite ui --port 3000   # http://127.0.0.1:3000 を開く
oos-lite app              # 同じダッシュボードをデスクトップウィンドウで開く
```

ダッシュボードには、ファイルエクスプローラー（リスト、ツリー、グリッド表示）、プレビュー、バージョン履歴、スナップショット、アップロード（フォルダーを含む）、ウォッチャーの操作、GC/fsck があります。

UI サーバーは既定で `127.0.0.1` にバインドされ、`Host`、`Origin`、`Sec-Fetch-Site` ヘッダーを検査して DNS リバインディングとクロスサイトリクエストを防ぎます。**ログイン機能はありません。** `0.0.0.0` にバインドすると、同じネットワーク上の誰でも利用できてしまいます。

**公開共有は既定で有効です。** UI の起動時に、OOS-Lite は次の処理も行います。

1. ポート `3001` で共有サーバーを起動し、すべてのネットワークインターフェースで待ち受ける。
2. そのポートへの Cloudflare quick tunnel を開始する。ストアディレクトリに `cloudflared` がなければ、最新版を自動でダウンロードする。

共有リンクはランダムなトークンと任意の有効期限で保護されます。共有が不要な場合は無効にしてください。

```bash
OOS_DISABLE_PUBLIC_SHARING=1 oos-lite ui        # Linux / macOS
$env:OOS_DISABLE_PUBLIC_SHARING = "1"           # Windows PowerShell（その後 oos-lite ui を実行）
```

## ディスク上のレイアウト

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

redb への移行より前のバージョンは、メタデータを sled に保存していました。新しいビルドがそのようなストアを開くと、`metadata.redb.staging` を経由して sled のデータを取り込み、元の sled ファイルはそのまま残し、古いバイナリが移行済みストアを開けないようにブロッカーを設置します。移行が中断された場合は自動的に再開されます。`.redb-active` が存在する状態でデータベースが欠落または破損している場合は、古い sled データから黙って再インポートするのではなく、エラーとして報告されます。

ストアをバックアップする際は、ディレクトリ全体をコピーし、状態を示すマーカーファイルは削除しないでください。

## プロジェクト構成

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

## 開発

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all
cargo bench --workspace                # Criterion ベンチマーク
cargo run -p oos-lite-benchmarks       # 通常のコピーとの比較
```

リリース履歴は [release_notes.md](../../release_notes.md) と[リリースページ](https://github.com/pudo58/oos-lite/releases)にあります。

## コントリビュート

コントリビュートを歓迎します。issue や pull request を作成する前に [CONTRIBUTING.ja.md](CONTRIBUTING.ja.md) をお読みください。セキュリティ上の問題は、そこに記載された方法で非公開で報告してください。

OOS-Lite が役に立ったら、[プロジェクトを支援](https://pudo58.github.io/oos-lite/support/)していただくこともできます。

## ライセンス

次のいずれかのライセンスを選択して利用できます。

- MIT License（[LICENSE-MIT](../../LICENSE-MIT)）
- Apache License, Version 2.0（[LICENSE-APACHE](../../LICENSE-APACHE)）
