# 为 OOS-Lite 做贡献

[English](../../CONTRIBUTING.md) | **简体中文** | [日本語](CONTRIBUTING.ja.md) | [Français](CONTRIBUTING.fr.md) | [한국어](CONTRIBUTING.ko.md) | [Tiếng Việt](CONTRIBUTING.vi.md)

> 本文译自[英文 CONTRIBUTING](../../CONTRIBUTING.md)。如两者内容不一致，以英文版为准。

感谢你对 OOS-Lite 的关注。我们欢迎错误报告、文档修正、测试和代码。本指南说明如何搭建开发环境，以及怎样的贡献更容易被接受。

## 贡献方式

- **报告错误。** 提交 issue 时请注明操作系统、OOS-Lite 版本（`oos-lite --version`）、执行的命令、预期结果和实际结果。日志很有帮助，可使用 `RUST_LOG=debug` 获取更多细节。
- **提出功能建议。** 在写代码之前先提交 issue 描述要解决的问题，尤其是涉及磁盘格式或命令行的改动。
- **改进文档或翻译。** README 和 CONTRIBUTING 提供英文、中文、日文、法文、韩文和越南文版本。英文版为准；译文位于 [`docs/i18n/`](.)。
- **提交 pull request。** 小而聚焦的 PR 会比大型 PR 更快得到审查。

## 报告安全问题

**不要**为安全问题（例如通过 Web UI 或分享服务器泄露数据、加密缺陷、路径穿越）提交公开 issue。请使用 GitHub 的[私密漏洞报告](https://github.com/pudo58/oos-lite/security/advisories/new)，并附上复现步骤。在公开披露之前，请给维护者留出合理的时间发布修复。

## 开发环境

你需要：

- 较新的 Rust 稳定版工具链，并安装 `clippy` 和 `rustfmt`（`rustup component add clippy rustfmt`）；
- C 链接器（Windows 上为 MSVC 或 MinGW）；
- 在 Linux 上构建 FUSE 支持需要 `libfuse3-dev` 和 `pkg-config`。

```bash
git clone https://github.com/<your-username>/oos-lite.git
cd oos-lite
cargo build --workspace
cargo test --workspace
```

工作区包含三个 crate：

| Crate | 路径 | 内容 |
| :--- | :--- | :--- |
| `oos-lite-core` | `core/` | 存储引擎：分块、分段、WAL、元数据、GC、加密、监视器 |
| `oos-lite` | `cli/` | 命令行、桌面应用、Web UI、挂载、分享、Windows 集成 |
| `oos-lite-benchmarks` | `benchmarks/` | Criterion 基准测试和对比工具 |

手动测试时请始终使用临时仓库，例如 `oos-lite --store-dir /tmp/oos-test ...`，避免碰到真实数据。在本地运行 UI 时，如不需要分享功能，请设置 `OOS_DISABLE_PUBLIC_SHARING=1`。

## 提交 pull request 之前

在仓库根目录运行：

```bash
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test --workspace
```

CI 会在 Ubuntu 和 Windows 上运行 `cargo check` 和 `cargo test`。也请确认你的改动没有引入新的 clippy 警告。

### 修改存储引擎

`core/` 保存着用户数据，因此修改时需要格外小心：

- **添加测试。** 修复缺陷需要附带回归测试（见 `core/tests/core_regressions.rs`）。对写入、恢复或 GC 流程的修改应由 `core/tests/kill_recovery.rs` 中的崩溃测试覆盖。
- **保持磁盘格式兼容。** 现有的仓库、分段、WAL 记录和元数据必须继续可用。如果确实需要修改格式，请先在 issue 中讨论，并提供迁移路径和测试，参考 `core/src/index/migration.rs` 中从 sled 到 redb 的迁移。
- **遵守持久化顺序。** 数据必须先持久化（fsync），之后才能发布指向它的元数据。
- **不要记录机密信息。** 口令和密钥绝不能出现在日志、错误信息或 `Debug` 输出中。
- **避免 `unwrap()` 和 `expect()`**：在处理用户输入或磁盘数据的非测试代码中，请返回 `OosLiteError`。

### 修改命令行、UI 或网络功能

- UI 服务器默认应继续绑定 `127.0.0.1`，并保留 Host/Origin 检查。
- 任何监听网络、或下载并执行代码的改动，都需要在 PR 描述中清楚说明。
- 如果修改了命令或选项，请更新 README（至少更新英文版）。

## 提交信息

请使用 [Conventional Commits](https://www.conventionalcommits.org/) 规范，与现有历史保持一致：

```
feat(core): add keyed chunk IDs for encrypted stores
fix(watcher): retry files locked during startup
docs: update mount guide
```

常用类型：`feat`、`fix`、`docs`、`test`、`refactor`、`perf`、`ci`、`chore`。每个提交只包含一项改动，避免把多个功能合并到一个提交中。

## Pull request 流程

1. Fork 仓库，并从 `master` 创建分支。
2. 完成改动，并附上测试和文档。
3. 按上文运行格式化、clippy 和测试。
4. 提交 pull request，说明改了什么、为什么改以及如何测试。如有相关 issue，请一并链接。
5. 根据审查意见，向同一分支推送新的提交。

## 翻译

修改 `README.md` 或 `CONTRIBUTING.md` 时，你不必亲自更新所有译文。在 PR 中注明译文已过时即可，母语使用者可以后续跟进。如果你更新了译文，请保持代码块、命令和链接与英文版一致。

## 许可证

提交贡献即表示你同意你的贡献以 MIT License 和 Apache License 2.0 双许可证发布，与本项目相同，且不附加任何其他条款或条件。
