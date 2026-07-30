# CrabGrab 便携式 MediaInfo 报告实施计划

> **执行要求：** 按任务顺序实施并使用测试驱动开发。直接在 `/Users/huanglian/Desktop/rust_code/crabgrab` 工作，不创建 Git worktree。用户手动验收前不得暂存或提交代码。

**目标：** 实现 `crabgrab mediainfo -i <文件> -o <目录>`，驱动随便携包分发的固定版本 MediaInfo CLI，生成英文标准文本 `mediainfo.txt`。

**架构：** Rust 负责 CLI、文件验证、辅助程序定位与完整性检查、子进程调用、报告验证和事务式写入。MediaInfo 使用官方预编译程序，不进入 Git，不参与 Cargo 编译。

**技术栈：** Rust 2024、clap、thiserror、tempfile、SHA-256、`std::process::Command`、GitHub Actions、MediaInfo CLI。

## 全局约束

- 不使用 C++、CMake、FFI、MediaInfoLib 源码、静态链接、`build.rs` 或 Git submodule。
- 不修改现有 TMDB 顶层参数、配置、版本和帮助行为。
- MediaInfo 子命令不读取 TMDB 配置、不访问网络。
- 只使用 CrabGrab 便携包中的固定 MediaInfo；不搜索 `PATH`，不自动下载。
- 普通 `cargo build` 和 `cargo test` 不下载或启动真实 MediaInfo。
- 报告使用英文标准文本，不使用 `--Full` 或 `--Language=raw`。
- 输出统一为 UTF-8、LF，并以一个换行结束。
- 已有报告安全覆盖，失败时保留或恢复旧报告。
- 正式目标为 Windows x86_64 和 macOS arm64；Linux 延后。
- MediaInfo 二进制、下载包和提取目录必须被 Git 忽略。
- 每完成一个任务运行聚焦测试；最终统一验证，但不自动提交。

---

## 任务 1：报告工作流与事务式写入

**文件：**

- 新建：`src/media_info.rs`
- 新建：`src/media_info/install.rs`
- 修改：`src/lib.rs`

**接口：**

```rust
pub trait MediaAnalyzer {
    fn analyze(&self, input: &Path) -> Result<String, AnalyzeError>;
}

pub fn generate_report(
    analyzer: &impl MediaAnalyzer,
    input: &Path,
    output: &Path,
) -> Result<PathBuf, MediaInfoError>;
```

- [ ] 编写失败测试：输入不存在、输入为目录、无扩展名输入、输出目录创建、空报告、只有 `General`、CRLF、末尾换行、旧报告覆盖、分析失败保留旧报告。
- [ ] 运行 `cargo test media_info --lib`，确认测试先失败。
- [ ] 实现输入文件可读性验证、输出目录验证和创建。
- [ ] 验证报告包含 `General`，并至少包含一个实际媒体分区。
- [ ] 使用同目录唯一临时文件完整写入、刷新和同步报告。
- [ ] 通过唯一备份路径替换旧报告；替换失败时尝试回滚并保留双重错误。
- [ ] 运行 `cargo test media_info --lib` 和 `cargo test`。

验收：假的分析器可以独立验证全部 Rust 工作流，不需要 MediaInfo 二进制。

## 任务 2：MediaInfo CLI 子命令

**文件：**

- 修改：`src/cli.rs`
- 修改：`tests/cli.rs`
- 新建：`tests/mediainfo_cli.rs`

- [ ] 编写短参数、长参数及缺失参数的失败测试。
- [ ] 编写注入假分析器的分派测试，证明命令不需要 TMDB 配置或 HTTP 服务。
- [ ] 运行 `cargo test --test mediainfo_cli`，确认测试先失败。
- [ ] 增加 `mediainfo -i/--input -o/--output` 子命令。
- [ ] 仅做支持服务注入所需的最小分派重构。
- [ ] 成功时输出最终 `mediainfo.txt` 路径，失败时通过统一错误返回非零状态。
- [ ] 运行 `cargo test --test mediainfo_cli --test cli --test tmdb_cli`。

验收：现有 TMDB、配置、版本和帮助行为不变。

## 任务 3：固定版本工具清单

**文件：**

- 新建：`tools/mediainfo-manifest.toml`
- 新建：`src/media_info/tool.rs`
- 修改：`Cargo.toml`
- 修改：`.gitignore`

工具清单必须记录 MediaInfo 版本，以及每个平台的目标三元组、官方 URL、原始包 SHA-256、提取后程序 SHA-256和程序名。

- [ ] 先为清单解析、目标选择、缺失文件、目录伪装、哈希不匹配和成功校验编写失败测试。
- [ ] 选择并固定 MediaInfo 官方稳定版本，不使用 `latest` URL。
- [ ] 实现清单解析和编译目标映射，只接受两个正式目标。
- [ ] 增加 SHA-256 依赖并流式计算辅助程序哈希。
- [ ] 校验程序是普通文件；macOS 同时校验可执行权限。
- [ ] 将 `.crabgrab-tools/`、下载包和提取临时目录加入 `.gitignore`。
- [ ] 运行 `cargo test media_info::tool --lib`。

验收：辅助程序缺失或被修改时拒绝运行，不产生 `mediainfo.txt`。

## 任务 4：显式下载与提取开发工具

**文件：**

- 新建：`scripts/fetch-mediainfo.sh`
- 视 Windows 开发需要新建：`scripts/fetch-mediainfo.ps1`
- 新建：`licenses/MediaInfo.txt`

- [ ] 下载脚本从工具清单读取当前或指定目标平台信息。
- [ ] 所有内容先下载到项目内或系统临时目录，不直接覆盖有效工具。
- [ ] 下载完成后先校验官方包 SHA-256。
- [ ] Windows 从官方 ZIP 只提取 CLI 和许可证材料。
- [ ] macOS 挂载或解开官方 CLI DMG，只复制已编译的 `mediainfo` 和许可证材料，不执行安装脚本。
- [ ] 校验提取后程序 SHA-256，并为 macOS 设置可执行权限。
- [ ] 原子安装到 `.crabgrab-tools/mediainfo/<目标三元组>/`，失败时清理本次临时内容。
- [ ] 支持重复执行；版本和哈希一致时直接成功。
- [ ] 用错误哈希测试下载失败路径，确认旧工具不受影响。

验收：脚本只下载和提取，不调用编译器，不由 Cargo 自动执行。

## 任务 5：安全的 MediaInfo 子进程分析器

**文件：**

- 新建：`src/media_info/process.rs`
- 修改：`src/media_info.rs`
- 修改：`src/cli.rs`

**接口：**

```rust
pub struct ProcessMediaAnalyzer {
    executable: PathBuf,
}

impl MediaAnalyzer for ProcessMediaAnalyzer { /* ... */ }
```

- [ ] 使用可控的假辅助程序编写失败测试：参数原样传递、成功输出、启动失败、非零退出、空输出、非 UTF-8、标准错误截断。
- [ ] 使用 `std::process::Command` 直接传参，禁止 Shell 和命令字符串拼接。
- [ ] 每次只传入一个输入路径，不启用 `--Full` 或 `--Language=raw`。
- [ ] 清理可能改变输出语言的环境变量，捕获标准输出和标准错误。
- [ ] 对退出码和输出协议做严格验证，将错误映射为 `AnalyzeError`。
- [ ] 生产模式从 `current_exe()/tools` 定位程序；测试和开发入口通过构造参数显式注入 `.crabgrab-tools` 路径。
- [ ] 先完成任务 3 的完整性校验，再允许启动子进程。
- [ ] 运行 `cargo test media_info::process --lib` 和全部 Rust 测试。

验收：中文、空格和特殊字符路径作为单个操作系统参数传递，不存在 Shell 注入面。

## 任务 6：真实 sidecar 集成测试与许可证

**文件：**

- 新建：`tests/fixtures/sample.mp4`
- 新建：`tests/fixtures/README.md`
- 新建：`tests/sidecar_mediainfo.rs`
- 修改：`licenses/MediaInfo.txt`

- [ ] 加入体积很小、来源和再分发条款明确的媒体样本，并记录 SHA-256。
- [ ] 真实测试仅在显式设置测试工具路径或发布 CI 中启用；工具缺失时普通测试不得下载。
- [ ] 验证报告包含 `General`、`Video`、`Format` 和 `Complete name`。
- [ ] 将样本复制到包含中文、空格和特殊字符的路径后再次验证。
- [ ] 验证空文件和文本文件产生受控错误。
- [ ] 验证分析失败不覆盖已有报告。
- [ ] 核对 MediaInfo 二进制再分发许可证以及实际随官方 CLI 包提供的第三方声明。

验收：测试机器不安装系统 MediaInfo，也能通过随测试环境准备的固定 sidecar 完成分析。

## 任务 7：GitHub Actions 便携发布包

**文件：**

- 新建或修改：`.github/workflows/ci.yml`
- 新建或修改：`.github/workflows/release.yml`

- [ ] 配置 Windows x86_64 与 macOS arm64 矩阵。
- [ ] Rust 单元测试阶段不下载 MediaInfo。
- [ ] sidecar 集成阶段按清单从官方 URL 下载并校验原始包。
- [ ] 提取程序后再次校验程序哈希，再运行真实测试。
- [ ] 执行格式检查、Clippy、全部测试和 Rust release 构建。
- [ ] 组装 `crabgrab + tools/mediainfo[.exe] + licenses/MediaInfo.txt`。
- [ ] Windows 生成平台标识 ZIP；macOS 生成平台标识 TAR.GZ。
- [ ] 在未安装系统 MediaInfo 的环境运行便携包冒烟测试。
- [ ] 临时移走 sidecar，确认 CrabGrab 明确失败且不回退到 `PATH`。
- [ ] 为发布压缩包生成 SHA-256 文件并上传 GitHub Release。

验收：整个发布流程不安装 CMake、不编译 MediaInfo 源码。

## 任务 8：最终验证与人工验收交接

- [ ] 运行 `cargo fmt --all -- --check`。
- [ ] 运行 `cargo clippy --all-targets --all-features -- -D warnings`。
- [ ] 运行 `cargo test --all-targets --all-features`。
- [ ] 运行 `cargo build --release`，确认构建日志没有 CMake、C++ 或 MediaInfoLib 编译。
- [ ] 使用用户提供的视频执行 macOS 便携目录下的 CLI 冒烟测试。
- [ ] 检查 `mediainfo.txt` 的英文标准字段、UTF-8、LF 和安全覆盖。
- [ ] 检查发布目录中不存在 MediaInfo 源码、头文件或动态构建缓存。
- [ ] 运行 `git diff --check` 和 `git status --short`。
- [ ] 向用户列出修改、测试证据和手动验收命令；不暂存、不提交。

## 计划自检

- CLI 与 TMDB 隔离：任务 1、2。
- 固定版本、官方来源和完整性：任务 3、4。
- 预编译 sidecar 驱动：任务 5。
- Unicode 路径、真实报告和许可证：任务 6。
- 双平台便携发布：任务 7。
- 性能和回归验证：任务 8。
- 计划中不存在 MediaInfoLib 源码编译、C++ 桥接、CMake、FFI、系统 `PATH` 回退、运行时下载或安装器。
