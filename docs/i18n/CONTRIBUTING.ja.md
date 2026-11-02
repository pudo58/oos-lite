# OOS-Lite へのコントリビュート

[English](../../CONTRIBUTING.md) | [简体中文](CONTRIBUTING.zh-CN.md) | **日本語** | [Français](CONTRIBUTING.fr.md) | [한국어](CONTRIBUTING.ko.md) | [Tiếng Việt](CONTRIBUTING.vi.md)

> この文書は[英語版 CONTRIBUTING](../../CONTRIBUTING.md) の翻訳です。内容が異なる場合は英語版を優先してください。

OOS-Lite に関心を持っていただきありがとうございます。バグ報告、ドキュメントの修正、テスト、コードのいずれも歓迎します。このガイドでは、開発環境の準備と、良いコントリビュートの進め方を説明します。

## コントリビュートの方法

- **バグを報告する。** OS、OOS-Lite のバージョン（`oos-lite --version`）、実行したコマンド、期待した結果と実際の結果を書いて issue を作成してください。ログがあると助かります。`RUST_LOG=debug` を付けて実行すると詳しい情報が得られます。
- **機能を提案する。** コードを書く前に、解決したい問題を issue で説明してください。ディスク上のフォーマットや CLI の変更では特に重要です。
- **ドキュメントや翻訳を改善する。** README と CONTRIBUTING は英語、中国語、日本語、フランス語、韓国語、ベトナム語で提供しています。英語版が原本で、翻訳は [`docs/i18n/`](.) にあります。
- **pull request を送る。** 小さく焦点を絞った PR ほど早くレビューされます。

## セキュリティ問題の報告

Web UI や共有サーバー経由のデータ漏えい、暗号化の弱点、パストラバーサルなどのセキュリティ問題については、公開 issue を**作成しないでください**。代わりに GitHub の[非公開の脆弱性報告](https://github.com/pudo58/oos-lite/security/advisories/new)を使い、再現手順を添えてください。公開する前に、メンテナーが修正をリリースするための十分な時間をください。

## 開発環境の準備

必要なもの：

- `clippy` と `rustfmt` を含む最近の Rust 安定版ツールチェーン（`rustup component add clippy rustfmt`）
- C リンカー（Windows では MSVC または MinGW）
- Linux で FUSE サポートをビルドする場合は `libfuse3-dev` と `pkg-config`

```bash
git clone https://github.com/<your-username>/oos-lite.git
cd oos-lite
cargo build --workspace
cargo test --workspace
```

ワークスペースは 3 つのクレートで構成されています。

| クレート | パス | 内容 |
| :--- | :--- | :--- |
| `oos-lite-core` | `core/` | ストレージエンジン：チャンク、セグメント、WAL、メタデータ、GC、暗号化、ウォッチャー |
| `oos-lite` | `cli/` | CLI、デスクトップアプリ、Web UI、マウント、共有、Windows 連携 |
| `oos-lite-benchmarks` | `benchmarks/` | Criterion ベンチマークと比較ツール |

手動でテストするときは、`oos-lite --store-dir /tmp/oos-test ...` のように必ず使い捨てのストアを使い、実データに触れないようにしてください。ローカルで UI を動かす際、共有が不要なら `OOS_DISABLE_PUBLIC_SHARING=1` を設定してください。

## pull request を作成する前に

リポジトリのルートで次を実行してください。

```bash
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test --workspace
```

CI は Ubuntu と Windows で `cargo check` と `cargo test` を実行します。変更によって新しい clippy の警告が増えていないことも確認してください。

### ストレージエンジンの変更

`core/` はユーザーのデータを扱うため、変更には特に注意が必要です。

- **テストを追加する。** バグ修正には回帰テストが必要です（`core/tests/core_regressions.rs` を参照）。書き込み、リカバリ、GC の処理を変更する場合は、`core/tests/kill_recovery.rs` のクラッシュテストでカバーしてください。
- **ディスク上のフォーマットの互換性を保つ。** 既存のストア、セグメント、WAL レコード、メタデータは引き続き動作する必要があります。フォーマット変更が避けられない場合は、まず issue で議論し、`core/src/index/migration.rs` の sled から redb への移行のように、移行手段とテストを用意してください。
- **永続化の順序を守る。** データは、それを指すメタデータが公開される前に永続化（fsync）されていなければなりません。
- **秘密情報をログに出さない。** パスフレーズや鍵は、ログ、エラーメッセージ、`Debug` 出力に決して含めないでください。
- **`unwrap()` と `expect()` を避ける。** ユーザー入力やディスク上のデータを扱うテスト以外のコードでは、`OosLiteError` を返してください。

### CLI、UI、ネットワーク機能の変更

- UI サーバーは既定で `127.0.0.1` にバインドしたままにし、Host/Origin のチェックを維持してください。
- ネットワークで待ち受ける処理や、コードをダウンロードして実行する処理を追加する場合は、PR の説明で明確に説明してください。
- コマンドやオプションを変更した場合は、README（少なくとも英語版）を更新してください。

## コミットメッセージ

既存の履歴と同様に [Conventional Commits](https://www.conventionalcommits.org/) を使ってください。

```
feat(core): add keyed chunk IDs for encrypted stores
fix(watcher): retry files locked during startup
docs: update mount guide
```

よく使う type：`feat`、`fix`、`docs`、`test`、`refactor`、`perf`、`ci`、`chore`。1 つのコミットには 1 つの変更だけを含め、複数の機能をまとめないでください。

## pull request の流れ

1. リポジトリをフォークし、`master` からブランチを作成します。
2. テストとドキュメントを含めて変更を加えます。
3. 上記のとおりフォーマット、clippy、テストを実行します。
4. 何を、なぜ変更し、どのようにテストしたかを説明した pull request を作成します。関連する issue があればリンクしてください。
5. レビューの指摘には、同じブランチに新しいコミットを push して対応します。

## 翻訳

`README.md` や `CONTRIBUTING.md` を変更する際、すべての翻訳を自分で更新する必要はありません。翻訳が古くなっていることを PR に記載してもらえれば、ネイティブスピーカーが後から対応できます。翻訳を更新する場合は、コードブロック、コマンド、リンクを英語版と同じに保ってください。

## ライセンス

コントリビュートすることで、あなたの貢献がプロジェクトと同じく MIT License と Apache License 2.0 のデュアルライセンスで提供されることに、追加の条件なしで同意したものとみなされます。
