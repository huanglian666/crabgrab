# CrabGrab 项目初始化设计

## 目标

完成 CrabGrab 仓库的基础 Git 初始化，使本地 Rust 项目与 GitHub 上已经创建的 `main` 分支形成一条可继续开发、可复现构建的提交历史。

## 范围

- 完善适用于 Rust 和跨平台协作的 `.gitignore`。
- 保留 `Cargo.lock`，因为 CrabGrab 是可执行程序。
- 从 `origin/main` 获取 GitHub 生成的 MIT `LICENSE` 文件及其已有历史。
- 将本地 Rust 初始工程合入远端历史。
- 格式化并验证当前工程。
- 提交初始化内容并推送到 `origin/main`。

本次不添加 README、第三方依赖、CLI 参数、业务模块或 CI 配置。

## `.gitignore` 规则

忽略以下本地或生成内容：

- Rust 构建产物，例如 `/target/`。
- macOS 和 Windows 生成的系统文件。
- JetBrains、RustRover 和 VS Code 的用户私有状态。
- `.env` 及其本地变体，但允许以后提交示例环境文件。
- 日志、交换文件、备份文件和常见临时文件。

不忽略 `Cargo.lock`，也不整体忽略所有编辑器目录中可能适合团队共享的配置。

## Git 历史整合

先读取远端引用，确认 `origin/main` 的实际提交和文件。由于本地 `main` 目前没有提交，优先以远端 `main` 为历史起点，再加入本地文件，避免制造互不相关的两段历史或使用强制推送。

如果远端状态与预期不同，例如除 MIT License 外还有其他文件或分支保护，停止写入并报告实际情况，不覆盖远端内容。

## 验证

推送前执行：

- `cargo fmt --check`
- `cargo check`
- `cargo test`
- 检查 Git 差异和待提交文件，确认没有构建产物或敏感环境文件。

验证通过后创建清晰的初始化提交，并以普通推送方式更新 `origin/main`。不使用强制推送。

## 完成标准

- 本地存在 GitHub 生成的 MIT `LICENSE`。
- `.gitignore` 覆盖约定的跨平台开发场景。
- Rust 工程格式、编译和测试检查通过。
- 初始化内容已经提交。
- 本地 `main` 与 `origin/main` 同步。
