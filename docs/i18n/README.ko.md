# OOS-Lite

[English](../../README.md) | [简体中文](README.zh-CN.md) | [日本語](README.ja.md) | [Français](README.fr.md) | **한국어** | [Tiếng Việt](README.vi.md)

[![CI](https://github.com/pudo58/oos-lite/actions/workflows/ci.yml/badge.svg)](https://github.com/pudo58/oos-lite/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/pudo58/oos-lite)](https://github.com/pudo58/oos-lite/releases)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE)
[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Support OOS-Lite](https://img.shields.io/badge/support-SOL%20%7C%20USDT-087f5b.svg)](https://pudo58.github.io/oos-lite/support/)

> 이 문서는 [영문 README](../../README.md)의 번역본입니다. 내용이 다를 경우 영문판을 기준으로 합니다.

OOS-Lite는 Rust로 작성된 로컬 우선, 콘텐츠 주소 기반의 파일 보관소입니다. 파일의 모든 버전을 보관하고, 청크 단위로 중복을 제거하며, 데이터를 복사하지 않고 스냅샷을 만듭니다. 명령줄, 데스크톱 앱, 웹 대시보드에서 사용할 수 있고, 읽기 전용 가상 드라이브로 마운트할 수도 있습니다.

---

## 목차

- [기능](#기능)
- [지원 플랫폼](#지원-플랫폼)
- [설치](#설치)
- [빠른 시작](#빠른-시작)
- [명령어 참조](#명령어-참조)
- [아키텍처](#아키텍처)
- [암호화](#암호화)
- [웹 UI와 공개 공유](#웹-ui와-공개-공유)
- [디스크 저장 구조](#디스크-저장-구조)
- [프로젝트 구조](#프로젝트-구조)
- [개발](#개발)
- [기여하기](#기여하기)
- [라이선스](#라이선스)

---

## 기능

- **청크 단위 중복 제거.** 파일은 FastCDC(최소 16 KiB, 목표 64 KiB, 최대 256 KiB)로 분할되며, 각 청크는 BLAKE3 해시로 식별됩니다. 큰 파일의 일부만 수정하면 변경된 청크만 저장됩니다.
- **버전 기록.** 기존 이름으로 `put`을 실행할 때마다 새 버전이 추가됩니다. 이전 버전을 읽거나, 롤백하거나, 정리(prune)할 수 있습니다.
- **스냅샷.** 스냅샷은 각 파일의 그 시점 버전을 기록할 뿐, 파일 데이터를 복사하지 않습니다.
- **충돌 복구.** 모든 쓰기는 redo 전용 선행 기록 로그(WAL)를 거치며, 메타데이터를 갱신하기 전에 fsync됩니다. 재시작 시 커밋된 작업을 다시 적용하고, 최신 세그먼트 끝의 미완료 쓰기는 잘라냅니다. 복구 과정은 강제 종료 후 재시작 테스트로 검증됩니다.
- **압축.** 5% 이상 줄어드는 경우 청크를 Zstandard로 압축합니다. 해시는 항상 압축 전 데이터로 계산하므로 압축이 중복 제거에 영향을 주지 않습니다.
- **저장 데이터 암호화(선택).** 청크 내용과 WAL 레코드는 XChaCha20-Poly1305로 암호화되며, 마스터 키는 Argon2id로 보호됩니다. 정확한 적용 범위는 [암호화](#암호화)를 참고하세요.
- **가비지 컬렉션.** 마크 앤 스윕 방식 GC가 사용 중인 청크를 새 세그먼트로 복사한 뒤, 충돌에 안전한 이름 변경 절차로 교체합니다.
- **무결성 검사.** `fsck`는 세그먼트 레코드, 체크섬, 매니페스트, 그리고 암호화된 저장소의 경우 인증 태그를 검증합니다.
- **Auto-Vault 감시기.** 폴더를 감시하다가 새 버전을 자동으로 커밋합니다. 디바운스, 쿨다운, I/O 제한, `.oosignore`를 지원합니다.
- **가상 드라이브.** Windows에서는 WebDAV(커널 드라이버 불필요), Linux/macOS에서는 FUSE로 저장소를 읽기 전용으로 마운트합니다.
- **데스크톱 앱과 웹 UI.** 파일, 버전, 스냅샷을 탐색하고 업로드와 유지보수를 실행하는 대시보드입니다. 영어와 베트남어 UI를 제공합니다.

## 지원 플랫폼

| 플랫폼 | 미리 빌드된 릴리스 | 마운트 방식 | CI 테스트 |
| :--- | :--- | :--- | :--- |
| Windows x86_64 | 설치 프로그램 및 포터블 `.zip` | WebDAV | 예 |
| Linux x86_64 (glibc) | `.tar.gz` | FUSE 3 | 예 |
| macOS | 소스에서 빌드 | FUSE (macFUSE) | 아니요 |

Windows 바이너리는 아직 서명되지 않았으므로 SmartScreen이 "알 수 없는 게시자" 경고를 표시할 수 있습니다.

## 설치

### Windows

[릴리스 페이지](https://github.com/pudo58/oos-lite/releases)에서 `OOS-Lite-Setup-<version>.exe`를 내려받아 실행합니다. 설치 프로그램에는 CLI(`oos-lite.exe`)와 데스크톱 앱(`oos-lite-gui.exe`)이 포함되며, 원하면 OOS-Lite를 `PATH`와 탐색기 오른쪽 클릭 메뉴에 추가할 수 있습니다.

포터블 버전을 쓰려면 `oos-lite-windows-x86_64-<version>.zip`의 압축을 풀고 `.\oos-lite.exe --version`을 실행합니다.

### Linux

런타임 라이브러리를 설치합니다(Ubuntu/Debian).

```bash
sudo apt update
sudo apt install -y libfuse3-3 libwayland-client0 libxkbcommon0 libdbus-1-3
```

현재 사용자용으로 내려받고, 검증하고, 설치합니다(`v1.0.0`은 원하는 버전으로 바꾸세요).

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

`$HOME/.local/bin`이 `PATH`에 포함되어 있는지 확인하세요.

### 소스에서 빌드

최신 Rust 안정 버전 툴체인과 C 링커(Windows에서는 MSVC 또는 MinGW)가 필요합니다. Linux에서 FUSE를 지원하려면 `libfuse3-dev`와 `pkg-config`도 필요합니다.

```bash
git clone https://github.com/pudo58/oos-lite.git
cd oos-lite
cargo build --release -p oos-lite
```

바이너리는 `target/release/oos-lite`와 `target/release/oos-lite-gui`(Windows에서는 `.exe`)에 생성됩니다.

## 빠른 시작

```bash
# 저장소 생성 (기본값 ./.oos-store, --store-dir로 변경 가능)
oos-lite init

# 파일을 저장하고, 수정한 뒤 다시 저장
oos-lite put report.docx
oos-lite put report.docx

# 버전을 확인하고 첫 번째 버전을 꺼내기
oos-lite versions report.docx
oos-lite get report.docx ./report_v1.docx --version 1

# 전체 저장소의 스냅샷을 만들고, 중복 제거로 절약한 공간 확인
oos-lite snapshot create before-cleanup
oos-lite stats
```

## 명령어 참조

전역 옵션: `--store-dir <path>`(별칭 `--store`, 기본값 `.oos-store`), `--password <pass>`, `--password-file <file>`. 암호 문구는 환경 변수 `OOS_PASSWORD`로도 전달할 수 있습니다.

### 파일과 버전

| 명령어 | 설명 |
| :--- | :--- |
| `oos-lite init` | 저장소 초기화(암호 문구를 주면 암호화) |
| `oos-lite put <path> [--name <name>]` | 파일을 저장하거나 새 버전을 추가 |
| `oos-lite get <name\|id> <out> [--version N]` | 파일(최신 또는 지정 버전)을 디스크에 기록 |
| `oos-lite list` | 저장된 파일 목록 |
| `oos-lite versions <name\|id>` | 파일의 버전 기록 표시 |
| `oos-lite rollback <name> --version N [--out <path>]` | 이전 버전을 다시 최신으로 지정하고, 선택적으로 디스크에도 기록 |
| `oos-lite rm <name>` | 저장소에서 이름 제거(기록은 prune과 gc 전까지 유지) |
| `oos-lite prune [--keep N] [name]` | 최신 N개 버전만 유지(기본값 10) |

### 스냅샷

| 명령어 | 설명 |
| :--- | :--- |
| `oos-lite snapshot create <label>` | 스냅샷 생성 |
| `oos-lite snapshot list` | 스냅샷 목록 |
| `oos-lite snapshot restore <label> <dir>` | 스냅샷을 디렉터리로 복원 |
| `oos-lite snapshot delete <label>` | 스냅샷 삭제 |

### 유지보수

| 명령어 | 설명 |
| :--- | :--- |
| `oos-lite stats` | 논리 크기, 물리 크기, 절약 비율, 청크 수 |
| `oos-lite gc` | 참조되지 않는 청크의 공간 회수 |
| `oos-lite fsck` | 데이터 무결성 검증 |

### 앱, 마운트, 연동

| 명령어 | 설명 |
| :--- | :--- |
| `oos-lite app` | 데스크톱 앱 실행(별칭 `desktop`, `gui`) |
| `oos-lite ui [--host 127.0.0.1] [--port 3000] [--no-open]` | 웹 UI 시작 |
| `oos-lite mount [Z: \| /mnt/dir] [--webdav] [--port 8080] [--cache-mb 128]` | 저장소를 읽기 전용으로 마운트 |
| `oos-lite watch <dir> [--debounce-secs 3] [--cooldown-secs 60] [--throttle-ms 10]` | 폴더를 자동으로 버전 관리 |
| `oos-lite shell-ext enable\|disable\|status` | Windows 탐색기 오른쪽 클릭 메뉴 |
| `oos-lite stop` | 백그라운드 OOS-Lite 프로세스를 중지하고 3000/8080 포트 해제 |
| `oos-lite uninstall` | 프로세스를 중지하고 설치된 구성 요소 제거 |

전체 옵션은 `oos-lite <command> --help`로 확인하세요.

마운트된 드라이브에는 세 개의 폴더가 있습니다: `current/`(모든 파일의 최신 버전), `snapshots/<label>/`, 그리고 `history/<path>/<file>@v<N>`와 `@latest`. 자세한 내용은 [마운트 가이드](../MOUNT_GUIDE.md)(베트남어)를 참고하세요.

## 아키텍처

### 쓰기 경로

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

### 구성 요소

- **청크:** FastCDC가 잘라낸 변경 불가능한 바이트 구간. BLAKE3로 식별하고 CRC32C로 검증합니다.
- **세그먼트 저장소:** 추가 전용 파일(`segment_00000001.seg` 등)로, 약 256 MiB가 되면 새 파일로 넘어갑니다. 읽기 쪽은 파일 디스크립터 캐시를 사용합니다.
- **매니페스트:** 파일의 한 버전을 구성하는 청크의 순서 있는 목록.
- **ObjectId:** 편집이나 이름 변경 후에도 바뀌지 않는 128비트 하이브리드 논리 ID. 기록이 파일을 따라갑니다.
- **메타데이터:** [redb](https://crates.io/crates/redb)에 저장됩니다. 이름 인덱스, 객체 레코드, 매니페스트, 스냅샷, 감시기 상태를 포함합니다.
- **WAL:** redo 전용 로그. 레코드에는 저장소에 아직 없는 청크의 데이터만 담기므로 각 청크가 두 번 기록되지 않습니다.

### 충돌 안전성

- 메타데이터는 WAL 레코드가 영속화된 뒤에만 갱신됩니다. 복구 시 커밋된 레코드를 멱등적으로 다시 적용하고, 커밋 전에 매니페스트의 모든 청크가 영속화되었는지 확인합니다.
- 복구는 최신 세그먼트 끝의 미완료 쓰기만 잘라냅니다. 그 밖의 위치에서 발견된 손상은 오류로 보고하며 데이터는 건드리지 않습니다.
- GC는 압축된 세그먼트를 임시 디렉터리에 쓰고, 기존 세그먼트를 `.seg.old`로 이름을 바꾼 다음 새 세그먼트를 옮겨 놓습니다. 이 과정이 충돌로 중단되면 다음 시작 시 `.seg.old` 파일에서 복원합니다.

## 암호화

```bash
# 암호화된 저장소 생성 (암호 문구는 8자 이상)
oos-lite --password-file ~/.oos-password init

# 이후 모든 명령에 같은 암호 문구가 필요
oos-lite --password-file ~/.oos-password put backup.tar.gz
export OOS_PASSWORD='...'   # 다른 방법
```

`--password`는 셸 기록에 남으므로 `--password-file`(`chmod 600` 적용)을 사용하는 것이 좋습니다.

동작 방식: 무작위로 생성한 256비트 마스터 키가 XChaCha20-Poly1305로 청크 내용과 WAL 레코드를 암호화하며, 레코드마다 무작위 nonce를 사용합니다. 마스터 키는 100바이트 `vault.key` 파일에 감싸져 저장되며, 감싸는 키는 암호 문구에서 Argon2id(64 MiB, 3회 반복)로 유도됩니다. 기존의 암호화되지 않은 저장소를 암호화 모드로 바꿀 수는 없습니다.

**알아 두어야 할 제한 사항:**

- **메타데이터는 암호화되지 않습니다.** redb 데이터베이스의 파일 이름, 크기, 타임스탬프, 버전 기록, 스냅샷 레이블, 매니페스트는 평문으로 저장됩니다.
- **청크 ID는 내용에 대한 일반 BLAKE3 해시입니다.** 저장소를 읽을 수 있는 사람은 자신이 가진 파일을 청크로 나눠 해시를 비교하여, 그 파일이 저장소에 있는지 알아낼 수 있습니다.
- **암호 문구나 `vault.key`를 잃으면 데이터도 잃습니다.** 복구 수단은 없습니다.

암호화는 디스크를 분실하거나 신뢰할 수 없는 곳에 백업할 때 파일 내용을 보호하기 위한 것이지, 어떤 파일을 가지고 있는지 숨기기 위한 것이 아닙니다.

## 웹 UI와 공개 공유

```bash
oos-lite ui --port 3000   # http://127.0.0.1:3000 열기
oos-lite app              # 같은 대시보드를 데스크톱 창에서 열기
```

대시보드에는 파일 탐색기(목록, 트리, 그리드 보기), 미리 보기, 버전 기록, 스냅샷, 업로드(폴더 포함), 감시기 제어, GC/fsck가 있습니다.

UI 서버는 기본적으로 `127.0.0.1`에 바인딩되며, `Host`, `Origin`, `Sec-Fetch-Site` 헤더를 검사해 DNS 리바인딩과 교차 사이트 요청을 막습니다. **로그인 기능은 없습니다.** `0.0.0.0`에 바인딩하면 같은 네트워크의 누구나 사용할 수 있습니다.

**공개 공유는 기본으로 켜져 있습니다.** UI가 시작되면 OOS-Lite는 다음 작업도 함께 수행합니다.

1. 포트 `3001`에서 공유 서버를 시작하고, 모든 네트워크 인터페이스에서 수신합니다.
2. 해당 포트로 Cloudflare quick tunnel을 엽니다. 저장소 디렉터리에 `cloudflared`가 없으면 최신 버전을 자동으로 내려받습니다.

공유 링크는 무작위 토큰과 선택적인 만료 시간으로 보호됩니다. 공유가 필요 없다면 꺼 두세요.

```bash
OOS_DISABLE_PUBLIC_SHARING=1 oos-lite ui        # Linux / macOS
$env:OOS_DISABLE_PUBLIC_SHARING = "1"           # Windows PowerShell, 그다음 oos-lite ui 실행
```

## 디스크 저장 구조

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

redb로 옮기기 전 버전은 메타데이터를 sled에 저장했습니다. 새 빌드가 이런 저장소를 열면 `metadata.redb.staging`을 거쳐 sled 데이터를 가져오고, 원본 sled 파일은 그대로 두며, 이전 바이너리가 마이그레이션된 저장소를 열지 못하도록 차단 파일을 설치합니다. 마이그레이션이 중단되면 자동으로 이어서 진행합니다. `.redb-active`가 존재하는 상태에서 데이터베이스가 없거나 손상되었다면, 오래된 sled 데이터에서 조용히 다시 가져오지 않고 오류로 보고합니다.

저장소를 백업할 때는 디렉터리 전체를 복사하고, 상태 표시 파일은 절대 삭제하지 마세요.

## 프로젝트 구조

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

## 개발

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all
cargo bench --workspace                # Criterion 벤치마크
cargo run -p oos-lite-benchmarks       # 일반 복사 방식과 비교
```

릴리스 기록은 [release_notes.md](../../release_notes.md)와 [릴리스 페이지](https://github.com/pudo58/oos-lite/releases)에서 볼 수 있습니다.

## 기여하기

기여를 환영합니다. issue나 pull request를 열기 전에 [CONTRIBUTING.ko.md](CONTRIBUTING.ko.md)를 읽어 주세요. 보안 문제는 그 문서에 안내된 방법에 따라 비공개로 보고해 주세요.

OOS-Lite가 도움이 되었다면 [프로젝트를 후원](https://pudo58.github.io/oos-lite/support/)할 수도 있습니다.

## 라이선스

다음 중 하나를 선택하여 사용할 수 있는 이중 라이선스입니다.

- MIT License([LICENSE-MIT](../../LICENSE-MIT))
- Apache License, Version 2.0([LICENSE-APACHE](../../LICENSE-APACHE))
