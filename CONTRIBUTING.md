# Contributing to OOS-Lite

**English** | [简体中文](docs/i18n/CONTRIBUTING.zh-CN.md) | [日本語](docs/i18n/CONTRIBUTING.ja.md) | [Français](docs/i18n/CONTRIBUTING.fr.md) | [한국어](docs/i18n/CONTRIBUTING.ko.md) | [Tiếng Việt](docs/i18n/CONTRIBUTING.vi.md)

Thanks for your interest in OOS-Lite. Bug reports, documentation fixes, tests, and code are all welcome. This guide explains how to get set up and what a good contribution looks like.

## Ways to contribute

- **Report a bug.** Open an issue with your OS, OOS-Lite version (`oos-lite --version`), the commands you ran, what you expected, and what happened. Logs help; run with `RUST_LOG=debug` to get more detail.
- **Suggest a feature.** Open an issue describing the problem you want to solve before writing code, especially for changes to the on-disk format or the CLI.
- **Improve documentation or translations.** README and CONTRIBUTING are available in English, Chinese, Japanese, French, Korean, and Vietnamese. The English files are the source of truth; translations live in [`docs/i18n/`](docs/i18n/).
- **Send a pull request.** Small, focused PRs are reviewed faster than large ones.

## Reporting security issues

Do **not** open a public issue for security problems such as data exposure through the Web UI or sharing server, encryption weaknesses, or path traversal. Use GitHub's [private vulnerability reporting](https://github.com/pudo58/oos-lite/security/advisories/new) instead, and include steps to reproduce. Please give the maintainer reasonable time to release a fix before disclosing publicly.

## Development setup

You need:

- a recent stable Rust toolchain with `clippy` and `rustfmt` (`rustup component add clippy rustfmt`);
- a C linker (MSVC or MinGW on Windows);
- on Linux, `libfuse3-dev` and `pkg-config` to build FUSE support.

```bash
git clone https://github.com/<your-username>/oos-lite.git
cd oos-lite
cargo build --workspace
cargo test --workspace
```

The workspace has three crates:

| Crate | Path | Contents |
| :--- | :--- | :--- |
| `oos-lite-core` | `core/` | Storage engine: chunks, segments, WAL, metadata, GC, encryption, watcher |
| `oos-lite` | `cli/` | CLI, desktop app, Web UI, mounts, sharing, Windows integration |
| `oos-lite-benchmarks` | `benchmarks/` | Criterion benchmarks and comparison runner |

Always use a throwaway store when testing by hand, for example `oos-lite --store-dir /tmp/oos-test ...`, so you never touch real data. Set `OOS_DISABLE_PUBLIC_SHARING=1` when running the UI locally if you do not need sharing.

## Before you open a pull request

Run these from the repository root:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test --workspace
```

CI runs `cargo check` and `cargo test` on Ubuntu and Windows. Please also make sure your change does not add new clippy warnings.

### Changes to the storage engine

`core/` holds user data, so changes there need extra care:

- **Add tests.** Bug fixes need a regression test (see `core/tests/core_regressions.rs`). Changes to the write, recovery, or GC paths should be covered by crash tests in `core/tests/kill_recovery.rs`.
- **Keep the on-disk format compatible.** Existing stores, segments, WAL records, and metadata must keep working. If a format change is unavoidable, discuss it in an issue first and include a migration path and tests, like the sled-to-redb migration in `core/src/index/migration.rs`.
- **Respect the durability order.** Data must be durable (fsynced) before the metadata that points to it is published.
- **Do not log secrets.** Passphrases and keys must never appear in logs, errors, or `Debug` output.
- **Avoid `unwrap()` and `expect()`** in non-test code paths that handle user input or disk data. Return an `OosLiteError` instead.

### Changes to the CLI, UI, or networking

- Keep the UI server bound to `127.0.0.1` by default and keep the Host/Origin checks in place.
- Anything that listens on the network or downloads and runs code needs a clear explanation in the PR description.
- If you change commands or options, update the README (at least the English version).

## Commit messages

Use [Conventional Commits](https://www.conventionalcommits.org/), as in the existing history:

```
feat(core): add keyed chunk IDs for encrypted stores
fix(watcher): retry files locked during startup
docs: update mount guide
```

Common types: `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `ci`, `chore`. Keep each commit focused on one change; avoid combining several features in a single commit.

## Pull request process

1. Fork the repository and create a branch from `master`.
2. Make your change, with tests and docs.
3. Run format, clippy, and tests as shown above.
4. Open a pull request that explains what changed, why, and how you tested it. Link the related issue if there is one.
5. Address review feedback by pushing new commits to the same branch.

## Translations

When you change `README.md` or `CONTRIBUTING.md`, you do not need to update every translation yourself. Mention in the PR that the translations are out of date, and native speakers can follow up. If you do update a translation, keep code blocks, commands, and links identical to the English version.

## License

By contributing, you agree that your contributions will be dual-licensed under the MIT License and the Apache License 2.0, the same as the project, without any additional terms or conditions.
