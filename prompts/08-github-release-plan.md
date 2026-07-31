# GitHub Actions Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 通过 `v*` Tag 自动构建、打包并发布 CrabGrab 的 macOS ARM64 和 Windows x86_64 便携包。

**Architecture:** 两个平台分别在原生 GitHub-hosted runner 上测试和构建；独立的 macOS release job 下载二进制 artifact，并复用现有脚本下载两个目标的固定 sidecar。发布 job 校验版本、组装目录、生成压缩包和校验和，最后用 GitHub CLI 创建 Release。

**Tech Stack:** GitHub Actions、Rust stable、GitHub CLI、现有 POSIX shell 工具脚本。

## Global Constraints

- 只在推送 `v*` Tag 时发布。
- Tag 去掉 `v` 后必须等于 `Cargo.toml` package version。
- macOS 使用 `macos-14` ARM64 runner；Windows 使用 `windows-latest` x86_64 runner。
- Release 包必须内置 MediaInfo、FFmpeg、README 和许可证。
- Windows 包必须额外包含 `LIBCURL.DLL`。
- 只授予 `contents: write`，使用 `${{ github.token }}`，不新增仓库 Secret。

---

### Task 1: 双平台测试与构建

**Files:**
- Create: `.github/workflows/release.yml`

**Interfaces:**
- Produces: `crabgrab-macos-binary` 和 `crabgrab-windows-binary` Actions artifacts。

- [ ] **Step 1: 定义触发器和权限**

设置 `push.tags: ["v*"]` 与 `permissions.contents: write`。

- [ ] **Step 2: 添加 macOS 构建 job**

在 `macos-14` 上 checkout、更新 Rust stable、执行 `cargo test` 和 `cargo build --release`，上传 `target/release/crabgrab`。

- [ ] **Step 3: 添加 Windows 构建 job**

在 `windows-latest` 上执行同样的测试与 release 构建，上传 `target/release/crabgrab.exe`。

### Task 2: 便携包组装与 Release

**Files:**
- Modify: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: Task 1 的两个二进制 artifacts。
- Produces: 两个平台压缩包、`SHA256SUMS.txt` 和 GitHub Release。

- [ ] **Step 1: 下载二进制并校验版本**

release job 依赖两个 build job；下载 artifacts，并比较 `${GITHUB_REF_NAME#v}` 与 `Cargo.toml` 第一项 package version。

- [ ] **Step 2: 准备两个平台的 sidecar**

分别以 `aarch64-apple-darwin` 和 `x86_64-pc-windows-msvc` 调用现有 MediaInfo/FFmpeg 脚本，让脚本负责来源和 SHA-256 校验。

- [ ] **Step 3: 组装目录和压缩包**

复制主程序、sidecar、Windows `LIBCURL.DLL`、README、项目许可证和第三方许可证；macOS 生成 `.tar.gz`，Windows 生成 `.zip`。

- [ ] **Step 4: 生成校验和并发布**

用 `shasum -a 256` 生成 `SHA256SUMS.txt`，再以 `gh release create --verify-tag --generate-notes` 上传三个文件。

### Task 3: 静态与回归验证

**Files:**
- Verify: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: 完整 workflow。
- Produces: 可提交的、语法有效且与当前项目布局一致的发布配置。

- [ ] **Step 1: 解析 YAML**

Run: `ruby -e 'require "yaml"; YAML.parse_file(".github/workflows/release.yml")'`

Expected: 退出码为零。

- [ ] **Step 2: 检查关键发布路径**

确认运行时目标文件名为 `tools/mediainfo[.exe]` 与 `tools/ffmpeg[.exe]`；Windows 同目录包含 `LIBCURL.DLL`。

- [ ] **Step 3: 运行回归验证**

Run: `cargo test`

Expected: 全部通过。

Run: `git diff --check`

Expected: 无输出。
