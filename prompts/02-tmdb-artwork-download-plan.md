# CrabGrab 第二阶段：TMDB 图片下载实施计划

> **供智能代理执行：** 必须使用 `superpowers:subagent-driven-development`（推荐）或 `superpowers:executing-plans` 技能逐项实施。所有任务使用复选框跟踪；所有生产代码必须遵循测试驱动开发，先观察测试因目标行为缺失而失败，再写最小实现。

**目标：** 实现 `crabgrab -i tmdb-{movie|tv}-<id> -o <父目录>`，从 TMDB 下载一张 original backdrop 和一张 original poster，并通过双路径配置安全管理用户 Token。

**架构：** 将 CLI、资源 ID、配置、提供方策略、TMDB 协议和文件事务拆成独立模块。CLI 将结构化资源 ID 分派到 `ArtworkProvider`；当前只有 `TmdbProvider`。图片先完整下载到目标目录中的临时文件，两张均成功后再通过备份、重命名和回滚更新正式文件。

**技术栈：** Rust 2024、clap derive、reqwest blocking + rustls、serde、toml、directories、tempfile、thiserror；测试使用 Rust 内置测试、集成测试、临时目录和本地 HTTP 模拟服务。

## 全局约束

- 当前工作直接保留在 `main` 工作区，不自动暂存或提交；每个任务末尾只检查差异，由用户人工提交。
- 只接受 `tmdb-movie-<正整数>` 和 `tmdb-tv-<正整数>`；`tv` 覆盖电视剧和综艺。
- CLI 同时支持 `-i/--id` 与 `-o/--output`，两者必须同时提供；每次只处理一个 ID。
- 保留现有 `-v`、`--version`、`-h`、`--help` 和无参数帮助行为。
- 配置优先读取可执行文件同级 `config.toml`，其次读取系统配置目录的 `crabgrab/config.toml`；两份配置不合并。
- `config init` 不覆盖已有配置；优先在可执行文件同级创建，不可写时回退系统配置目录。
- TMDB Token 只能从配置文件读取，不得出现在命令参数、源码、日志或错误输出中。
- 图片使用 TMDB `original` 尺寸，固定保存为 `background/background.jpg` 和 `cover/cover.jpg`。
- backdrop 或 poster 任一缺失或下载失败时，正式文件不得发生部分更新。
- 自动化测试不得访问公网或使用真实 TMDB Token。
- 不实现 IMDb、豆瓣、批量任务、缓存、日志、图片处理、影片元数据、MediaInfo、FFmpeg、README、CI 或发布流程。

## 文件结构

- 修改：`Cargo.toml`、`Cargo.lock`、`src/main.rs`、`tests/cli.rs`
- 创建：`src/lib.rs`、`src/domain.rs`、`src/domain/resource_id.rs`、`src/config.rs`、`src/providers.rs`、`src/providers/tmdb.rs`、`src/artwork.rs`、`src/artwork/download.rs`、`src/cli.rs`
- 创建：`tests/tmdb_cli.rs`
- 保留：`prompts/02-tmdb-artwork-download-design.md`
- 使用本计划：`prompts/02-tmdb-artwork-download-plan.md`

---

### 任务一：建立库边界并解析统一资源 ID

**涉及文件：**

- 修改：`Cargo.toml`
- 创建：`src/lib.rs`
- 创建：`src/domain.rs`、`src/domain/resource_id.rs`

**接口：**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind { Tmdb }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType { Movie, Tv }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceId {
    pub provider: ProviderKind,
    pub media_type: MediaType,
    pub numeric_id: u64,
}

impl std::str::FromStr for ResourceId {
    type Err = ResourceIdError;
}
```

- [ ] **步骤 1：添加本阶段依赖声明**

在 `Cargo.toml` 中加入运行依赖 `reqwest`（关闭默认 TLS，启用 `blocking`、`json`、`rustls-tls`）、`serde` derive、`toml`、`directories`、`tempfile` 和 `thiserror`；开发依赖加入本地 HTTP 模拟库。不要手工编辑 `Cargo.lock`。

- [ ] **步骤 2：先写资源 ID 失败测试**

在 `src/domain/resource_id.rs` 的测试模块覆盖：

```rust
assert_eq!("tmdb-movie-550".parse::<ResourceId>().unwrap().media_type, MediaType::Movie);
assert_eq!("tmdb-tv-1399".parse::<ResourceId>().unwrap().media_type, MediaType::Tv);
for invalid in ["imdb-movie-550", "tmdb-show-1", "tmdb-movie-0", "tmdb-movie-x", "tmdb-550", "tmdb-movie-1-extra"] {
    assert!(invalid.parse::<ResourceId>().is_err(), "{invalid}");
}
```

- [ ] **步骤 3：运行 RED 测试**

执行 `cargo test resource_id --lib`，确认测试因类型或解析实现缺失而失败，而不是语法或依赖错误。

- [ ] **步骤 4：实现最小解析器并导出模块**

严格按三个 `-` 分隔部分解析；仅接受 `tmdb`、`movie|tv` 和大于零的 `u64`。错误信息必须包含接受格式 `tmdb-movie-550` 与 `tmdb-tv-1399`，但不得触发网络请求。

- [ ] **步骤 5：运行 GREEN 测试与格式检查**

执行：

```bash
cargo test resource_id --lib
cargo fmt --check
git diff --check
```

预期：资源 ID 测试全部通过，无格式或空白错误。

---

### 任务二：实现双路径配置读取和安全初始化

**涉及文件：**

- 创建：`src/config.rs`
- 修改：`src/lib.rs`

**接口：**

```rust
pub struct ConfigPaths {
    pub portable: PathBuf,
    pub system: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig { pub tmdb: TmdbConfig }

#[derive(Debug, Clone, Deserialize)]
pub struct TmdbConfig {
    pub api_token: String,
    #[serde(default = "default_language")]
    pub language: String,
}

pub struct LoadedConfig { pub path: PathBuf, pub value: AppConfig }

pub fn resolve_config_paths(executable: &Path) -> Result<ConfigPaths, ConfigError>;
pub fn load_config(paths: &ConfigPaths) -> Result<LoadedConfig, ConfigError>;
pub fn init_config(paths: &ConfigPaths) -> Result<PathBuf, ConfigError>;
```

- [ ] **步骤 1：写配置读取失败测试**

使用临时目录构造 portable/system 路径，覆盖：portable 优先、portable 缺失时读取 system、portable 无效时不回退、缺省语言为 `zh-CN`、Token 缺失或全空白失败、错误文本不包含测试 Token。

- [ ] **步骤 2：运行配置读取 RED 测试**

执行 `cargo test config::tests::load --lib`，确认因配置函数缺失而失败。

- [ ] **步骤 3：实现路径解析、TOML 读取与验证**

portable 路径是 `current_exe()` 的父目录加 `config.toml`；system 路径通过 `directories::ProjectDirs` 得到配置目录再加 `config.toml`。只读取第一个存在的文件；读取后 trim Token 并拒绝空值。错误必须携带实际配置路径但不能携带配置内容。

- [ ] **步骤 4：运行配置读取 GREEN 测试**

执行 `cargo test config::tests::load --lib`，预期全部通过。

- [ ] **步骤 5：写 `config init` 失败测试**

覆盖：生成精确模板、创建 system 父目录、任一位置已存在时拒绝且内容不变、portable 创建失败时回退 system、两个位置失败时同时报告候选路径。

- [ ] **步骤 6：运行初始化 RED 测试**

执行 `cargo test config::tests::init --lib`，确认因初始化行为缺失而失败。

- [ ] **步骤 7：实现仅创建与回退**

先检查两个候选路径是否存在；任一存在即返回 AlreadyExists。使用 `OpenOptions::create_new(true)` 创建模板：

```toml
[tmdb]
api_token = ""
language = "zh-CN"
```

portable 创建失败后仅在属于不可写/目录不可创建一类文件系统错误时尝试 system；绝不覆盖并发创建的文件。

- [ ] **步骤 8：验证任务二**

执行：

```bash
cargo test config --lib
cargo fmt --check
git diff --check
```

---

### 任务三：定义图片策略并实现 TMDB 元数据请求

**涉及文件：**

- 创建：`src/providers.rs`
- 创建：`src/providers/tmdb.rs`
- 修改：`src/lib.rs`

**接口：**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artwork {
    pub background_url: Url,
    pub cover_url: Url,
}

pub trait ArtworkProvider {
    fn artwork(&self, id: &ResourceId) -> Result<Artwork, ProviderError>;
}

pub struct TmdbProvider {
    client: reqwest::blocking::Client,
    api_base: Url,
    token: SecretToken,
    language: String,
}
```

`SecretToken` 的 `Debug` 和 `Display` 不得输出原值。生产构造器固定使用 TMDB API 地址；测试构造器允许注入本地模拟 API 地址。图片安全基础地址必须来自 TMDB `/3/configuration` 响应的 `images.secure_base_url`，不得在业务代码中另行硬编码。

- [ ] **步骤 1：写 movie/TV 请求失败测试**

使用本地模拟服务器分别断言：

- movie 请求路径为 `/3/movie/550`。
- TV 请求路径为 `/3/tv/1399`。
- 每次操作请求 `/3/configuration` 并读取 `images.secure_base_url` 与图片尺寸列表。
- 查询参数含配置语言。
- Authorization 为 Bearer 测试 Token。
- 响应中的 `/backdrop.jpg`、`/poster.jpg` 被构造成 `/t/p/original/...` URL。

- [ ] **步骤 2：运行 TMDB RED 测试**

执行 `cargo test tmdb::tests::fetches --lib`，确认因策略尚未实现而失败。

- [ ] **步骤 3：实现策略与最小响应类型**

详情响应只反序列化 `backdrop_path` 和 `poster_path`；配置响应只反序列化 `images.secure_base_url`、`poster_sizes` 和 `backdrop_sizes`。确认两个尺寸列表都包含 `original` 后再构造 URL。根据 `MediaType` 选择明确详情端点，不探测另一类型。客户端设置连接超时、总超时和有限重定向；所有 API 请求带 Bearer 头，详情请求带 `language`。

- [ ] **步骤 4：运行 TMDB GREEN 测试**

执行 `cargo test tmdb::tests::fetches --lib`，预期通过。

- [ ] **步骤 5：写 TMDB 错误失败测试**

分别模拟 401、404、429、500、无效 JSON、缺 backdrop、缺 poster、配置缺少 `original`、连接失败和超时。断言错误分类可操作，且 `format!("{error:?} {error}")` 不包含测试 Token。

- [ ] **步骤 6：实现错误映射并验证**

执行：

```bash
cargo test tmdb --lib
cargo fmt --check
git diff --check
```

预期：所有状态和内容错误测试通过，测试不访问公网。

---

### 任务四：实现两张图片的事务式下载与替换

**涉及文件：**

- 创建：`src/artwork.rs`
- 创建：`src/artwork/download.rs`
- 修改：`src/lib.rs`

**接口：**

```rust
pub struct DownloadedArtwork {
    pub background: PathBuf,
    pub cover: PathBuf,
}

pub trait BinaryFetcher {
    fn fetch_to(&self, url: &Url, destination: &mut File) -> Result<(), DownloadError>;
}

pub fn install_artwork(
    fetcher: &dyn BinaryFetcher,
    artwork: &Artwork,
    output_parent: &Path,
) -> Result<DownloadedArtwork, DownloadError>;
```

- [ ] **步骤 1：写目录和成功安装失败测试**

使用内存可控的 fake fetcher 与临时目录，断言创建 `background/background.jpg` 和 `cover/cover.jpg`，内容分别匹配模拟 backdrop/poster 字节，返回值是两个规范化后的绝对路径。

- [ ] **步骤 2：运行下载 RED 测试**

执行 `cargo test download::tests::installs --lib`，确认因安装函数缺失而失败。

- [ ] **步骤 3：实现临时下载与首次安装**

使用 `tempfile::NamedTempFile::new_in` 在两个目标目录创建唯一临时文件。两次 `fetch_to` 和 flush/sync 均成功后才持久化为正式文件；失败时由临时文件析构清理。

- [ ] **步骤 4：运行首次安装 GREEN 测试**

执行 `cargo test download::tests::installs --lib`，预期通过。

- [ ] **步骤 5：写覆盖、失败保留和清理测试**

覆盖：已有两张旧图时成功替换；第二张下载失败时两张旧图不变；替换第二张失败时恢复旧文件；成功和失败后没有本次 `.tmp`/`.bak` 残留。通过注入受控文件操作触发替换失败，不依赖平台权限碰运气。

- [ ] **步骤 6：实现备份、替换与回滚状态机**

每个目标只操作本次生成的唯一临时/备份路径。先完成两次网络写入，再备份旧文件，再逐个替换；失败时根据已完成步骤逆序恢复。回滚错误要与原始错误一起报告，不能静默吞掉，也不能删除归属不明文件。

- [ ] **步骤 7：实现 reqwest 流式 fetcher**

生产 `BinaryFetcher` 使用 blocking response 的流式复制写入文件，先检查 HTTP 成功状态；图片响应失败按下载阶段报告，不在内存中聚合完整图片。

- [ ] **步骤 8：验证任务四**

执行：

```bash
cargo test download --lib
cargo fmt --check
git diff --check
```

---

### 任务五：接入 CLI 与 `config init`

**涉及文件：**

- 创建：`src/cli.rs`
- 修改：`src/main.rs`
- 修改：`src/lib.rs`
- 修改：`tests/cli.rs`

**CLI 类型：**

```rust
#[derive(Parser)]
pub struct Cli {
    #[arg(short = 'i', long, requires = "output")]
    pub id: Option<String>,
    #[arg(short = 'o', long, requires = "id")]
    pub output: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command { Config { #[command(subcommand)] command: ConfigCommand } }

#[derive(Subcommand)]
pub enum ConfigCommand { Init }
```

- [ ] **步骤 1：先扩展 CLI 失败测试**

在现有集成测试中加入：`-i` 缹 `-o`、`-o` 缺 `-i`、下载参数与 `config` 子命令冲突、非法 ID 非零退出、帮助中包含 `-i/--id`、`-o/--output` 和 `config`；保留四个现有版本/帮助测试。

- [ ] **步骤 2：运行 CLI RED 测试**

执行 `cargo test --test cli`，确认新增测试因新参数缺失而失败，原有测试仍通过。

- [ ] **步骤 3：实现 CLI 结构与命令分派**

`main` 只负责调用可测试的 `run` 并将错误写到 stderr、返回非零。下载分派顺序必须是：解析 ID → 解析配置路径 → 加载配置 → 构造策略 → 获取 Artwork → install_artwork → 输出两个绝对路径。非法 ID 必须在配置和网络之前失败。

- [ ] **步骤 4：运行 CLI GREEN 测试**

执行 `cargo test --test cli`，确认原有和新增解析测试通过。

- [ ] **步骤 5：写 `config init` 命令失败测试**

为可测试的 `run_with_environment` 注入临时“可执行文件路径”和“系统配置路径”，验证创建模板、输出实际绝对路径、重复初始化非零且不覆盖内容。

- [ ] **步骤 6：接入初始化并验证**

执行：

```bash
cargo test cli
cargo test --test cli
cargo fmt --check
git diff --check
```

---

### 任务六：端到端本地 HTTP 集成测试

**涉及文件：**

- 创建：`tests/tmdb_cli.rs`
- 修改：`src/cli.rs`，加入可注入的配置路径和 TMDB API 地址运行环境。
- 修改：`src/providers/tmdb.rs`，让测试构造器使用本地模拟 API 地址。
- 修改：`src/artwork/download.rs`，让应用层注入生产或测试 `BinaryFetcher`。

- [ ] **步骤 1：写电影下载端到端失败测试**

使用本地 HTTP 模拟服务与临时配置/输出目录，通过可注入运行环境调用真实 CLI 应用层，模拟 movie 详情和两张图片响应，断言固定文件、原始字节、请求认证/语言、成功输出绝对路径。

- [ ] **步骤 2：运行 movie RED 测试**

执行 `cargo test --test tmdb_cli movie_download`，确认因端到端接线缺失而失败。

- [ ] **步骤 3：补齐最小接线并运行 GREEN**

执行同一命令，预期通过；不得为测试增加会暴露 Token 的公开 CLI 参数。

- [ ] **步骤 4：写 TV、错误和旧文件保护测试**

覆盖 TV 端点、401/404/429、缺图、第二张图片失败以及失败后旧文件不变。每个测试使用独立临时目录和模拟服务器。

- [ ] **步骤 5：运行端到端 GREEN 测试**

执行 `cargo test --test tmdb_cli`，预期所有测试通过且没有公网请求。

- [ ] **步骤 6：回归全部测试**

执行：

```bash
cargo test
cargo fmt --check
git diff --check
```

---

### 任务七：完整验收与范围复核

**涉及文件：**

- 复核：本阶段所有修改和新增文件

- [ ] **步骤 1：静态与测试验证**

依次执行：

```bash
cargo fmt --check
cargo check
cargo test
git diff --check
```

预期：命令全部成功，无警告、失败或空白错误。

- [ ] **步骤 2：离线 CLI 验证**

执行：

```bash
cargo run -- -v
cargo run -- --version
cargo run -- --help
cargo run --
cargo run -- -i invalid -o /tmp/crabgrab-invalid-check
```

预期：版本输出一致；帮助包含下载参数和 config；无参数非零并显示帮助；非法 ID 在读取配置或请求网络前失败。

- [ ] **步骤 3：配置模板人工验收**

不要在仓库可执行文件旁直接运行可能写入真实配置的命令。通过自动化测试证据确认模板为：

```toml
[tmdb]
api_token = ""
language = "zh-CN"
```

并确认 Token 不在 `git diff`、测试日志或错误快照中。

- [ ] **步骤 4：范围和待提交文件复核**

执行：

```bash
git status --short
git diff --stat
git diff
```

确认 `Cargo.lock` 继续跟踪；没有 `target/`、真实 `config.toml`、Token、下载图片、临时文件或范围外功能进入待提交内容；文档重命名和 `02` 设计/计划文件均在预期范围内。

- [ ] **步骤 5：独立代码审查**

使用 `superpowers:requesting-code-review` 对设计、计划、实现和测试做只读审查。Critical 与 Important 问题必须修复并重新验证；Minor 问题记录给用户决定。

## 完成标准

- 七个任务全部完成且每个 RED 测试都曾因对应行为缺失而失败。
- movie 与 TV/综艺图片下载在本地模拟服务端到端测试中通过。
- 两张 original 图片固定保存到 `background/background.jpg` 与 `cover/cover.jpg`。
- 双路径配置、初始化防覆盖、Token 脱敏和事务式更新均有自动化测试。
- 现有版本与帮助行为无回归。
- 全部格式、编译、测试和差异检查通过。
- 工作区保持未暂存、未提交，由用户人工复核并提交。
