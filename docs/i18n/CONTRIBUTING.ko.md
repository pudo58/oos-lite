# OOS-Lite에 기여하기

[English](../../CONTRIBUTING.md) | [简体中文](CONTRIBUTING.zh-CN.md) | [日本語](CONTRIBUTING.ja.md) | [Français](CONTRIBUTING.fr.md) | **한국어** | [Tiếng Việt](CONTRIBUTING.vi.md)

> 이 문서는 [영문 CONTRIBUTING](../../CONTRIBUTING.md)의 번역본입니다. 내용이 다를 경우 영문판을 기준으로 합니다.

OOS-Lite에 관심을 가져 주셔서 감사합니다. 버그 보고, 문서 수정, 테스트, 코드 모두 환영합니다. 이 가이드는 개발 환경을 준비하는 방법과 좋은 기여의 모습을 설명합니다.

## 기여 방법

- **버그 보고.** 운영체제, OOS-Lite 버전(`oos-lite --version`), 실행한 명령, 기대한 결과와 실제 결과를 적어 issue를 열어 주세요. 로그가 있으면 큰 도움이 됩니다. `RUST_LOG=debug`로 실행하면 더 자세한 정보를 얻을 수 있습니다.
- **기능 제안.** 코드를 작성하기 전에 해결하려는 문제를 issue로 먼저 설명해 주세요. 디스크 저장 형식이나 CLI 변경이라면 특히 그렇습니다.
- **문서 또는 번역 개선.** README와 CONTRIBUTING은 영어, 중국어, 일본어, 프랑스어, 한국어, 베트남어로 제공됩니다. 영문 파일이 원본이며, 번역본은 [`docs/i18n/`](.)에 있습니다.
- **pull request 보내기.** 작고 목적이 분명한 PR일수록 빨리 리뷰됩니다.

## 보안 문제 보고

웹 UI나 공유 서버를 통한 데이터 노출, 암호화 취약점, 경로 탐색(path traversal) 같은 보안 문제는 공개 issue로 **올리지 마세요**. 대신 GitHub의 [비공개 취약점 보고](https://github.com/pudo58/oos-lite/security/advisories/new)를 사용하고 재현 절차를 함께 적어 주세요. 공개하기 전에 메인테이너가 수정 사항을 배포할 수 있도록 충분한 시간을 주세요.

## 개발 환경 준비

필요한 것:

- `clippy`와 `rustfmt`가 포함된 최신 Rust 안정 버전 툴체인(`rustup component add clippy rustfmt`)
- C 링커(Windows에서는 MSVC 또는 MinGW)
- Linux에서 FUSE 지원을 빌드하려면 `libfuse3-dev`와 `pkg-config`

```bash
git clone https://github.com/<your-username>/oos-lite.git
cd oos-lite
cargo build --workspace
cargo test --workspace
```

워크스페이스는 세 개의 크레이트로 구성됩니다.

| 크레이트 | 경로 | 내용 |
| :--- | :--- | :--- |
| `oos-lite-core` | `core/` | 스토리지 엔진: 청크, 세그먼트, WAL, 메타데이터, GC, 암호화, 감시기 |
| `oos-lite` | `cli/` | CLI, 데스크톱 앱, 웹 UI, 마운트, 공유, Windows 연동 |
| `oos-lite-benchmarks` | `benchmarks/` | Criterion 벤치마크와 비교 도구 |

직접 테스트할 때는 `oos-lite --store-dir /tmp/oos-test ...`처럼 항상 버려도 되는 저장소를 사용해 실제 데이터를 건드리지 않도록 하세요. 로컬에서 UI를 실행할 때 공유가 필요 없다면 `OOS_DISABLE_PUBLIC_SHARING=1`을 설정하세요.

## pull request를 열기 전에

저장소 루트에서 다음을 실행하세요.

```bash
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test --workspace
```

CI는 Ubuntu와 Windows에서 `cargo check`와 `cargo test`를 실행합니다. 변경으로 인해 새로운 clippy 경고가 생기지 않았는지도 확인해 주세요.

### 스토리지 엔진 변경

`core/`는 사용자 데이터를 다루므로 변경할 때 특히 주의해야 합니다.

- **테스트를 추가하세요.** 버그 수정에는 회귀 테스트가 필요합니다(`core/tests/core_regressions.rs` 참고). 쓰기, 복구, GC 경로를 바꾼다면 `core/tests/kill_recovery.rs`의 충돌 테스트로 검증해야 합니다.
- **디스크 저장 형식의 호환성을 유지하세요.** 기존 저장소, 세그먼트, WAL 레코드, 메타데이터는 계속 동작해야 합니다. 형식 변경이 불가피하다면 먼저 issue에서 논의하고, `core/src/index/migration.rs`의 sled에서 redb로의 마이그레이션처럼 마이그레이션 경로와 테스트를 함께 제공하세요.
- **영속화 순서를 지키세요.** 데이터는 그것을 가리키는 메타데이터가 공개되기 전에 영속화(fsync)되어야 합니다.
- **비밀 정보를 로그에 남기지 마세요.** 암호 문구와 키는 로그, 오류 메시지, `Debug` 출력에 절대 나타나서는 안 됩니다.
- **`unwrap()`과 `expect()`를 피하세요.** 사용자 입력이나 디스크 데이터를 처리하는 테스트 외 코드에서는 `OosLiteError`를 반환하세요.

### CLI, UI, 네트워크 기능 변경

- UI 서버는 기본적으로 `127.0.0.1`에 바인딩된 상태를 유지하고, Host/Origin 검사를 그대로 두세요.
- 네트워크에서 수신하거나 코드를 내려받아 실행하는 기능은 PR 설명에서 명확히 설명해야 합니다.
- 명령어나 옵션을 바꿨다면 README(최소한 영문판)를 갱신하세요.

## 커밋 메시지

기존 기록과 마찬가지로 [Conventional Commits](https://www.conventionalcommits.org/)를 사용하세요.

```
feat(core): add keyed chunk IDs for encrypted stores
fix(watcher): retry files locked during startup
docs: update mount guide
```

자주 쓰는 타입: `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `ci`, `chore`. 커밋 하나에는 변경 하나만 담고, 여러 기능을 한 커밋에 묶지 마세요.

## pull request 절차

1. 저장소를 포크하고 `master`에서 브랜치를 만듭니다.
2. 테스트와 문서를 포함해 변경합니다.
3. 위와 같이 포맷, clippy, 테스트를 실행합니다.
4. 무엇을, 왜 바꿨고, 어떻게 테스트했는지 설명하는 pull request를 엽니다. 관련 issue가 있으면 연결합니다.
5. 리뷰 의견은 같은 브랜치에 새 커밋을 push하여 반영합니다.

## 번역

`README.md`나 `CONTRIBUTING.md`를 수정할 때 모든 번역본을 직접 갱신할 필요는 없습니다. PR에 번역본이 오래되었다고 적어 주면 원어민이 나중에 반영할 수 있습니다. 번역본을 갱신한다면 코드 블록, 명령어, 링크는 영문판과 동일하게 유지해 주세요.

## 라이선스

기여함으로써, 귀하의 기여물이 프로젝트와 동일하게 MIT License와 Apache License 2.0의 이중 라이선스로 추가 조건 없이 제공되는 데 동의하는 것으로 간주합니다.
