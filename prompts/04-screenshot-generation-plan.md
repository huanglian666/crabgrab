# CrabGrab 第四阶段：截图生成实施计划

> **执行要求：** 按任务顺序实施并使用测试驱动开发。直接在 `/Users/huanglian/Desktop/rust_code/crabgrab` 当前工作区工作；用户已明确要求立即执行，并要求人工验收前不得暂存或提交代码。

**目标：** 实现 `crabgrab sc -i <视频> -o <目录>`，按配置的固定与随机时间点生成 PNG 截图，并按外挂字幕、内封字幕、无字幕的顺序安全降级。

**架构：** Rust 负责配置、MediaInfo JSON 探测、时间轴、字幕选择、FFmpeg sidecar 校验与调用以及事务式目录安装。FFmpeg 作为独立预编译 sidecar，不链接进 Rust；所有外部进程通过可注入接口隔离，使普通测试无需真实媒体工具。

**技术栈：** Rust 2024、clap、serde/toml/serde_json、rand、thiserror、tempfile、SHA-256、`std::process::Command`、MediaInfo CLI、FFmpeg CLI。

## 全局约束

- 对外命令固定为 `sc`；`-o` 是资源总输出目录，图片位于 `<output>/screenshots/`。
- 默认生成 3 张；实际数量为 `max(count, timestamps.length)`，`count` 范围是 `1..=100`。
- 显式时间支持 `HH:MM:SS`、`HH:MM:SS.mmm`、百分比；内部使用毫秒，清单显示 `HH:MM:SS`。
- 默认使用视频 `5%..95%` 区间分区随机补点，固定点永不丢弃。
- 字幕默认开启，顺序为语言匹配外挂、严格同名外挂、内封默认/语言轨、第一可渲染内封轨、无字幕。
- 外挂格式优先级固定为 `ASS > SSA > SRT > VTT`。
- 输出固定为原分辨率无损 PNG，按时间升序编号。
- 非空且没有有效 `.crabgrab-screenshots` 标记的目录不得覆盖。
- 不搜索系统 `PATH`，不在运行时下载工具，不通过 shell 拼接命令。
- 常规 `cargo test` 不依赖真实 MediaInfo 或 FFmpeg。
- 用户验收前不运行 `git add`、`git commit` 或任何发布操作。

---

## 任务 1：配置解耦与截图配置

**文件：**

- 修改：`src/config.rs`
- 修改：`src/cli.rs`

**接口：**

```rust
pub struct ScreenshotConfig {
    pub count: usize,
    pub timestamps: Vec<String>,
    pub subtitles: bool,
    pub subtitle_languages: Vec<String>,
}

impl TmdbConfig {
    pub(crate) fn require_token(self, path: &Path) -> Result<Self, ConfigError>;
}
```

- [ ] 先增加测试，证明空 TMDB Token 的配置可被通用加载，并得到截图默认值 3、空时间点、开启字幕和默认中文语言顺序。
- [ ] 运行 `cargo test config::tests --lib`，确认测试因当前 `EmptyToken` 失败。
- [ ] 增加显式 `[screenshot]` 的解析测试，覆盖合法值、`count=0`、`count=101`、缺失 `[screenshot]` 和空语言数组。
- [ ] 实现带 serde 默认值的 `ScreenshotConfig`，把 Token 非空验证移到 TMDB 下载分支。
- [ ] 更新 `CONFIG_TEMPLATE`，加入已确认的 `[screenshot]` 示例。
- [ ] 运行 `cargo test config::tests --lib` 和现有 CLI/TMDB 测试。

验收：`sc` 可以读取空 TMDB Token 的配置，TMDB 下载仍拒绝空 Token且不泄露 Token。

## 任务 2：结构化媒体探测

**文件：**

- 修改：`src/media_info.rs`
- 修改：`src/media_info/process.rs`
- 新建：`src/media_info/probe.rs`
- 修改：`Cargo.toml`

**接口：**

```rust
pub struct MediaProbe {
    pub duration_ms: u64,
    pub has_video: bool,
    pub subtitles: Vec<SubtitleTrack>,
}

pub struct SubtitleTrack {
    pub stream_kind_position: usize,
    pub language: Option<String>,
    pub is_default: bool,
    pub format: String,
}

pub trait MediaProber {
    fn probe(&self, input: &Path) -> Result<MediaProbe, AnalyzeError>;
}
```

- [ ] 使用手写的完整 MediaInfo JSON 夹具增加失败测试，覆盖毫秒时长、视频轨、默认字幕、语言、字幕序号、无视频和无有效时长。
- [ ] 运行 `cargo test media_info::probe --lib`，确认模块或 API 不存在而失败。
- [ ] 实现仅反序列化所需字段的 JSON 结构，并把时长规范化为毫秒。
- [ ] 为假 sidecar 增加测试，证明 JSON 调用参数、特殊字符路径、非零退出和非 UTF-8错误受控。
- [ ] 扩展 `ProcessMediaAnalyzer` 实现 `MediaProber`，调用 `mediainfo --Output=JSON <input>`，复用现有哈希验证。
- [ ] 运行 `cargo test media_info --lib` 和全部现有测试。

验收：截图流程不解析 `mediainfo.txt`，可可靠取得时长和字幕轨元数据。

## 任务 3：时间格式与随机补点

**文件：**

- 新建：`src/screenshot.rs`
- 新建：`src/screenshot/timeline.rs`
- 修改：`src/lib.rs`
- 修改：`Cargo.toml`

**接口：**

```rust
pub enum TimestampSpec {
    Absolute(u64),
    Percent(f64),
}

pub fn parse_timestamp(value: &str) -> Result<TimestampSpec, TimelineError>;

pub fn build_timeline<R: rand::Rng + ?Sized>(
    duration_ms: u64,
    count: usize,
    configured: &[String],
    rng: &mut R,
) -> Result<Timeline, TimelineError>;

pub struct Timeline {
    pub points_ms: Vec<u64>,
    pub duplicate_count: usize,
    pub expanded_beyond_count: bool,
}
```

- [ ] 先测试绝对时间、毫秒、小数百分比、超过 24 小时以及非法分秒、负数、`0%`、`100%`。
- [ ] 运行 `cargo test screenshot::timeline --lib`，确认失败原因是 API 尚不存在。
- [ ] 实现时间解析和使用整数毫秒的百分比换算，避免浮点结果越界。
- [ ] 先测试 `timestamps<count`、`timestamps>count`、毫秒去重、排序、越界和三张默认分区随机。
- [ ] 使用可注入且带固定种子的 RNG 实现 `5%..95%` 分区补点、冲突重试和短视频间距收缩。
- [ ] 增加 `format_timestamp(ms) -> String` 测试，固定输出 `HH:MM:SS` 且允许小时超过 24。
- [ ] 运行时间轴聚焦测试和全部库测试。

验收：随机测试可复现，生产调用使用线程 RNG；固定时间点全部保留且输出有序。

## 任务 4：外挂与内封字幕选择

**文件：**

- 新建：`src/screenshot/subtitle.rs`

**接口：**

```rust
pub enum SubtitleSource {
    External(PathBuf),
    Embedded { stream_kind_position: usize },
}

pub fn select_subtitle(
    video: &Path,
    languages: &[String],
    tracks: &[SubtitleTrack],
) -> Result<Option<SubtitleSource>, SubtitleError>;
```

- [ ] 在临时目录创建真实文件并先测试 `Movie.zh-CN.ass`、`Movie.zh_CN.srt`、大小写语言和严格同名候选。
- [ ] 运行 `cargo test screenshot::subtitle --lib`，确认失败。
- [ ] 实现目录枚举、基础名称边界、语言归一化和 `ASS > SSA > SRT > VTT` 排序。
- [ ] 增加测试证明 `Movie-Trailer.srt`、`Movie.extra.srt` 和不支持扩展名不会误匹配。
- [ ] 先增加内封测试：默认且语言匹配、默认但语言不匹配、无默认时语言匹配、第一可渲染、无轨。
- [ ] 实现使用 `stream_kind_position` 的内封选择；不可渲染文本/图形格式名单以真实 FFmpeg 能力的保守集合判断。
- [ ] 运行字幕聚焦测试和全部库测试。

验收：选择结果完全由目录内容、语言优先级和 MediaInfo 轨道决定，不依赖操作系统枚举顺序。

## 任务 5：FFmpeg sidecar 与帧生成

**文件：**

- 新建：`src/screenshot/process.rs`
- 新建：`src/screenshot/tool.rs`
- 新建：`tools/ffmpeg-manifest.toml`
- 修改：`src/screenshot.rs`

**接口：**

```rust
pub trait FrameExtractor {
    fn extract(&self, request: &FrameRequest) -> Result<(), ExtractError>;
}

pub struct FrameRequest<'a> {
    pub input: &'a Path,
    pub output: &'a Path,
    pub timestamp_ms: u64,
    pub subtitle: Option<&'a SubtitleSource>,
}

pub struct ProcessFrameExtractor {
    executable: PathBuf,
    expected_sha256: String,
}
```

- [ ] 用可执行假脚本先测试参数为独立 OS 参数、时间保留毫秒、输出为一帧 PNG、禁止覆盖和无字幕调用。
- [ ] 运行 `cargo test screenshot::process --lib`，确认失败。
- [ ] 实现 sidecar 定位与 SHA-256 校验，复用或抽取当前 MediaInfo 工具校验的通用逻辑。
- [ ] 实现外挂路径的 FFmpeg filtergraph 转义，并用含中文、空格、冒号、反斜杠和单引号的字面量测试。
- [ ] 实现内封字幕序号参数、自动旋转、方形像素和 RGB PNG 输出参数。
- [ ] 为字幕滤镜失败定义可识别错误，使编排层能够按外挂、内封、无字幕重试；视频解码失败保持致命。
- [ ] 固定 macOS arm64 与 Windows x86_64 的预编译 FFmpeg 版本、下载 URL、包哈希、可执行文件哈希和许可证类型到清单，不使用 `latest` URL。
- [ ] 运行进程聚焦测试和全部库测试。

验收：普通测试不需要真实 FFmpeg，生产模式只运行哈希匹配的随包工具。

## 任务 6：事务式截图目录安装

**文件：**

- 新建：`src/screenshot/install.rs`
- 修改：`Cargo.toml`

**接口：**

```rust
pub struct ScreenshotManifest {
    pub ffmpeg_version: String,
    pub video: String,
    pub images: Vec<ManifestImage>,
}

pub struct ManifestImage {
    pub file: String,
    pub timestamp: String,
    pub subtitle: Option<String>,
}

pub fn install_screenshots(
    output_root: &Path,
    manifest: &ScreenshotManifest,
    generate: impl FnOnce(&Path) -> Result<(), ScreenshotError>,
) -> Result<PathBuf, InstallError>;
```

- [ ] 先测试不存在目录、空目录、有效所有权标记更新和非空无标记目录拒绝覆盖。
- [ ] 运行 `cargo test screenshot::install --lib`，确认失败。
- [ ] 实现同父目录唯一暂存目录、TOML 清单和按总数量确定的 PNG 编号。
- [ ] 先测试生成失败保留旧目录、提交失败恢复备份、数量减少不遗留旧图片。
- [ ] 实现备份、重命名提交、回滚和只清理本次拥有的临时路径。
- [ ] 运行安装聚焦测试和全部库测试。

验收：任务要么出现一套完整新截图，要么完整保留旧截图，不会混合或覆盖未知文件。

## 任务 7：截图工作流与 CLI

**文件：**

- 修改：`src/screenshot.rs`
- 修改：`src/cli.rs`
- 新建：`tests/screenshot_cli.rs`
- 修改：`tests/cli.rs`

**接口：**

```rust
pub fn generate_screenshots(
    prober: &impl MediaProber,
    extractor: &impl FrameExtractor,
    input: &Path,
    output: &Path,
    config: &ScreenshotConfig,
) -> Result<ScreenshotResult, ScreenshotError>;
```

- [ ] 先增加 clap 测试，证明 `sc -i/--input -o/--output` 可解析且缺少参数会失败。
- [ ] 运行 `cargo test screenshot --lib --test screenshot_cli`，确认失败。
- [ ] 使用记录请求并真实写入 PNG 占位字节的假 extractor，测试固定点加随机点、按时间排序、文件编号和清单时间格式。
- [ ] 实现 `generate_screenshots` 编排输入验证、探测、时间轴、字幕选择、逐帧生成和事务提交。
- [ ] 先测试外挂渲染失败回退内封、内封失败回退无字幕、无字幕解码失败时整体失败且保留旧目录。
- [ ] 实现回退链和警告集合；同一字幕来源一旦被证明不可用，后续帧直接使用已确定的降级来源。
- [ ] 把 `sc` 接入 CLI；只为该分支加载截图配置，生产注入 bundled MediaInfo 与 FFmpeg。
- [ ] 成功时打印 screenshots、generated、subtitle、timestamps；警告写入 stderr，字幕降级仍返回成功。
- [ ] 运行 `cargo test --all-targets --all-features`。

验收：完整 Rust 工作流可由假外部工具端到端验证，且不要求 TMDB Token。

## 任务 8：工具获取、许可证与最终验证

**文件：**

- 新建：`scripts/fetch-ffmpeg.sh`
- 视 Windows 开发需要新建：`scripts/fetch-ffmpeg.ps1`
- 新建：`licenses/FFmpeg.txt`
- 修改：发布工作流文件（如果仓库存在对应工作流）

- [ ] 实现显式下载脚本：读取固定清单、下载到临时目录、校验包哈希、只提取运行所需文件、校验可执行文件哈希、原子安装到 `.crabgrab-tools/ffmpeg/<target>/`。
- [ ] 用错误哈希和已有有效工具验证下载失败不会破坏旧工具；脚本不由 Cargo 自动调用。
- [ ] 记录实际 FFmpeg 构建的 LGPL/GPL 状态、来源、版本、构建配置和再分发要求；发布包带相应许可证材料。
- [ ] 若仓库已有发布工作流，将 FFmpeg 与 MediaInfo 一起装入 `tools/`，并校验便携包不回退系统 `PATH`。
- [ ] 运行 `cargo fmt --all -- --check`。
- [ ] 运行 `cargo clippy --all-targets --all-features -- -D warnings`。
- [ ] 运行 `cargo test --all-targets --all-features`。
- [ ] 运行 `cargo build --release`，确认 Cargo 未编译或链接 FFmpeg C/C++ 库。
- [ ] 使用可用的真实视频与 sidecar 执行人工冒烟测试；若工作区没有真实 FFmpeg，则明确列出用户验收命令，不伪造成功结论。
- [ ] 运行 `git diff --check` 与 `git status --short`，向用户交付修改、测试证据和待人工验收事项，不暂存、不提交。

## 计划自检

- 配置与 TMDB 解耦：任务 1。
- MediaInfo JSON 和轨道信息：任务 2。
- 固定、随机和显示时间：任务 3。
- 外挂语言后缀及内封回退：任务 4。
- FFmpeg 安全调用、哈希和字幕渲染：任务 5。
- PNG、清单、目录所有权和事务更新：任务 6。
- `sc` 命令与完整降级链：任务 7。
- 工具获取、许可、构建与最终验证：任务 8。
- 计划无占位符；所有生产行为均先由可观察的失败测试驱动。
- 用户已明确要求当前工作区直接执行，因此不创建 worktree；用户验收前不提交。
