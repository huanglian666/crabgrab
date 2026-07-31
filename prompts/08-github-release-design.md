# GitHub Actions 自动发布设计

## 目标

推送 `v<semver>` Git Tag 时，自动测试并构建 macOS Apple Silicon 与 Windows x86_64 版本，组装包含 MediaInfo、FFmpeg、许可证和 README 的便携包，生成 SHA-256 校验文件并创建 GitHub Release。

## 流程

1. `build-macos` 在 `macos-14` 上测试并构建 ARM64 原生二进制。
2. `build-windows` 在 `windows-latest` 上测试并构建 x86_64 原生二进制。
3. 两个构建产物作为 Actions artifact 传给 `release` job。
4. `release` job 在 macOS 上使用仓库现有脚本，为两个目标下载并校验固定版本的 MediaInfo 与 FFmpeg。
5. Tag 版本必须与 `Cargo.toml` 的 package version 相等。
6. 组装 macOS `.tar.gz` 和 Windows `.zip`，Windows 包包含 MediaInfo 所需的 `LIBCURL.DLL`。
7. 为两个压缩包生成 `SHA256SUMS.txt`。
8. 使用仓库自动提供的 `GITHUB_TOKEN` 和 GitHub CLI 创建 Release，并自动生成 Release Notes。

## 安全与失败规则

- Workflow 仅授予 `contents: write`，不使用个人 Token。
- 使用 `--verify-tag`，只发布已经推送的 Tag。
- 测试、工具下载、SHA-256 校验、版本一致性或打包任一步失败时，不创建 Release。
- 运行时工具只来自当前 `tools/*-manifest.toml` 固定的地址与哈希。
- 发布包保留主程序预期的 `tools/` 相对路径，不要求用户配置系统 `PATH`。

## 发布产物

以 `v0.1.0` 为例：

```text
crabgrab-0.1.0-macos-arm64.tar.gz
crabgrab-0.1.0-windows-x86_64.zip
SHA256SUMS.txt
```

## 验证

- YAML 可以被解析。
- Workflow 包含 Tag 触发、最小写权限、两个构建 job 和依赖它们的 release job。
- macOS 与 Windows 发布目录包含主程序、两个 sidecar、项目许可证、第三方许可证和 README。
- Windows 发布目录额外包含 `LIBCURL.DLL`。
- Tag 与 Cargo 版本不一致时显式失败。
- 本地完整 Rust 测试仍通过。
